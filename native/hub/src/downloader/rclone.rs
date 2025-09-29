use std::{error::Error, path::PathBuf, process::Stdio};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, error, info, instrument, trace, warn};

use crate::utils::get_sys_proxy;

static CONNECTION_TIMEOUT: &str = "5s";
static IO_IDLE_TIMEOUT: &str = "30s";

#[derive(Debug, Clone, Deserialize)]
pub struct RcloneSizeOutput {
    // pub count: u64,
    pub bytes: u64,
    // pub sizeless: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcloneTransferStats {
    pub bytes: u64,
    pub total_bytes: u64,
    // pub elapsed_time: f64,
    pub eta: Option<u64>,
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
pub struct RcloneStatLine {
    pub stats: RcloneTransferStats,
}

#[derive(Debug)]
pub enum RcloneTransferOperation {
    Copy,
    Sync,
}

impl RcloneTransferOperation {
    pub fn as_str(&self) -> &str {
        match self {
            RcloneTransferOperation::Copy => "copy",
            RcloneTransferOperation::Sync => "sync",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RcloneClient {
    rclone_path: PathBuf,
    config_path: Option<PathBuf>,
    sys_proxy: Option<String>,
    bandwidth_limit: String,
}

impl RcloneClient {
    #[instrument(fields(sys_proxy), ret)]
    pub fn new(
        rclone_path: PathBuf,
        config_path: Option<PathBuf>,
        bandwidth_limit: String,
    ) -> Self {
        let sys_proxy = get_sys_proxy();
        Span::current().record("sys_proxy", sys_proxy.as_deref());
        Self { rclone_path, config_path, sys_proxy, bandwidth_limit }
    }

    #[instrument(skip(self), level = "trace")]
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.rclone_path);
        command.kill_on_drop(true);

        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW

        if let Some(proxy) = &self.sys_proxy {
            trace!(proxy, "Using system proxy");
            command.env("http_proxy", proxy);
            command.env("https_proxy", proxy);
        }

        if let Some(config_path) = &self.config_path {
            command.arg("--config").arg(config_path);
        }
        command.arg("--use-json-log");

        command.args(args);
        trace!(command = ?command, "Constructed rclone command");
        command
    }

    #[instrument(skip(self), level = "trace")]
    async fn run_to_string(&self, args: &[&str]) -> Result<String> {
        let output = self.command(args).output().await.context("rclone command failed")?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            trace!(stdout, "rclone command successful");
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            error!(code = output.status.code().unwrap_or(-1), stderr, "rclone command failed");
            Err(anyhow::anyhow!(
                "rclone returned exit code {}, stderr:\n{}",
                output.status.code().map_or("unknown".to_string(), |c| c.to_string()),
                stderr
            ))
        }
    }

    pub async fn remotes(&self) -> Result<Vec<String>> {
        let output = self.run_to_string(&["listremotes"]).await?;
        let remotes: Vec<String> =
            output.lines().map(|line| line.trim().trim_end_matches(':').to_string()).collect();
        Ok(remotes)
    }

    #[instrument(skip(self), ret, err)]
    pub async fn size(&self, path: &str) -> Result<RcloneSizeOutput> {
        let output = self.run_to_string(&["size", "--fast-list", "--json", path]).await?;
        let size_output: RcloneSizeOutput =
            serde_json::from_str(&output).context("Failed to parse rclone size output")?;
        Ok(size_output)
    }

    #[instrument(skip(self, cancellation_token), err)]
    pub async fn transfer(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        self.transfer_with_stats(source, dest, operation, 0, None, cancellation_token).await
    }

    #[instrument(skip(self, stats_tx, cancellation_token), err)]
    pub async fn transfer_with_stats(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        total_bytes: u64,
        stats_tx: Option<UnboundedSender<RcloneTransferStats>>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        // TODO: create an internal function that both transfer and transfer_with_stats can use
        // Disable json log for when not using stats
        ensure!(
            total_bytes > 0 || stats_tx.is_none(),
            "total_bytes must be provided if stats_tx is provided"
        );

        let mut args = vec![
            operation.as_str(),
            "--stats",
            "0.5s",
            "--stats-log-level",
            "NOTICE",
            "--fast-list",
            "--contimeout",
            CONNECTION_TIMEOUT,
            "--timeout",
            IO_IDLE_TIMEOUT,
            "--retries",
            "3",
            "--transfers",
            "8", // TODO: make configurable
        ];

        if !self.bandwidth_limit.is_empty() {
            args.extend_from_slice(&["--bwlimit", &self.bandwidth_limit]);
        }

        args.extend_from_slice(&[&source, &dest]);

        let mut child = self.command(&args).stderr(Stdio::piped()).spawn()?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;
        let mut lines = BufReader::new(stderr).lines();

        let transfer_future = async {
            if let Some(stats_tx) = stats_tx {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcloneStorage {
    client: RcloneClient,
    remote: String,
    root_dir: String,
}

impl RcloneStorage {
    #[instrument]
    pub fn new(
        rclone_path: PathBuf,
        config_path: Option<PathBuf>,
        remote: String,
        bandwidth_limit: String,
    ) -> Self {
        Self {
            client: RcloneClient::new(rclone_path, config_path, bandwidth_limit),
            remote,
            root_dir: "Quest Games/".to_string(), // TODO: make configurable
        }
    }

    fn format_remote_path(&self, path: &str) -> String {
        format!(
            "{}:{}",
            self.remote,
            PathBuf::from(self.root_dir.trim_end_matches(['/', '\\'])).join(path).display()
        )
    }

    #[instrument(skip(self, stats_tx, cancellation_token), err, ret)]
    pub async fn download_dir_with_stats(
        &self,
        source: String,
        dest: PathBuf,
        stats_tx: UnboundedSender<RcloneTransferStats>,
        cancellation_token: CancellationToken,
    ) -> Result<PathBuf> {
        ensure!(dest.parent().is_some(), "destination must have a parent directory");
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

    #[instrument(skip(self), err, ret)]
    pub async fn download_file(&self, source: String, dest: PathBuf) -> Result<PathBuf> {
        ensure!(dest.is_dir(), "destination must be a directory");
        let source = self.format_remote_path(&source);
        let mut dest_path = dest.clone();
        info!(source = %source, dest = %dest.display(), "Starting file download");
        self.client
            .transfer(
                source.clone(),
                dest.display().to_string(),
                RcloneTransferOperation::Copy,
                None,
            )
            .await?;
        dest_path
            .push(source.split(['/', '\\']).next_back().context("Failed to get source file name")?);
        Ok(dest_path)
    }

    #[instrument(skip(self), ret, err)]
    pub async fn remotes(&self) -> Result<Vec<String>> {
        // TODO: make configurable
        let regex = lazy_regex::regex!(r"^FFA-\d+$");
        let remotes = self.client.remotes().await?;
        Ok(remotes.into_iter().filter(|r| regex.is_match(r)).collect())
    }

    pub fn remote_name(&self) -> &str {
        &self.remote
    }
}
