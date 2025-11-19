use std::{
    error::Error,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use lazy_regex::Regex;
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, error, instrument, trace, warn};

use crate::utils::{get_sys_proxy, resolve_binary_path};

static CONNECTION_TIMEOUT: &str = "5s";
static IO_IDLE_TIMEOUT: &str = "30s";

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
    // pub count: u64,
    pub bytes: u64,
    // pub sizeless: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RcloneTransferStats {
    pub bytes: u64,
    pub total_bytes: u64,
    // pub elapsed_time: f64,
    // pub eta: Option<u64>,
    #[serde(deserialize_with = "deserialize_speed")]
    pub speed: u64,
}

fn deserialize_speed<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let speed = f64::deserialize(deserializer)?;
    Ok(speed as u64)
}

#[derive(Debug, Clone, Deserialize)]
struct RcloneStatLine {
    stats: RcloneTransferStats,
}

#[derive(Debug)]
enum RcloneTransferOperation {
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
struct RcloneClient {
    rclone_path: PathBuf,
    config_path: PathBuf,
    sys_proxy: Option<String>,
    bandwidth_limit: String,
}

impl RcloneClient {
    #[instrument(level = "debug", fields(sys_proxy), ret)]
    fn new(rclone_path: PathBuf, config_path: PathBuf, bandwidth_limit: String) -> Self {
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

        // Make any unexpected prompts fail to avoid hanging
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
        let output = self.command(args, false).output().await.context("rclone command failed")?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            trace!(stdout, "rclone command successful");
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            error!(code = output.status.code().unwrap_or(-1), stderr, "rclone command failed");
            bail!(
                "rclone returned exit code {}, stderr:\n{}",
                output.status.code().map_or("unknown".to_string(), |c| c.to_string()),
                stderr
            )
        }
    }

    #[instrument(skip(self), level = "debug")]
    async fn remotes(&self) -> Result<Vec<String>> {
        let output = self.run_to_string(&["listremotes"]).await?;
        let remotes: Vec<String> =
            output.lines().map(|line| line.trim().trim_end_matches(':').to_string()).collect();
        Ok(remotes)
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    async fn lsjson(&self, path: &str) -> Result<Vec<RcloneLsJsonEntry>> {
        let output = self
            .run_to_string(&["lsjson", "--fast-list", path])
            .await
            .context("rclone lsjson failed")?;
        let entries: Vec<RcloneLsJsonEntry> =
            serde_json::from_str(&output).context("Failed to parse rclone lsjson output")?;
        Ok(entries)
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    async fn size(&self, path: &str) -> Result<RcloneSizeOutput> {
        let output = self.run_to_string(&["size", "--fast-list", "--json", path]).await?;
        let size_output: RcloneSizeOutput =
            serde_json::from_str(&output).context("Failed to parse rclone size output")?;
        Ok(size_output)
    }

    #[instrument(level = "debug", skip(self, cancellation_token), err)]
    async fn transfer(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        self.transfer_internal(source, dest, operation, None, None, cancellation_token).await
    }

    #[instrument(level = "debug", skip(self, stats_tx, cancellation_token), err)]
    async fn transfer_with_stats(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        total_bytes: u64,
        stats_tx: Option<UnboundedSender<RcloneTransferStats>>,
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
        stats_tx: Option<UnboundedSender<RcloneTransferStats>>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        ensure!(
            total_bytes.is_some() || stats_tx.is_none(),
            "total_bytes must be provided if stats_tx is provided"
        );

        let mut args = vec![
            operation.as_str(),
            "--stats",
            "0.5s",
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
            if let (Some(stats_tx), Some(total_bytes)) = (stats_tx, total_bytes) {
                while let Some(line) = lines.next_line().await? {
                    let line: String = line;
                    match serde_json::from_str::<RcloneStatLine>(&line) {
                        Ok(stat_line) => {
                            trace!(?stat_line, "parsed rclone stat line");
                            let mut stats = stat_line.stats;
                            stats.total_bytes = total_bytes;
                            trace!(?stats, "sending stats update");
                            if stats_tx.send(stats).is_err() {
                                warn!("Stats receiver dropped, stopping stats processing.");
                                break;
                            }
                        }
                        Err(e) => {
                            debug!(
                                line = line,
                                error = &e as &dyn Error,
                                "Error parsing rclone stat line"
                            );
                        }
                    }
                }
            }

            let status = child.wait().await?;
            match status.success() {
                true => Ok(()),
                false => {
                    let mut stderr_str = String::new();
                    while let Some(line) = lines.next_line().await? {
                        stderr_str.push_str(&line);
                    }
                    error!(code = status.code().unwrap_or(-1), stderr = %stderr_str, "rclone transfer failed");
                    Err(anyhow!(
                        "rclone failed with exit code: {}, stderr: {}",
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
                    Err(anyhow!("Download cancelled by user"))
                }
            }
        } else {
            transfer_future.await
        }
    }
}

fn filter_remotes_with_regex(remotes: Vec<String>, regex: Option<&Regex>) -> Vec<String> {
    if let Some(re) = regex {
        remotes.into_iter().filter(|r| re.is_match(r)).collect()
    } else {
        remotes
    }
}

fn filter_remotes_with_pattern(remotes: Vec<String>, pattern: Option<&str>) -> Vec<String> {
    if let Some(pat) = pattern {
        match Regex::new(pat) {
            Ok(re) => filter_remotes_with_regex(remotes, Some(&re)),
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

#[derive(Debug, Clone)]
pub(super) struct RcloneStorage {
    client: RcloneClient,
    remote: String,
    root_dir: String,
    // Keep original string for equality, compile once for runtime use
    remote_filter_regex_str: Option<String>,
    remote_filter_regex: Option<Regex>,
}

impl RcloneStorage {
    #[instrument]
    pub(super) fn new(
        rclone_path: PathBuf,
        config_path: PathBuf,
        root_dir: String,
        remote: String,
        bandwidth_limit: String,
        remote_filter_regex: Option<String>,
    ) -> Self {
        let compiled = match &remote_filter_regex {
            Some(pat) => match Regex::new(pat) {
                Ok(r) => Some(r),
                Err(e) => {
                    warn!(pattern = %pat, error = &e as &dyn Error, "Invalid remote filter regex, ignoring");
                    None
                }
            },
            None => None,
        };
        Self {
            client: RcloneClient::new(rclone_path, config_path, bandwidth_limit),
            remote,
            root_dir,
            remote_filter_regex_str: remote_filter_regex,
            remote_filter_regex: compiled,
        }
    }

    fn format_remote_path(&self, path: &str) -> String {
        format!(
            "{}:{}",
            self.remote,
            PathBuf::from(self.root_dir.trim_end_matches(['/', '\\'])).join(path).display()
        )
    }

    /// Upload a single local file to an arbitrary remote and path.
    ///
    /// The file name from `local_path` is appended to `remote_dir`.
    #[instrument(level = "debug", skip(self, cancellation_token), err)]
    pub(super) async fn upload_file_to_remote(
        &self,
        local_path: &Path,
        remote: &str,
        remote_dir: &str,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        ensure!(local_path.is_file(), "Local path is not a file: {}", local_path.display());
        ensure!(!remote.is_empty(), "Remote name must not be empty");

        let file_name = local_path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            anyhow!("Local path has no valid UTF-8 file name: {}", local_path.display())
        })?;

        let remote_dir_path = if remote_dir.is_empty() {
            PathBuf::from(file_name)
        } else {
            PathBuf::from(remote_dir.trim_start_matches(['/', '\\'])).join(file_name)
        };

        let dest = format!("{remote}:{}", remote_dir_path.display());
        debug!(src = %local_path.display(), dest = %dest, "Starting rclone upload");

        self.client
            .transfer(
                local_path.display().to_string(),
                dest,
                RcloneTransferOperation::Copy,
                cancellation_token,
            )
            .await
    }

    #[instrument(level = "debug", skip(self, stats_tx, cancellation_token), err, ret)]
    pub(super) async fn download_dir_with_stats(
        &self,
        source: String,
        dest: PathBuf,
        stats_tx: UnboundedSender<RcloneTransferStats>,
        cancellation_token: CancellationToken,
    ) -> Result<PathBuf> {
        ensure!(dest.parent().is_some(), "Destination must have a parent directory");
        let source = self.format_remote_path(&source);
        let total_bytes =
            self.client.size(&source).await.context("Failed to get remote dir size")?.bytes;
        self.client
            .transfer_with_stats(
                source,
                dest.display().to_string(),
                RcloneTransferOperation::Sync,
                total_bytes,
                Some(stats_tx),
                Some(cancellation_token),
            )
            .await
            .map(|_| dest)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    pub(super) async fn download_file(
        &self,
        source: String,
        dest: PathBuf,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<PathBuf> {
        ensure!(dest.is_dir(), "Destination must be a directory");
        let local_leaf = PathBuf::from(&source)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.clone());
        let source = self.format_remote_path(&source);
        let mut dest_path = dest.clone();
        debug!(source = %source, dest = %dest.display(), "Starting file download");
        self.client
            .transfer(
                source.clone(),
                dest.display().to_string(),
                RcloneTransferOperation::Copy,
                cancellation_token,
            )
            .await?;
        dest_path.push(local_leaf);
        Ok(dest_path)
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    pub(super) async fn list_dir_json(&self, source: String) -> Result<Vec<RcloneLsJsonEntry>> {
        let remote_path = self.format_remote_path(&source);
        self.client.lsjson(&remote_path).await
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    pub(super) async fn remotes(&self) -> Result<Vec<String>> {
        let remotes = self.client.remotes().await?;
        Ok(filter_remotes_with_regex(remotes, self.remote_filter_regex.as_ref()))
    }
}

impl PartialEq for RcloneStorage {
    fn eq(&self, other: &Self) -> bool {
        self.client == other.client
            && self.remote == other.remote
            && self.root_dir == other.root_dir
            && self.remote_filter_regex_str == other.remote_filter_regex_str
    }
}

impl Eq for RcloneStorage {}

#[instrument(level = "debug", ret, err)]
pub(super) async fn list_remotes(
    rclone_path: &Path,
    config_path: &Path,
    remote_filter_regex: Option<&str>,
) -> Result<Vec<String>> {
    let client =
        RcloneClient::new(rclone_path.to_path_buf(), config_path.to_path_buf(), String::new());
    let remotes = client.remotes().await?;
    Ok(filter_remotes_with_pattern(remotes, remote_filter_regex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_equality_reflects_bandwidth_limit() {
        let base = RcloneStorage::new(
            PathBuf::from("rclone"),
            PathBuf::from("config"),
            "root".to_string(),
            "remote".to_string(),
            "".to_string(),
            None,
        );
        let same = RcloneStorage::new(
            PathBuf::from("rclone"),
            PathBuf::from("config"),
            "root".to_string(),
            "remote".to_string(),
            "".to_string(),
            None,
        );
        let with_limit = RcloneStorage::new(
            PathBuf::from("rclone"),
            PathBuf::from("config"),
            "root".to_string(),
            "remote".to_string(),
            "2M".to_string(),
            None,
        );

        assert_eq!(base, same, "identical bandwidth limits should be equal");
        assert_ne!(base, with_limit, "changing bandwidth limit should change storage equality");
    }
}
