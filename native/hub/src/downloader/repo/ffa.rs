use std::{collections::HashSet, error::Error, path::Path};

use anyhow::{Context, Result, anyhow, bail, ensure};
use derive_more::Debug;
use futures::StreamExt as _;
use rand::seq::IndexedRandom;
use tokio::{
    fs::{self, File},
    sync::mpsc::UnboundedSender,
};
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, instrument, warn};

use super::{RepoAppList, RepoCapabilities, RepoDownloadResult};
use crate::{
    downloader::{
        AppDownloadProgress, TransferStats,
        config::DownloaderConfig,
        rclone::{self, RcloneStorage},
    },
    models::{CloudApp, Settings},
};

/// FFA layout – direct files and list under a configurable remote/root.
#[derive(Debug, Clone)]
pub(in crate::downloader) struct FFARepo {
    config: DownloaderConfig,
    storage: RcloneStorage,
}

impl FFARepo {
    pub(super) async fn new(
        cfg: &DownloaderConfig,
        cache_dir: &Path,
        settings: &Settings,
        cancel: &CancellationToken,
    ) -> Result<(Self, Option<String>)> {
        let (rclone_path, rclone_config_path) =
            rclone::prepare_rclone_files(cache_dir, cfg, cancel).await?;
        let remote = cancel
            .run_until_cancelled(pick_remote_name(
                &rclone_path,
                &rclone_config_path,
                cfg.remote_name_filter_regex.as_deref(),
                &settings.rclone_remote_name,
                !cfg.disable_randomize_remote,
            ))
            .await
            .context("Downloader initialization cancelled")??;
        let persist = (remote != settings.rclone_remote_name).then(|| remote.clone());
        let storage = RcloneStorage::new(
            rclone_path,
            rclone_config_path,
            cfg.root_dir.clone(),
            remote,
            settings.bandwidth_limit.clone(),
            cfg.remote_name_filter_regex.clone(),
        );
        Ok((Self { config: cfg.clone(), storage }, persist))
    }

    pub(super) fn remote(&self) -> &str {
        self.storage.remote()
    }

    pub(super) fn set_bandwidth_limit(&mut self, limit: String) {
        self.storage.set_bandwidth_limit(limit);
    }

    pub(super) async fn select_remote(&mut self, requested: &str) -> Result<String> {
        let remotes = self.storage.remotes().await?;
        let remote = if remotes.iter().any(|r| r == requested) {
            requested.to_string()
        } else {
            remotes.first().context("Remote list is empty")?.clone()
        };
        self.storage.set_remote(remote.clone());
        Ok(remote)
    }

    fn id(&self) -> &'static str {
        "ffa"
    }

    pub(super) fn capabilities() -> RepoCapabilities {
        RepoCapabilities {
            supports_remote_selection: true,
            supports_bandwidth_limit: true,
            supports_download_mode_selection: false,
            supports_donation_upload: true,
        }
    }

    pub(super) async fn list_remotes(&self) -> Result<Vec<String>> {
        self.storage.remotes().await
    }

    #[instrument(level = "debug", name = "repo.load_app_list", skip(self, cancellation_token), fields(layout = %self.id()))]
    pub(super) async fn load_app_list(
        &self,
        cache_dir: &Path,
        cancellation_token: CancellationToken,
    ) -> Result<RepoAppList> {
        let storage = &self.storage;
        let blacklist = async {
            if let Some(path) =
                self.config.donation_blacklist_path.as_deref().filter(|p| !p.is_empty())
            {
                load_blacklist_from_remote(storage, path, cache_dir, cancellation_token.clone())
                    .await
            } else {
                Ok(HashSet::new())
            }
        };
        let apps = async {
            let path = storage
                .download_file(
                    self.config.list_path.clone(),
                    cache_dir.to_path_buf(),
                    Some(cancellation_token.clone()),
                )
                .await
                .context("Failed to download app list file")?;

            debug!(path = %path.display(), "App list file downloaded, parsing...");
            let file = File::open(&path).await.context("Could not open app list file")?;
            let mut reader =
                csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
            let records = reader.deserialize::<CloudApp>();
            let cloud_apps: Vec<CloudApp> = records
                .enumerate()
                .filter_map(|(idx, result)| async move {
                    match result {
                        Ok(app) => Some(app),
                        Err(e) => {
                            warn!(
                                line = idx + 1,
                                error = &e as &dyn Error,
                                "Skipping malformed line in app list"
                            );
                            None
                        }
                    }
                })
                .collect()
                .await;
            Ok::<_, anyhow::Error>(cloud_apps)
        };
        let (apps, blacklist) = tokio::join!(apps, blacklist);
        let cloud_apps = apps?;
        let donation_blacklist = blacklist.unwrap_or_default().into_iter().collect();

        Span::current().record("count", cloud_apps.len());
        Ok(RepoAppList { apps: cloud_apps, donation_blacklist })
    }

    pub(super) async fn download_app(
        &self,
        app_full_name: &str,
        destination_dir: &Path,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<RepoDownloadResult> {
        let storage = &self.storage;
        let _ = progress_tx.send(AppDownloadProgress::Status("Downloading files...".to_string()));
        let (stats_tx, mut stats_rx) = tokio::sync::mpsc::unbounded_channel::<TransferStats>();
        let forward_progress = tokio::spawn(async move {
            while let Some(stats) = stats_rx.recv().await {
                let _ = progress_tx.send(AppDownloadProgress::Transfer(stats));
            }
        });
        storage
            .download_dir_with_stats(
                app_full_name.to_string(),
                destination_dir.to_path_buf(),
                stats_tx,
                cancellation_token,
            )
            .await?;
        let _ = forward_progress.await;
        Ok(RepoDownloadResult { skipped: false })
    }

    pub(super) async fn upload_donation_archive(
        &self,
        archive_path: &Path,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let storage = &self.storage;
        let remote =
            self.config.donation_remote_name.as_deref().filter(|s| !s.is_empty()).ok_or_else(
                || anyhow!("App donation remote is not configured in downloader.json"),
            )?;
        let remote_path =
            self.config.donation_remote_path.as_deref().filter(|s| !s.is_empty()).ok_or_else(
                || anyhow!("App donation remote path is not configured in downloader.json"),
            )?;

        ensure!(
            archive_path.is_file(),
            "Donation archive does not exist or is not a file: {}",
            archive_path.display()
        );

        storage
            .upload_file_to_remote(
                archive_path,
                remote,
                remote_path,
                stats_tx,
                Some(cancellation_token),
            )
            .await
            .context("Failed to upload donation archive")
    }
}

#[instrument(level = "debug", err)]
async fn pick_remote_name(
    rclone_path: &Path,
    rclone_config_path: &Path,
    remote_filter_regex: Option<&str>,
    current_remote: &str,
    allow_randomize: bool,
) -> Result<String> {
    let remotes =
        rclone::list_remotes(rclone_path, rclone_config_path, remote_filter_regex).await?;

    if remotes.is_empty() {
        bail!("Remote list is empty");
    }

    let mut chosen = current_remote.to_string();
    if allow_randomize {
        let mut rng = rand::rng();
        if let Some(choice) = remotes.choose(&mut rng) {
            chosen = choice.clone();
        }
    } else if remotes.iter().all(|r| r != current_remote) {
        chosen = remotes.first().cloned().unwrap_or_else(|| current_remote.to_string());
    }

    Ok(chosen)
}

#[instrument(
    level = "debug",
    name = "load_blacklist_from_remote",
    skip(storage),
    fields(path = %remote_path, cache_dir = %cache_dir.display())
)]
async fn load_blacklist_from_remote(
    storage: &RcloneStorage,
    remote_path: &str,
    cache_dir: &Path,
    cancellation_token: CancellationToken,
) -> Result<HashSet<String>> {
    match storage
        .download_file(remote_path.to_string(), cache_dir.to_path_buf(), Some(cancellation_token))
        .await
    {
        Ok(path) => load_blacklist_from_path(&path).await,
        Err(e) => {
            warn!(
                error = e.as_ref() as &dyn Error,
                path = remote_path,
                "Failed to download donation blacklist, continuing without it"
            );
            Ok(HashSet::new())
        }
    }
}

#[instrument(
    level = "debug",
    name = "load_blacklist_from_path",
    fields(path = %path.display())
)]
async fn load_blacklist_from_path(path: &Path) -> Result<HashSet<String>> {
    if !path.exists() {
        warn!(path = %path.display(), "Donation blacklist file does not exist");
        return Ok(HashSet::new());
    }

    match fs::read_to_string(path).await {
        Ok(text) => Ok(parse_blacklist(&text)),
        Err(e) => {
            warn!(
                error = &e as &dyn Error,
                path = %path.display(),
                "Failed to read donation blacklist file, continuing without it"
            );
            Ok(HashSet::new())
        }
    }
}

fn parse_blacklist(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}
