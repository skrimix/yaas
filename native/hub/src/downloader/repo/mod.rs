use std::{path::Path, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use derive_more::Debug;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use self::{ffa::FFARepo, new_repo::NewRepo};
use super::{AppDownloadProgress, TransferStats, rclone::RcloneStorage};
use crate::{
    downloader::config::{DownloaderConfig, RepoLayoutKind},
    models::CloudApp,
};

mod ffa;
mod new_repo;

#[derive(Debug)]
pub(super) struct BuildStorageResult {
    pub storage: RepoStorage,
    /// If Some, Downloader should persist this remote name into settings.
    pub persist_remote: Option<String>,
}

#[derive(Debug)]
pub(super) struct RepoAppList {
    pub apps: Vec<CloudApp>,
    /// Package names that repo doesn't want donations for.
    pub donation_blacklist: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RepoCapabilities {
    pub supports_remote_selection: bool,
    pub supports_bandwidth_limit: bool,
    pub supports_donation_upload: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RepoDownloadResult {
    pub skipped: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum RepoStorage {
    Ffa(RcloneStorage),
    NewRepo(new_repo::NewRepoStorage),
}

/// High-level operations a repository must implement.
#[async_trait]
pub(super) trait Repo: Send + Sync {
    fn id(&self) -> &'static str;

    fn capabilities(&self) -> RepoCapabilities;

    async fn build_storage(&self, args: BuildStorageArgs<'_>) -> Result<BuildStorageResult>;

    async fn list_remotes(&self, storage: RepoStorage) -> Result<Vec<String>>;

    async fn load_app_list(
        &self,
        storage: RepoStorage,
        list_path: String,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<RepoAppList>;

    #[allow(clippy::too_many_arguments)]
    async fn download_app(
        &self,
        storage: RepoStorage,
        app_full_name: &str,
        destination_dir: &Path,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<RepoDownloadResult>;

    async fn upload_donation_archive(
        &self,
        storage: RepoStorage,
        config: &DownloaderConfig,
        archive_path: &Path,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: CancellationToken,
    ) -> Result<()>;

    /// If the repo generates its own rclone config at runtime, return the
    /// suggested filename to be used. Otherwise None.
    fn generated_config_filename(&self) -> Option<&'static str> {
        None
    }
}

/// Factory: choose a concrete repo based on config.
pub(super) fn make_repo_from_config(cfg: &DownloaderConfig) -> Arc<dyn Repo> {
    match cfg.layout {
        RepoLayoutKind::Ffa => Arc::new(FFARepo::from_config(cfg)),
        RepoLayoutKind::NewRepo => Arc::new(NewRepo::from_config(cfg)),
    }
}

/// Arguments for building storage, passed to repo implementations.
#[derive(Debug)]
pub(super) struct BuildStorageArgs<'a> {
    pub rclone_path: Option<&'a Path>,
    pub rclone_config_path: Option<&'a Path>,
    pub root_dir: &'a str,
    /// Remote selected by Downloader. Repo may keep or replace it.
    pub remote_name: &'a str,
    pub bandwidth_limit: &'a str,
    pub remote_name_filter_regex: Option<String>,
    /// Whether repo is allowed to pick a different remote automatically.
    pub allow_randomize_remote: bool,
}
