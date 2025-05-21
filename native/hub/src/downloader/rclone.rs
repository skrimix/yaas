use std::{path::PathBuf, process::Stdio};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};
use tracing::{debug, error, trace};

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
    pub eta: u64,
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
    pub fn new(
        rclone_path: PathBuf,
        config_path: Option<PathBuf>,
        bandwidth_limit: String,
    ) -> Self {
        Self { rclone_path, config_path, sys_proxy: get_sys_proxy(), bandwidth_limit }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.rclone_path);
        command.kill_on_drop(true);

        if let Some(proxy) = &self.sys_proxy {
            trace!("Using system proxy: {}", proxy);
            command.env("http_proxy", proxy);
            command.env("https_proxy", proxy);
        }

        if let Some(config_path) = &self.config_path {
            command.arg("--config").arg(config_path);
        }
        command.arg("--use-json-log");

        command.args(args);
        command
    }

    async fn run_to_string(&self, args: &[&str]) -> Result<String> {
        let output = self.command(args).output().await.context("rclone command failed")?;
        // TODO: handle expected non-zero exit codes
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            Err(anyhow::anyhow!(
                "rclone returned exit code {}, stderr:\n{}",
                output.status.code().map_or("unknown".to_string(), |c| c.to_string()),
                error
            ))
        }
    }

    // pub async fn list_remotes(&self) -> Result<Vec<String>> {
    //     let remotes = self
    //         .run_to_string(&["listremotes"])
    //         .await
    //         .context("failed to list remotes")?
    //         .lines()
    //         .map(|line| line.trim().to_string())
    //         .collect();
    //     Ok(remotes)
    // }

    pub async fn size(&self, path: &str) -> Result<RcloneSizeOutput> {
        let output = self.run_to_string(&["size", "--fast-list", "--json", path]).await?;
        serde_json::from_str(&output).context("Failed to parse rclone size output")
    }

    pub async fn transfer(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        total_bytes: u64,
    ) -> Result<()> {
        // FIXME: disallow concurrent transfers to the same destination
        self.transfer_with_stats(source, dest, operation, total_bytes, None).await
    }

    pub async fn transfer_with_stats(
        &self,
        source: String,
        dest: String,
        operation: RcloneTransferOperation,
        total_bytes: u64,
        stats_tx: Option<UnboundedSender<RcloneTransferStats>>,
    ) -> Result<()> {
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
        let mut log_messages = Vec::new();

        if let Some(stats_tx) = stats_tx {
            println!("starting stats loop");
            while let Some(line) = lines.next_line().await? {
                match serde_json::from_str::<RcloneStatLine>(&line) {
                    Ok(stat_line) => {
                        trace!(?stat_line, "parsed rclone stat line");
                        let mut stats = stat_line.stats;
                        stats.total_bytes = total_bytes;
                        trace!(?stats, "sending stats update");
                        if stats_tx.send(stats).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        debug!("error parsing rclone stat line: {}", e);
                    }
                }
                log_messages.push(line);
            }
        }

        let status = child.wait().await?;
        match status.success() {
            true => Ok(()),
            false => {
                let stderr = log_messages.join("\n");
                error!(code = status.code().unwrap_or(-1), stderr, "rclone transfer failed");
                Err(anyhow!(
                    "rclone failed with exit code: {}",
                    status.code().map_or("unknown".to_string(), |c| c.to_string())
                ))
            }
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
    pub fn new(
        rclone_path: PathBuf,
        config_path: Option<PathBuf>,
        remote: String,
        bandwidth_limit: String,
    ) -> Self {
        Self {
            client: RcloneClient::new(rclone_path, config_path, bandwidth_limit),
            remote,
            root_dir: "Quest Games/".to_string(),
        }
    }

    fn format_remote_path(&self, path: &str) -> String {
        format!(
            "{}:{}",
            self.remote,
            PathBuf::from(self.root_dir.trim_end_matches('/')).join(path).display()
        )
    }

    pub async fn download_dir_with_stats(
        &self,
        source: String,
        dest: PathBuf,
        stats_tx: UnboundedSender<RcloneTransferStats>,
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
            )
            .await
            .map(|_| dest)
    }

    pub async fn download_file(&self, source: String, dest: PathBuf) -> Result<PathBuf> {
        ensure!(dest.is_dir(), "destination must be a directory");
        let source = self.format_remote_path(&source);
        let total_bytes =
            self.client.size(&source).await.context("Failed to get remote file size")?.bytes;
        let mut dest = dest;
        self.client
            .transfer(
                source.clone(),
                dest.display().to_string(),
                RcloneTransferOperation::Copy,
                total_bytes,
            )
            .await?;
        dest.push(source.split('/').next_back().context("Failed to get source file name")?);
        Ok(dest)
    }
}
