use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use rand::seq::IndexedRandom;
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock, mpsc::UnboundedSender};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, instrument, warn};

use crate::{
    downloader::{
        config::{DownloaderConfig, RepoLayoutKind},
        rclone::RcloneStorage,
    },
    models::{
        CloudApp, Settings,
        signals::{
            cloud_apps::{
                details::{AppDetailsResponse, GetAppDetailsRequest},
                list::{CloudAppsChangedEvent, LoadCloudAppsRequest},
                reviews::{AppReviewsResponse, GetAppReviewsRequest},
            },
            downloads_local::DownloadsChanged,
            storage::remotes::{GetRcloneRemotesRequest, RcloneRemotesChanged},
        },
    },
    settings::SettingsHandler,
};

mod rclone;
pub(crate) use rclone::RcloneTransferStats;
pub(crate) mod artifacts;
mod cloud_api;
pub(crate) mod config;
mod http_cache;
pub(crate) mod metadata;
mod repo;

pub(crate) struct Downloader {
    config: Arc<DownloaderConfig>,
    cache_dir: PathBuf,
    rclone_path: PathBuf,
    rclone_config_path: PathBuf,
    root_dir: String,
    list_path: String,
    cloud_apps: Mutex<Vec<CloudApp>>,
    storage: RwLock<RcloneStorage>,
    download_dir: RwLock<PathBuf>,
    current_load_token: RwLock<CancellationToken>,
    write_legacy_release_json: RwLock<bool>,
    cancel_token: CancellationToken,
    http_client: reqwest::Client,
    repo: Arc<dyn repo::Repo>,
}

impl Downloader {
    #[instrument(level = "debug", skip(settings_stream))]
    pub(crate) async fn new(
        config: Arc<DownloaderConfig>,
        cache_dir: PathBuf,
        rclone_path: PathBuf,
        rclone_config_path: PathBuf,
        settings_handler: Arc<SettingsHandler>,
        mut settings_stream: WatchStream<Settings>,
    ) -> Result<Arc<Self>> {
        let settings =
            settings_stream.next().await.expect("Settings stream closed on downloader init");

        let repo = repo::make_repo_from_config(&config);

        let http_client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let share_remote_configured =
            config.share_remote_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
                || config.share_remote_path.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
        let blacklist_path_configured =
            config.share_blacklist_path.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
        if share_remote_configured && !blacklist_path_configured {
            warn!(
                "App sharing remote is configured but `share_blacklist_path` is missing; sharing \
                 blacklist will be disabled"
            );
        }

        let mut remote_for_init = settings.rclone_remote_name.clone();
        if matches!(config.layout, RepoLayoutKind::Ffa) {
            remote_for_init = Self::pick_remote_name(
                &rclone_path,
                &rclone_config_path,
                config.remote_name_filter_regex.as_deref(),
                &remote_for_init,
                !config.disable_randomize_remote,
            )
            .await?;

            if remote_for_init != settings.rclone_remote_name {
                debug!(
                    old = settings.rclone_remote_name,
                    new = remote_for_init,
                    "Remote name changed on init, persisting to settings"
                );
                let mut updated = settings.clone();
                updated.rclone_remote_name = remote_for_init.clone();
                let _ = settings_handler.save_settings(&updated);
            }
        }

        let built = repo
            .build_storage(repo::BuildStorageArgs {
                rclone_path: &rclone_path,
                rclone_config_path: &rclone_config_path,
                root_dir: &config.root_dir,
                remote_name: &remote_for_init,
                bandwidth_limit: &settings.bandwidth_limit,
                remote_name_filter_regex: config.remote_name_filter_regex.clone(),
                http_client: &http_client,
                cache_dir: &cache_dir,
            })
            .await?;
        // If the repo asked us to persist a remote, update settings
        if let Some(remote) = &built.persist_remote
            && *remote != settings.rclone_remote_name
        {
            debug!(
                old = settings.rclone_remote_name,
                new = remote,
                "Remote name changed on repo request, persisting to settings"
            );
            let mut updated = settings.clone();
            updated.rclone_remote_name = remote.clone();
            let _ = settings_handler.save_settings(&updated);
        }
        let storage = built.storage;

        let root_dir = config.root_dir.clone();
        let list_path = config.list_path.clone();

        let handle = Arc::new(Self {
            config,
            cache_dir,
            rclone_path,
            rclone_config_path,
            root_dir,
            list_path,
            cloud_apps: Mutex::new(Vec::new()),
            storage: RwLock::new(storage),
            download_dir: RwLock::new(PathBuf::from(settings.downloads_location.clone())),
            current_load_token: RwLock::new(CancellationToken::new()),
            write_legacy_release_json: RwLock::new(settings.write_legacy_release_json),
            cancel_token: CancellationToken::new(),
            http_client,
            repo,
        });

        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_commands().await;
            }
        });

        tokio::spawn({
            let handle = handle.clone();
            async move {
                debug!("Downloader starting to listen for settings changes");
                loop {
                    tokio::select! {
                        _ = handle.cancel_token.cancelled() => {
                            debug!("Downloader settings listener cancelled, exiting");
                            return;
                        }
                        maybe_settings = settings_stream.next() => {
                            let Some(settings) = maybe_settings else {
                                panic!("Settings stream closed for Downloader");
                            };
                            debug!("Downloader received settings update");
                            debug!(?settings, "New settings");

                            // Rebuild storage on settings changes, do not randomize the remote
                            let mut chosen_remote = settings.rclone_remote_name.clone();
                            if matches!(handle.config.layout, RepoLayoutKind::Ffa) {
                                match Self::pick_remote_name(
                                    &handle.rclone_path,
                                    &handle.rclone_config_path,
                                    handle.config.remote_name_filter_regex.as_deref(),
                                    &chosen_remote,
                                    false,
                                )
                                .await
                                {
                                    Ok(resolved) => {
                                        if resolved != chosen_remote {
                                            chosen_remote = resolved.clone();
                                            let mut updated = settings.clone();
                                            updated.rclone_remote_name = resolved;
                                            let _ = settings_handler.save_settings(&updated);
                                        }
                                    }
                                    Err(e) => {
                                        error!(
                                            error = e.as_ref() as &dyn Error,
                                            "Remote list is empty after settings change"
                                        );
                                    }
                                }
                            }

                            let built = handle
                                .repo
                                .build_storage(repo::BuildStorageArgs {
                                    rclone_path: &handle.rclone_path,
                                    rclone_config_path: &handle.rclone_config_path,
                                    root_dir: &handle.root_dir,
                                    remote_name: &chosen_remote,
                                    bandwidth_limit: &settings.bandwidth_limit,
                                    remote_name_filter_regex: handle.config.remote_name_filter_regex.clone(),
                                    http_client: &handle.http_client,
                                    cache_dir: &handle.cache_dir,
                                })
                                .await;

                            let new_storage = match built {
                                Ok(res) => {
                                    if let Some(remote) = res.persist_remote
                                        && remote != settings.rclone_remote_name {
                                            let mut updated = settings.clone();
                                            updated.rclone_remote_name = remote.clone();
                                            let _ = settings_handler.save_settings(&updated);
                                        }
                                    res.storage
                                }
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Failed to rebuild storage on settings change");
                                    handle.storage.read().await.clone()
                                }
                            };

                            if new_storage != *handle.storage.read().await {
                                info!("Rclone storage config changed, recreating and refreshing app list");
                                handle.current_load_token.read().await.cancel();
                                let new_token = CancellationToken::new();
                                *handle.current_load_token.write().await = new_token.clone();

                                *handle.storage.write().await = new_storage;

                                match handle.storage.read().await.remotes().await {
                                    Ok(remotes) => {
                                        RcloneRemotesChanged { remotes, error: None }.send_signal_to_dart();
                                    }
                                    Err(e) => {
                                        error!(error = e.as_ref() as &dyn Error, "Failed to get rclone remotes after reload");
                                        RcloneRemotesChanged {
                                            remotes: Vec::new(),
                                            error: Some(format!("Failed to get rclone remotes: {:#}", e)),
                                        }
                                        .send_signal_to_dart();
                                    }
                                }

                                // Refresh app list
                                handle.load_app_list(true, new_token).await;
                            }

                            let mut download_dir = handle.download_dir.write().await;
                            let new_download_dir = PathBuf::from(settings.downloads_location);
                            if *download_dir != new_download_dir {
                                debug!(new_dir = %new_download_dir.display(), "Download directory changed");
                                *download_dir = new_download_dir;
                            }

                            // Update legacy release.json toggle
                            let mut legacy_flag = handle.write_legacy_release_json.write().await;
                            *legacy_flag = settings.write_legacy_release_json;
                        }
                    }
                }
            }
        }.instrument(info_span!("task_handle_settings_updates")),
        );

        // On init, send rclone remotes list
        tokio::spawn({
            let handle = handle.clone();
            async move {
                tokio::select! {
                    _ = handle.cancel_token.cancelled() => {
                        debug!("Downloader cancelled before sending initial remotes");
                    }
                    res = async {
                        let storage = handle.storage.read().await.clone();
                        storage.remotes().await
                    } => {
                        match res {
                            Ok(remotes) => {
                                RcloneRemotesChanged { remotes, error: None }.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(
                                    error = e.as_ref() as &dyn Error,
                                    "Failed to get rclone remotes on init"
                                );
                                RcloneRemotesChanged {
                                    remotes: Vec::new(),
                                    error: Some(format!("Failed to get rclone remotes: {:#}", e)),
                                }
                                .send_signal_to_dart();
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
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

    /// Returns the cached CloudApp (if any) that matches the given full name
    #[instrument(level = "debug", skip(self))]
    async fn get_app_by_full_name(&self, full_name: &str) -> Option<CloudApp> {
        let cache = self.cloud_apps.lock().await;
        cache.iter().find(|a| a.full_name == full_name).cloned()
    }

    /// Upload a prepared archive (and its MD5 sidecar) used for app sharing.
    ///
    /// This uses optional `share_remote_name` and `share_remote_path` from DownloaderConfig.
    /// If either is missing or empty, the call fails with a configuration error.
    #[instrument(skip(self, cancellation_token), err)]
    pub(crate) async fn upload_shared_archive(
        &self,
        archive_path: &Path,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let remote =
            self.config.share_remote_name.as_deref().filter(|s| !s.is_empty()).ok_or_else(
                || anyhow!("App sharing remote is not configured in downloader.json"),
            )?;
        let remote_path =
            self.config.share_remote_path.as_deref().filter(|s| !s.is_empty()).ok_or_else(
                || anyhow!("App sharing remote path is not configured in downloader.json"),
            )?;

        ensure!(
            archive_path.is_file(),
            "Shared archive does not exist or is not a file: {}",
            archive_path.display()
        );

        let storage = self.storage.read().await.clone();

        storage
            .upload_file_to_remote(
                archive_path,
                remote,
                remote_path,
                Some(cancellation_token.clone()),
            )
            .await
            .context("Failed to upload shared archive")?;

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn receive_commands(&self) {
        let load_cloud_apps_receiver = LoadCloudAppsRequest::get_dart_signal_receiver();
        let get_rclone_remotes_receiver = GetRcloneRemotesRequest::get_dart_signal_receiver();
        let get_app_details_receiver = GetAppDetailsRequest::get_dart_signal_receiver();
        let get_app_reviews_receiver = GetAppReviewsRequest::get_dart_signal_receiver();
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    info!("Downloader command loop cancelled, exiting");
                    return;
                }
                request = load_cloud_apps_receiver.recv() => {
                    if let Some(request) = request {
                        debug!(refresh = request.message.refresh, "Received LoadCloudAppsRequest");
                        let new_token = CancellationToken::new();
                        {
                            let mut guard = self.current_load_token.write().await;
                            guard.cancel();
                            *guard = new_token.clone();
                        }
                        self.load_app_list(request.message.refresh, new_token).await;
                    } else {
                        info!("LoadCloudAppsRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
                request = get_rclone_remotes_receiver.recv() => {
                    if request.is_some() {
                        debug!("Received GetRcloneRemotesRequest");
                        let remotes = self.storage.read().await.remotes().await;
                        match remotes {
                            Ok(remotes) => {
                                RcloneRemotesChanged { remotes, error: None }.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(error = e.as_ref() as &dyn Error, "Failed to get rclone remotes");
                                RcloneRemotesChanged { remotes: Vec::new(), error: Some(format!("Failed to get rclone remotes: {:#}", e)) }.send_signal_to_dart();
                            }
                        }
                    } else {
                        info!("GetRcloneRemotesRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
                request = get_app_details_receiver.recv() => {
                    if let Some(request) = request {
                        let package_name = request.message.package_name;
                        debug!(%package_name, "Received GetAppDetailsRequest");
                        let client = self.http_client.clone();
                        tokio::spawn(async move {
                            match cloud_api::fetch_app_details(&client, package_name.clone()).await {
                                Ok(Some(api)) => {
                                    let crate::models::AppApiResponse {
                                        id,
                                        display_name,
                                        description,
                                        quality_rating_aggregate,
                                        rating_count,
                                    } = api;
                                    AppDetailsResponse {
                                        package_name,
                                        app_id: id,
                                        display_name,
                                        description,
                                        rating_average: quality_rating_aggregate,
                                        rating_count,
                                        not_found: false,
                                        error: None,
                                    }.send_signal_to_dart();
                                }
                                Ok(None) => {
                                    AppDetailsResponse::default_not_found(package_name).send_signal_to_dart();
                                }
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Failed to fetch app details");
                                    AppDetailsResponse::default_error(package_name, format!("Failed to fetch app details: {:#}", e)).send_signal_to_dart();
                                }
                            }
                        });
                    } else {
                        info!("GetAppDetailsRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
                request = get_app_reviews_receiver.recv() => {
                    if let Some(request) = request {
                        let app_id = request.message.app_id;
                        let limit = request.message.limit.unwrap_or(5);
                        let offset = request.message.offset.unwrap_or(0);
                        let sort_by = request.message.sort_by.unwrap_or_else(|| "helpful".to_string());
                        debug!(%app_id, "Received GetAppReviewsRequest");
                        let client = self.http_client.clone();
                        tokio::spawn(async move {
                            match cloud_api::fetch_app_reviews(&client, &app_id, limit, offset, &sort_by).await {
                                Ok(reviews) => {
                                    AppReviewsResponse { app_id, total: Some(reviews.total), reviews: reviews.reviews, error: None }.send_signal_to_dart();
                                }
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Failed to fetch app reviews");
                                    AppReviewsResponse { app_id, total: None, reviews: Vec::new(), error: Some(format!("Failed to fetch reviews: {:#}", e)) }.send_signal_to_dart();
                                }
                            }
                        });
                    } else {
                        info!("GetAppReviewsRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    pub(crate) async fn stop(&self) {
        info!("Stopping downloader instance");
        self.cancel_token.cancel();
        self.current_load_token.read().await.cancel();
    }

    #[instrument(level = "debug", skip(self, cancellation_token))]
    async fn load_app_list(&self, force_refresh: bool, cancellation_token: CancellationToken) {
        fn send_event(is_loading: bool, apps: Option<Vec<CloudApp>>, error: Option<String>) {
            if let Some(ref a) = apps {
                debug!(count = a.len(), ?error, "Sending app list to UI");
            }
            CloudAppsChangedEvent { is_loading, apps, error }.send_signal_to_dart();
        }

        // Short lock to decide refresh vs cached send
        let (should_refresh, cached_snapshot) = {
            let cache = self.cloud_apps.lock().await;
            let should = cache.is_empty() || force_refresh;
            let snapshot = if should { None } else { Some(cache.clone()) };
            (should, snapshot)
        };

        if !should_refresh {
            debug!(
                count = cached_snapshot.as_ref().map(|v| v.len()).unwrap_or(0),
                "Using cached app list"
            );
            send_event(false, cached_snapshot, None);
            return;
        }

        if cancellation_token.is_cancelled() {
            warn!("App list load cancelled before starting");
            return;
        }

        info!("Loading app list from remote");
        send_event(true, None, None);

        let storage = self.storage.read().await.clone();
        let list_path = self.list_path.clone();
        let cache_dir = self.cache_dir.clone();
        let client = self.http_client.clone();

        let timeout = Duration::from_secs(30);
        let repo = self.repo.clone();
        let fut =
            repo.load_app_list(storage, list_path, &cache_dir, &client, cancellation_token.clone());

        match tokio::time::timeout(timeout, fut).await {
            Ok(Ok(apps)) => {
                debug!(len = apps.len(), "Loaded app list successfully");
                {
                    let mut cache = self.cloud_apps.lock().await;
                    *cache = apps.clone();
                }
                send_event(false, Some(apps), None);
            }
            Ok(Err(e)) => {
                if cancellation_token.is_cancelled() {
                    warn!("App list load cancelled");
                    return;
                }
                error!(error = e.as_ref() as &dyn Error, "Failed to load app list");
                send_event(false, None, Some(format!("Failed to load app list: {e:#}")));
            }
            Err(_) => {
                warn!("App list load timed out");
                send_event(false, None, Some("Timed out while loading app list".into()));
            }
        }
    }

    #[instrument(skip(self), err, ret)]
    pub(crate) async fn download_app(
        &self,
        app_full_name: String,
        progress_tx: UnboundedSender<RcloneTransferStats>,
        // Status updates for non-transfer steps (e.g., extraction)
        stage_tx: tokio::sync::mpsc::UnboundedSender<String>,
        cancellation_token: CancellationToken,
    ) -> Result<String> {
        let dst_dir = self.download_dir.read().await.join(&app_full_name);
        info!(app = %app_full_name, dest = %dst_dir.display(), "Starting app download");

        let source = self.repo.source_for_download(&app_full_name);
        let storage = self.storage.read().await.clone();

        match self
            .repo
            .pre_download(
                &storage,
                &app_full_name,
                &dst_dir,
                &self.http_client,
                &self.cache_dir,
                cancellation_token.clone(),
            )
            .await
        {
            Ok(repo::PreDownloadDecision::SkipAlreadyPresent) => {
                debug!("Pre-download decided to skip transfer");
            }
            _ => {
                storage
                    .download_dir_with_stats(
                        source,
                        dst_dir.clone(),
                        progress_tx,
                        cancellation_token.clone(),
                    )
                    .await?;
            }
        }

        // Prepare metadata inputs without holding long locks
        let cached = self.get_app_by_full_name(&app_full_name).await;
        let write_legacy = *self.write_legacy_release_json.read().await;

        if let Err(e) = metadata::write_download_metadata(
            cached.clone(),
            &app_full_name,
            &dst_dir,
            write_legacy,
        )
        .await
        {
            warn!(
                error = e.as_ref() as &dyn Error,
                dir = %dst_dir.display(),
                "Failed to write download metadata"
            );
        }

        // Layout-specific post-processing (no-op for FFA)
        let _ = stage_tx.send("Processing download...".into());
        if let Err(e) = self
            .repo
            .post_download(
                &app_full_name,
                &dst_dir,
                &self.http_client,
                &self.cache_dir,
                Some(stage_tx.clone()),
                cancellation_token.clone(),
            )
            .await
        {
            warn!(error = e.as_ref() as &dyn Error, dir = %dst_dir.display(), "Post-download step failed");
        }
        // Notify UI that downloads may have changed
        DownloadsChanged {}.send_signal_to_dart();

        Ok(dst_dir.display().to_string())
    }
}
