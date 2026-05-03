use std::{
    error::Error,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, ensure};
use lazy_regex::Regex;
use tokio::{fs, sync::mpsc::UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::{debug, instrument, warn};

use super::cli::{RcloneCli, RcloneTransferOperation};
use crate::downloader::TransferStats;

#[derive(Debug, Clone)]
pub(crate) struct RcloneStorage {
    client: RcloneCli,
    remote: String,
    root_dir: String,
    // Keep original string for equality, compile once for runtime use
    remote_filter_regex_str: Option<String>,
    remote_filter_regex: Option<Regex>,
}

impl RcloneStorage {
    #[instrument]
    pub(crate) fn new(
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
            client: RcloneCli::new(rclone_path, config_path, bandwidth_limit),
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
    #[instrument(level = "debug", skip(self, stats_tx, cancellation_token))]
    pub(crate) async fn upload_file_to_remote(
        &self,
        local_path: &Path,
        remote: &str,
        remote_dir: &str,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<()> {
        ensure!(local_path.is_file(), "Local path is not a file: {}", local_path.display());
        ensure!(!remote.is_empty(), "Remote name must not be empty");

        let file_name = local_path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            anyhow!("Local path has no valid UTF-8 file name: {}", local_path.display())
        })?;

        let dest = format_upload_destination(remote, remote_dir);
        debug!(
            src = %local_path.display(),
            dest = %dest,
            file_name,
            "Starting rclone upload"
        );

        let total_bytes = fs::metadata(local_path)
            .await
            .with_context(|| format!("Failed to get metadata for {}", local_path.display()))?
            .len();

        self.client
            .transfer_with_stats(
                local_path.display().to_string(),
                dest,
                RcloneTransferOperation::Copy,
                total_bytes,
                stats_tx,
                cancellation_token,
            )
            .await
    }

    #[instrument(level = "debug", skip(self, stats_tx, cancellation_token), ret)]
    pub(crate) async fn download_dir_with_stats(
        &self,
        source: String,
        dest: PathBuf,
        stats_tx: UnboundedSender<TransferStats>,
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

    #[instrument(level = "debug", skip(self, cancellation_token), ret)]
    pub(crate) async fn download_file(
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
    pub(crate) async fn remotes(&self) -> Result<Vec<String>> {
        let remotes = self.client.remotes().await?;
        Ok(filter_remotes_with_regex(remotes, self.remote_filter_regex.as_ref()))
    }
}

fn format_upload_destination(remote: &str, remote_dir: &str) -> String {
    let remote_dir = normalize_relative_remote_path(remote_dir);
    if remote_dir.is_empty() { format!("{remote}:") } else { format!("{remote}:{remote_dir}") }
}

fn normalize_relative_remote_path(path: &str) -> String {
    path.trim_matches(['/', '\\']).replace('\\', "/")
}

fn filter_remotes_with_regex(remotes: Vec<String>, regex: Option<&Regex>) -> Vec<String> {
    if let Some(re) = regex {
        remotes.into_iter().filter(|r| re.is_match(r)).collect()
    } else {
        remotes
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

    #[test]
    fn upload_destination_is_remote_directory_for_rclone_copy() {
        assert_eq!(format_upload_destination("FFA-DD", "_donations"), "FFA-DD:_donations");
        assert_eq!(format_upload_destination("FFA-DD", "/_donations/"), "FFA-DD:_donations");
    }

    #[test]
    fn upload_destination_supports_remote_root() {
        assert_eq!(format_upload_destination("FFA-DD", ""), "FFA-DD:");
        assert_eq!(format_upload_destination("FFA-DD", "/"), "FFA-DD:");
    }

    #[test]
    fn upload_destination_uses_remote_path_separators() {
        assert_eq!(
            format_upload_destination("FFA-DD", r"uploads\donations"),
            "FFA-DD:uploads/donations"
        );
    }
}
