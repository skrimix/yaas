use std::{collections::HashSet, error::Error, path::Path};

use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use derive_more::Debug;
use futures::StreamExt as _;
use rand::seq::IndexedRandom;
use tokio::{
    fs::{self, File},
    sync::mpsc::UnboundedSender,
};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, instrument, warn};

use super::{
    BuildStorageArgs, BuildStorageResult, Repo, RepoAppList, RepoCapabilities, RepoDownloadResult,
    RepoStorage,
};
use crate::{
    downloader::{
        AppDownloadProgress, TransferStats,
        config::DownloaderConfig,
        rclone::{self, RcloneStorage},
    },
    models::CloudApp,
};

/// FFA layout – direct files and list under a configurable remote/root.
#[derive(Debug, Clone, Default)]
pub(super) struct FFARepo {
    donation_blacklist_path: Option<String>,
}

impl FFARepo {
    pub(super) fn from_config(cfg: &DownloaderConfig) -> Self {
        Self { donation_blacklist_path: cfg.donation_blacklist_path.clone() }
    }
}

#[async_trait]
impl Repo for FFARepo {
    fn id(&self) -> &'static str {
        "ffa"
    }

    fn capabilities(&self) -> RepoCapabilities {
        RepoCapabilities {
            supports_remote_selection: true,
            supports_bandwidth_limit: true,
            supports_donation_upload: true,
        }
    }

    #[instrument(level = "debug", name = "repo.build_storage", fields(layout = %self.id()))]
    async fn build_storage(&self, args: BuildStorageArgs<'_>) -> Result<BuildStorageResult> {
        debug!("Using repository layout: FFA");

        let rclone_path = args
            .rclone_path
            .ok_or_else(|| anyhow!("Missing rclone path for ffa repository layout"))?;
        let rclone_config_path = args
            .rclone_config_path
            .ok_or_else(|| anyhow!("Missing rclone config path for ffa repository layout"))?;

        let remote_name = pick_remote_name(
            rclone_path,
            rclone_config_path,
            args.remote_name_filter_regex.as_deref(),
            args.remote_name,
            args.allow_randomize_remote,
        )
        .await?;
        let persist_remote = (remote_name != args.remote_name).then(|| remote_name.clone());

        let storage = RcloneStorage::new(
            rclone_path.to_path_buf(),
            rclone_config_path.to_path_buf(),
            args.root_dir.to_string(),
            remote_name,
            args.bandwidth_limit.to_string(),
            args.remote_name_filter_regex.clone(),
        );
        Ok(BuildStorageResult { storage: RepoStorage::Ffa(storage), persist_remote })
    }

    async fn list_remotes(&self, storage: RepoStorage) -> Result<Vec<String>> {
        match storage {
            RepoStorage::Ffa(storage) => storage.remotes().await,
            RepoStorage::NewRepo(_) => unreachable!("new-repo storage passed to ffa repo"),
        }
    }

    #[instrument(level = "debug", name = "repo.load_app_list", skip(storage, _http_client, cancellation_token), fields(layout = %self.id()))]
    async fn load_app_list(
        &self,
        storage: RepoStorage,
        list_path: String,
        cache_dir: &Path,
        _http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<RepoAppList> {
        let RepoStorage::Ffa(storage) = storage else {
            unreachable!("new-repo storage passed to ffa repo");
        };
        let blacklist_handle = if let Some(blacklist_path) =
            self.donation_blacklist_path.as_deref().filter(|p| !p.is_empty())
        {
            let storage_clone = storage.clone();
            let cache_dir = cache_dir.to_path_buf();
            let path = blacklist_path.to_string();
            Some(tokio::spawn(
                async move { load_blacklist_from_remote(&storage_clone, &path, &cache_dir).await }
                    .instrument(Span::current()),
            ))
        } else {
            None
        };

        let path = storage
            .download_file(list_path, cache_dir.to_path_buf(), Some(cancellation_token))
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
        let mut donation_blacklist = Vec::new();
        if let Some(handle) = blacklist_handle {
            match handle.await {
                Ok(Ok(blacklist)) => {
                    donation_blacklist = blacklist.into_iter().collect();
                }
                Ok(Err(e)) => {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Failed to load donation blacklist in FFA repo, continuing without it"
                    );
                }
                Err(e) => {
                    warn!(
                        error = &e as &dyn Error,
                        "Blacklist task join error in FFA repo, continuing without blacklist"
                    );
                }
            }
        }

        Span::current().record("count", cloud_apps.len());
        Ok(RepoAppList { apps: cloud_apps, donation_blacklist })
    }

    async fn download_app(
        &self,
        storage: RepoStorage,
        app_full_name: &str,
        destination_dir: &Path,
        _cache_dir: &Path,
        _http_client: &reqwest::Client,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<RepoDownloadResult> {
        let RepoStorage::Ffa(storage) = storage else {
            unreachable!("new-repo storage passed to ffa repo");
        };
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

    async fn upload_donation_archive(
        &self,
        storage: RepoStorage,
        config: &DownloaderConfig,
        archive_path: &Path,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let RepoStorage::Ffa(storage) = storage else {
            unreachable!("new-repo storage passed to ffa repo");
        };
        let remote =
            config.donation_remote_name.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
                anyhow!("App donation remote is not configured in downloader.json")
            })?;
        let remote_path =
            config.donation_remote_path.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
                anyhow!("App donation remote path is not configured in downloader.json")
            })?;

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
) -> Result<HashSet<String>> {
    match storage.download_file(remote_path.to_string(), cache_dir.to_path_buf(), None).await {
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
