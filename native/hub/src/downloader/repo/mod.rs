use std::path::Path;

use anyhow::Result;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use self::{ffa::FFARepo, newrepo::NewRepo};
use super::{AppDownloadProgress, TransferStats};
use crate::{
    downloader::config::{DownloaderConfig, RepoLayoutKind},
    models::{
        CloudApp, DownloadMode, Settings, signals::downloader::availability::RepoCapabilities,
    },
};

mod ffa;
mod newrepo;

#[derive(Debug, Clone)]
pub(super) struct RepoAppList {
    pub apps: Vec<CloudApp>,
    /// Package names that the repository excludes from donations.
    pub donation_blacklist: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RepoDownloadResult {
    pub skipped: bool,
}

/// A repository and the runtime state used by its operations.
#[derive(Debug, Clone)]
pub(super) enum Repo {
    Ffa(Box<FFARepo>),
    NewRepo(NewRepo),
}

pub(super) fn capabilities(layout: RepoLayoutKind) -> RepoCapabilities {
    match layout {
        RepoLayoutKind::Ffa => FFARepo::capabilities(),
        RepoLayoutKind::NewRepo => NewRepo::capabilities(),
    }
}

impl Repo {
    pub(super) async fn new(
        cfg: &DownloaderConfig,
        cache_dir: &Path,
        settings: &Settings,
        cancel: &CancellationToken,
    ) -> Result<(Self, Option<String>)> {
        match cfg.layout {
            RepoLayoutKind::Ffa => {
                let (repo, remote) = FFARepo::new(cfg, cache_dir, settings, cancel).await?;
                Ok((Self::Ffa(Box::new(repo)), remote))
            }
            RepoLayoutKind::NewRepo => Ok((Self::NewRepo(NewRepo::from_config(cfg)), None)),
        }
    }

    pub(super) fn remote(&self) -> Option<&str> {
        match self {
            Self::Ffa(repo) => Some(repo.remote()),
            Self::NewRepo(_) => None,
        }
    }

    pub(super) fn set_bandwidth_limit(&mut self, limit: String) {
        if let Self::Ffa(repo) = self {
            repo.set_bandwidth_limit(limit);
        }
    }

    pub(super) async fn select_remote(&mut self, requested: &str) -> Result<Option<String>> {
        match self {
            Self::Ffa(repo) => repo.select_remote(requested).await.map(Some),
            Self::NewRepo(_) => Ok(None),
        }
    }

    pub(super) async fn list_remotes(&self) -> Result<Vec<String>> {
        match self {
            Self::Ffa(repo) => repo.list_remotes().await,
            Self::NewRepo(repo) => repo.list_remotes().await,
        }
    }

    pub(super) async fn load_app_list(
        &self,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<RepoAppList> {
        match self {
            Self::Ffa(repo) => repo.load_app_list(cache_dir, cancellation_token).await,
            Self::NewRepo(repo) => {
                repo.load_app_list(cache_dir, http_client, cancellation_token).await
            }
        }
    }

    pub(super) async fn download_app(
        &self,
        app_full_name: &str,
        destination_dir: &Path,
        http_client: &reqwest::Client,
        download_mode: DownloadMode,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<RepoDownloadResult> {
        match self {
            Self::Ffa(repo) => {
                repo.download_app(app_full_name, destination_dir, progress_tx, cancellation_token)
                    .await
            }
            Self::NewRepo(repo) => {
                repo.download_app(
                    app_full_name,
                    destination_dir,
                    http_client,
                    download_mode,
                    progress_tx,
                    cancellation_token,
                )
                .await
            }
        }
    }

    pub(super) async fn upload_donation_archive(
        &self,
        archive_path: &Path,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        match self {
            Self::Ffa(repo) => {
                repo.upload_donation_archive(archive_path, stats_tx, cancellation_token).await
            }
            Self::NewRepo(repo) => {
                repo.upload_donation_archive(archive_path, stats_tx, cancellation_token).await
            }
        }
    }
}
