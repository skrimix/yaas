use std::{path::Path, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use derive_more::Debug;
use tokio_util::sync::CancellationToken;

use self::ffa::FFARepo;
use super::rclone::RcloneStorage;
use crate::{
    downloader::config::{DownloaderConfig, RepoLayoutKind},
    models::CloudApp,
};

mod ffa;

#[derive(Debug)]
pub(super) struct BuildStorageResult {
    pub storage: RcloneStorage,
    /// If Some, Downloader should persist this remote name into settings.
    pub persist_remote: Option<String>,
}

#[derive(Debug)]
pub(super) struct RepoAppList {
    pub apps: Vec<CloudApp>,
    /// Package names that repo doesn't want donations for.
    pub donation_blacklist: Vec<String>,
}

/// High-level operations a repository must implement.
#[async_trait]
pub(super) trait Repo: Send + Sync {
    fn id(&self) -> &'static str;

    async fn build_storage(&self, args: BuildStorageArgs<'_>) -> Result<BuildStorageResult>;

    async fn load_app_list(
        &self,
        storage: RcloneStorage,
        list_path: String,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<RepoAppList>;

    /// Source path under the root directory for download.
    fn source_for_download(&self, app_full_name: &str) -> String;

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
    }
}

/// Arguments for building storage, passed to repo implementations.
#[derive(Debug)]
pub(super) struct BuildStorageArgs<'a> {
    pub rclone_path: &'a Path,
    pub rclone_config_path: &'a Path,
    pub root_dir: &'a str,
    /// Remote selected by Downloader. Repo may keep or replace it.
    pub remote_name: &'a str,
    pub bandwidth_limit: &'a str,
    pub remote_name_filter_regex: Option<String>,
    /// Whether repo is allowed to pick a different remote automatically.
    pub allow_randomize_remote: bool,
}
