use std::{
    error::Error,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use lazy_regex::Regex;
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
    time::{self, Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;
use tracing::{Span, error, instrument, trace, warn};

use crate::downloader::{TransferSpeedTracker, TransferStats};
use crate::utils::{get_sys_proxy, resolve_binary_path};

static CONNECTION_TIMEOUT: &str = "5s";
static IO_IDLE_TIMEOUT: &str = "30s";
const RCLONE_STATS_INTERVAL: Duration = Duration::from_millis(500);
const RCLONE_STALE_SPEED_TIMEOUT: Duration = Duration::from_millis(1500);
const RCLONE_SPEED_SAMPLE_WINDOW: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(unused)]
pub(super) struct RcloneLsJsonEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub mod_time: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RcloneSizeOutput {
    pub bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RcloneTransferStats {
    bytes: u64,
    // total_bytes: u64,
    // #[serde(deserialize_with = "deserialize_speed")]
    // speed: u64,
}

// fn deserialize_speed<'de, D>(deserializer: D) -> Result<u64, D::Error>
// where
//     D: serde::Deserializer<'de>,
// {
//     let speed = f64::deserialize(deserializer)?;
//     Ok(speed as u64)
// }

#[derive(Debug, Clone, Deserialize)]
struct RcloneJsonLogLine {
    time: String,
    level: String,
    msg: String,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    stats: Option<RcloneTransferStats>,
}

#[derive(Debug)]
struct RcloneProgressTracker {
    speed_tracker: TransferSpeedTracker,
    started_at: Instant,
    expected_total_bytes: u64,
    last_stats: Option<TransferStats>,
    last_update_at: Option<Instant>,
}

impl RcloneProgressTracker {
    fn new(expected_total_bytes: u64) -> Self {
        Self {
            speed_tracker: TransferSpeedTracker::new(RCLONE_SPEED_SAMPLE_WINDOW),
            started_at: Instant::now(),
            expected_total_bytes,
            last_stats: None,
            last_update_at: None,
        }
    }

    fn record_stats(&mut self, stats: RcloneTransferStats) -> TransferStats {
        let speed = self.speed_tracker.record(stats.bytes, self.started_at.elapsed().as_millis());
        let normalized = TransferStats {
            bytes: stats.bytes,
            total_bytes: (stats.bytes <= self.expected_total_bytes)
                .then_some(self.expected_total_bytes),
            speed,
        };
        self.last_update_at = Some(Instant::now());
        self.last_stats = Some(normalized.clone());
        normalized
    }

    fn maybe_stale_stats(&mut self, now: Instant) -> Option<TransferStats> {
        let last_update_at = self.last_update_at?;
        if now.duration_since(last_update_at) < RCLONE_STALE_SPEED_TIMEOUT {
            return None;
        }

        let last_stats = self.last_stats.as_mut()?;
        if last_stats.speed == 0 {
            return None;
        }

        last_stats.speed = 0;
        self.last_update_at = Some(now);
        Some(last_stats.clone())
    }
}

impl RcloneJsonLogLine {
    /// Converts to human-readable format like "2025/12/03 16:25:39 ERROR : object: message"
    fn to_human_readable(&self) -> String {
        let formatted_time = self.format_time();
        let level_upper = self.level.to_uppercase();
        match &self.object {
            Some(obj) => {
                format!("{} {} : {}: {}", formatted_time, level_upper, obj, self.msg.trim())
            }
            None => format!("{} {} : {}", formatted_time, level_upper, self.msg.trim()),
        }
    }

    /// Formats ISO timestamp to rclone's default format.
    /// Input: "2025-12-03T16:18:24.677508041+03:00"
    /// Output: "2025/12/03 16:18:24"
    fn format_time(&self) -> String {
        if let Some(t_pos) = self.time.find('T') {
            let date_part = &self.time[..t_pos];
            let time_start = t_pos + 1;
            let time_end = (time_start + 8).min(self.time.len());
            let time_part = &self.time[time_start..time_end];
            let formatted_date = date_part.replace('-', "/");
            format!("{} {}", formatted_date, time_part)
        } else {
            self.time.clone()
        }
    }
}

/// Converts a JSON log line to human-readable format, or returns the original line if parsing fails.
fn convert_json_log_line(line: &str) -> String {
    match serde_json::from_str::<RcloneJsonLogLine>(line) {
        Ok(log_line) => log_line.to_human_readable(),
        Err(_) => line.to_string(),
    }
}

#[derive(Debug)]
pub(super) enum RcloneTransferOperation {
    Copy,
    Sync,
}

impl RcloneTransferOperation {
    fn as_str(&self) -> &str {
        match self {
            RcloneTransferOperation::Copy => "copy",
            RcloneTransferOperation::Sync => "sync",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RcloneCli {
    rclone_path: PathBuf,
    config_path: PathBuf,
    sys_proxy: Option<String>,
    bandwidth_limit: String,
}

impl RcloneCli {
    #[instrument(level = "debug", fields(sys_proxy), ret)]
    pub(super) fn new(rclone_path: PathBuf, config_path: PathBuf, bandwidth_limit: String) -> Self {
        let sys_proxy = get_sys_proxy();
        let resolved_path =
            match resolve_binary_path(Some(&rclone_path.to_string_lossy()), "rclone") {
                Ok(p) => p,
                Err(e) => {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        original = %rclone_path.display(),
                        "Failed to resolve rclone path, using provided value"
                    );
                    rclone_path
                }
            };
        Span::current().record("sys_proxy", sys_proxy.as_deref());
        Self { rclone_path: resolved_path, config_path, sys_proxy, bandwidth_limit }
    }

    #[instrument(skip(self), level = "debug")]
    fn command(&self, args: &[&str], use_json_log: bool) -> Command {
        let mut command = Command::new(&self.rclone_path);
        command.kill_on_drop(true);
        // Avoid hangs on unexpected input prompts
        command.stdin(Stdio::null());

        // Hide the console window on Windows
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW

        if let Some(proxy) = &self.sys_proxy {
            trace!(proxy, "Using system proxy");
            command.env("http_proxy", proxy);
            command.env("https_proxy", proxy);
        }

        command.arg("--config").arg(&self.config_path);
        if use_json_log {
            command.arg("--use-json-log");
        }

        command.args(["--contimeout", CONNECTION_TIMEOUT, "--timeout", IO_IDLE_TIMEOUT]);
        command.args(args);
        trace!(command = ?command, "Constructed rclone command");
        command
    }

    #[instrument(skip(self), level = "debug")]
    async fn run_to_string(&self, args: &[&str]) -> Result<String> {
        let output = self.command(args, false).output().await.context("Rclone command failed")?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            trace!(stdout, "Rclone command successful");
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            error!(code = output.status.code().unwrap_or(-1), stderr, "Rclone command failed");
            bail!(
                "Rclone returned exit code {}, stderr:\n{}",
                output.status.code().map_or("unknown".to_string(), |c| c.to_string()),
                stderr
            )
        }
    }

    #[instrument(skip(self), level = "debug")]
    pub(super) async fn remotes(&self) -> Result<Vec<String>> {
        let output = self.run_to_string(&["listremotes"]).await?;
        Ok(output.lines().map(|line| line.trim().trim_end_matches(':').to_string()).collect())
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    pub(super) async fn size(&self, path: &str) -> Result<RcloneSizeOutput> {
        // TODO: can `--check-first` be used to make `total_bytes` reliable instead?
        let output = self.run_to_string(&["size", "--fast-list", "--json", path]).await?;
        let size_output: RcloneSizeOutput =
            serde_json::from_str(&output).context("Failed to parse rclone size output")?;
        Ok(size_output)
    }

    #[instrument(level = "debug", skip(self, cancellation_token))]
    pub(super) async fn transfer(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        self.transfer_internal(source, dest, operation, None, None, cancellation_token).await
    }

    #[instrument(level = "debug", skip(self, stats_tx, cancellation_token))]
    pub(super) async fn transfer_with_stats(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        total_bytes: u64,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        self.transfer_internal(
            source,
            dest,
            operation,
            Some(total_bytes),
            stats_tx,
            cancellation_token,
        )
        .await
    }

    #[instrument(level = "debug", skip(self, stats_tx, cancellation_token))]
    async fn transfer_internal(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        total_bytes: Option<u64>,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        ensure!(
            total_bytes.is_some() || stats_tx.is_none(),
            "total_bytes must be provided if stats_tx is provided"
        );

        let mut args = vec![
            operation.as_str(),
            "--stats",
            if stats_tx.is_some() { "0.5s" } else { "0" },
            "--stats-log-level",
            "NOTICE",
            "--fast-list",
            "--retries",
            "3",
            "--transfers",
            "8",
        ];

        if !self.bandwidth_limit.is_empty() {
            args.extend_from_slice(&["--bwlimit", &self.bandwidth_limit]);
        }

        args.extend_from_slice(&[&source, &dest]);

        let use_json_log = stats_tx.is_some();
        let mut child = self.command(&args, use_json_log).stderr(Stdio::piped()).spawn()?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;
        let mut lines = BufReader::new(stderr).lines();

        let transfer_future = async {
            // Collect non-stat lines for error reporting
            let mut stderr_lines: Vec<String> = Vec::new();

            if let (Some(stats_tx), Some(total_bytes)) = (stats_tx, total_bytes) {
                let mut progress_tracker = RcloneProgressTracker::new(total_bytes);
                let mut stale_tick = time::interval(RCLONE_STATS_INTERVAL);
                stale_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
                stale_tick.tick().await;

                loop {
                    tokio::select! {
                        line = lines.next_line() => {
                            let Some(line) = line? else {
                                break;
                            };

                            match serde_json::from_str::<RcloneJsonLogLine>(&line) {
                                Ok(log_line) => {
                                    if let Some(stats) = log_line.stats {
                                        trace!(?stats, "Parsed rclone stats");
                                        let normalized = progress_tracker.record_stats(stats);
                                        trace!(?normalized, "Sending stats update");
                                        if stats_tx.send(normalized).is_err() {
                                            warn!("Stats receiver dropped, stopping stats processing.");
                                            break;
                                        }
                                    } else {
                                        stderr_lines.push(log_line.to_human_readable());
                                    }
                                }
                                Err(_) => {
                                    stderr_lines.push(line);
                                }
                            }
                        }
                        _ = stale_tick.tick() => {
                            if let Some(stale_stats) = progress_tracker.maybe_stale_stats(Instant::now()) {
                                trace!(?stale_stats, "Sending stale speed reset");
                                if stats_tx.send(stale_stats).is_err() {
                                    warn!("Stats receiver dropped, stopping stale stats processing.");
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            let status = child.wait().await?;
            match status.success() {
                true => Ok(()),
                false => {
                    while let Some(line) = lines.next_line().await? {
                        if use_json_log {
                            stderr_lines.push(convert_json_log_line(&line));
                        } else {
                            stderr_lines.push(line);
                        }
                    }
                    let stderr_str = stderr_lines.join("\n");
                    error!(code = status.code().unwrap_or(-1), stderr = %stderr_str, "Rclone transfer failed");
                    Err(anyhow!(
                        "Rclone failed with exit code: {}, stderr: {}",
                        status.code().map_or("unknown".to_string(), |c| c.to_string()),
                        stderr_str
                    ))
                }
            }
        };

        if let Some(token) = cancellation_token {
            tokio::select! {
                res = transfer_future => res,
                _ = token.cancelled() => {
                    warn!("Rclone transfer cancelled by token");
                    child.kill().await.context("Failed to kill rclone process")?;
                    Err(anyhow!("Download cancelled"))
                }
            }
        } else {
            transfer_future.await
        }
    }
}

fn filter_remotes_with_regex(remotes: Vec<String>, pattern: Option<&str>) -> Vec<String> {
    if let Some(pat) = pattern {
        match Regex::new(pat) {
            Ok(re) => remotes.into_iter().filter(|r| re.is_match(r)).collect(),
            Err(e) => {
                warn!(
                    pattern = %pat,
                    error = &e as &dyn Error,
                    "Invalid remote filter regex, returning unfiltered remotes"
                );
                remotes
            }
        }
    } else {
        remotes
    }
}

#[instrument(level = "debug", ret, err)]
pub(crate) async fn list_remotes(
    rclone_path: &Path,
    config_path: &Path,
    remote_filter_regex: Option<&str>,
) -> Result<Vec<String>> {
    let cli = RcloneCli::new(rclone_path.to_path_buf(), config_path.to_path_buf(), String::new());
    let remotes = cli.remotes().await?;
    Ok(filter_remotes_with_regex(remotes, remote_filter_regex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_tracker_derives_speed_from_bytes() {
        let mut tracker = RcloneProgressTracker::new(100);

        let first = tracker.record_stats(RcloneTransferStats { bytes: 25 });
        std::thread::sleep(Duration::from_millis(20));
        let second = tracker.record_stats(RcloneTransferStats { bytes: 25 });

        assert_eq!(first.total_bytes, Some(100));
        assert!(second.speed <= first.speed);
    }

    #[test]
    fn progress_tracker_marks_progress_unknown_when_bytes_exceed_expected_total() {
        let mut tracker = RcloneProgressTracker::new(100);

        let stats = tracker.record_stats(RcloneTransferStats { bytes: 120 });

        assert_eq!(stats.total_bytes, None);
    }

    #[test]
    fn progress_tracker_emits_zero_speed_after_stall() {
        let mut tracker = RcloneProgressTracker::new(100);
        std::thread::sleep(Duration::from_millis(20));
        let recorded = tracker.record_stats(RcloneTransferStats { bytes: 50 });
        assert!(recorded.speed > 0);
        tracker.last_update_at = Some(Instant::now() - RCLONE_STALE_SPEED_TIMEOUT);

        let stats = tracker.maybe_stale_stats(Instant::now()).expect("stale stats");

        assert_eq!(stats.speed, 0);
        assert_eq!(stats.bytes, 50);
    }

    #[test]
    fn parse_json_log_line_with_object() {
        let json = r#"{"time":"2025-12-03T16:18:24.49104384+03:00","level":"error","msg":"error reading source root directory: directory not found","object":"webdav root 'Quest Games/A Fishermans Tale v16+1.064 -QU'","objectType":"*webdav.Fs","source":"slog/logger.go:256"}"#;
        let parsed: RcloneJsonLogLine = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.level, "error");
        assert_eq!(parsed.msg, "error reading source root directory: directory not found");
        assert_eq!(
            parsed.object,
            Some("webdav root 'Quest Games/A Fishermans Tale v16+1.064 -QU'".to_string())
        );
        assert!(parsed.stats.is_none());
    }

    #[test]
    fn parse_json_log_line_without_object() {
        let json = r#"{"time":"2025-12-03T16:18:24.491141917+03:00","level":"error","msg":"Attempt 1/3 failed with 1 errors and: directory not found","source":"slog/logger.go:256"}"#;
        let parsed: RcloneJsonLogLine = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.level, "error");
        assert_eq!(parsed.msg, "Attempt 1/3 failed with 1 errors and: directory not found");
        assert!(parsed.object.is_none());
        assert!(parsed.stats.is_none());
    }

    #[test]
    fn parse_json_log_line_with_stats() {
        let json = r#"{"time":"2025-12-03T16:36:50.513851561+03:00","level":"info","msg":"\nTransferred: ...","stats":{"bytes":39841792,"checks":0,"deletedDirs":0,"deletes":0,"elapsedTime":2.000537856,"errors":0,"eta":3,"fatalError":false,"listed":1,"renames":0,"retryError":false,"serverSideCopies":0,"serverSideCopyBytes":0,"serverSideMoveBytes":0,"serverSideMoves":0,"speed":19920887.154321734,"totalBytes":107369499,"totalChecks":0,"totalTransfers":1,"transferTime":1.907390027,"transfers":0},"source":"slog/logger.go:256"}"#;
        let parsed: RcloneJsonLogLine = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.level, "info");
        assert!(parsed.stats.is_some());

        let stats = parsed.stats.unwrap();
        assert_eq!(stats.bytes, 39841792);
    }

    #[test]
    fn to_human_readable_with_object() {
        let log_line = RcloneJsonLogLine {
            time: "2025-12-03T16:18:24.49104384+03:00".to_string(),
            level: "error".to_string(),
            msg: "error reading source root directory: directory not found".to_string(),
            object: Some("webdav root 'Quest Games/Test'".to_string()),
            stats: None,
        };

        let human = log_line.to_human_readable();
        assert_eq!(
            human,
            "2025/12/03 16:18:24 ERROR : webdav root 'Quest Games/Test': error reading source \
             root directory: directory not found"
        );
    }

    #[test]
    fn to_human_readable_without_object() {
        let log_line = RcloneJsonLogLine {
            time: "2025-12-03T16:18:24.491141917+03:00".to_string(),
            level: "error".to_string(),
            msg: "Attempt 1/3 failed with 1 errors and: directory not found".to_string(),
            object: None,
            stats: None,
        };

        let human = log_line.to_human_readable();
        assert_eq!(
            human,
            "2025/12/03 16:18:24 ERROR : Attempt 1/3 failed with 1 errors and: directory not found"
        );
    }

    #[test]
    fn to_human_readable_notice_level() {
        let log_line = RcloneJsonLogLine {
            time: "2025-12-03T16:18:24.677564739+03:00".to_string(),
            level: "notice".to_string(),
            msg: "Failed to sync: directory not found".to_string(),
            object: None,
            stats: None,
        };

        let human = log_line.to_human_readable();
        assert_eq!(human, "2025/12/03 16:18:24 NOTICE : Failed to sync: directory not found");
    }

    #[test]
    fn format_time_iso_to_rclone() {
        let log_line = RcloneJsonLogLine {
            time: "2025-12-03T16:18:24.677508041+03:00".to_string(),
            level: "info".to_string(),
            msg: "test".to_string(),
            object: None,
            stats: None,
        };

        assert_eq!(log_line.format_time(), "2025/12/03 16:18:24");
    }

    #[test]
    fn convert_json_log_line_valid() {
        let json = r#"{"time":"2025-12-03T16:25:39.000000000+03:00","level":"error","msg":"test message","source":"test"}"#;
        let result = convert_json_log_line(json);
        assert_eq!(result, "2025/12/03 16:25:39 ERROR : test message");
    }

    #[test]
    fn convert_json_log_line_invalid_returns_original() {
        let invalid = "not valid json at all";
        let result = convert_json_log_line(invalid);
        assert_eq!(result, invalid);
    }
}
