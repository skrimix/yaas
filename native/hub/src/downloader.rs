use std::{error::Error, path::PathBuf, sync::Arc, time::Duration};

use anyhow::Result;
use rinf::{DartSignal, RustSignal};
use tokio::{
    fs,
    sync::{Mutex, RwLock, mpsc::UnboundedSender},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, instrument, warn};

use crate::{
    downloader::rclone::RcloneStorage,
    models::{
        CloudApp, DownloaderConfig, Settings,
        signals::{
            cloud_apps::{
                details::{AppDetailsResponse, GetAppDetailsRequest},
                list::{CloudAppsChangedEvent, LoadCloudAppsRequest},
                reviews::{AppReviewsResponse, GetAppReviewsRequest},
            },
            downloader::availability::DownloaderAvailabilityChanged,
            downloads_local::DownloadsChanged,
            storage::remotes::{GetRcloneRemotesRequest, RcloneRemotesChanged},
        },
    },
    settings::SettingsHandler,
    task::TaskManager,
};

mod rclone;
pub use rclone::RcloneTransferStats;
pub mod artifacts;
mod cloud_api;
mod cloud_list;
mod metadata;
pub mod setup;

pub struct Downloader {
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
}

impl Downloader {
    #[instrument(skip(settings_stream))]
    pub async fn new(
        config: Arc<DownloaderConfig>,
        cache_dir: PathBuf,
        rclone_path: PathBuf,
        rclone_config_path: PathBuf,
        settings_handler: Arc<SettingsHandler>,
        mut settings_stream: WatchStream<Settings>,
    ) -> Arc<Self> {
        let settings =
            settings_stream.next().await.expect("Settings stream closed on downloader init");

        let storage = RcloneStorage::new(
            rclone_path.clone(),
            rclone_config_path.clone(),
            config.root_dir.clone(),
            settings.rclone_remote_name.clone(),
            settings.bandwidth_limit.clone(),
            config.remote_name_filter_regex.clone(),
        );
        let storage = match storage.remotes().await {
            Ok(remotes) if config.randomize_remote => {
                use rand::seq::IndexedRandom;
                let mut rng = rand::rng();
                let remote = remotes.choose(&mut rng).unwrap_or(&settings.rclone_remote_name);
                if remote != &settings.rclone_remote_name {
                    let mut updated = settings.clone();
                    updated.rclone_remote_name = remote.to_string();
                    if let Err(e) = settings_handler.save_settings(&updated) {
                        warn!(
                            error = e.as_ref() as &dyn std::error::Error,
                            "Failed to persist randomized rclone remote"
                        );
                    }
                }

                RcloneStorage::new(
                    rclone_path.clone(),
                    rclone_config_path.clone(),
                    config.root_dir.clone(),
                    remote.to_string(),
                    settings.bandwidth_limit.clone(),
                    config.remote_name_filter_regex.clone(),
                )
            }
            Ok(_) => storage,
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn std::error::Error,
                    "Failed to get rclone remotes on init"
                );
                storage
            }
        };

        let root_dir = config.root_dir.clone();
        let list_path = config.list_path.clone();

        let http_client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let handle = Arc::new(Self {
            config,
            cache_dir,
            rclone_path,
            rclone_config_path,
            root_dir,
            list_path,
            cloud_apps: Mutex::new(Vec::new()),
            storage: RwLock::new(storage),
            download_dir: RwLock::new(PathBuf::from(settings.downloads_location)),
            current_load_token: RwLock::new(CancellationToken::new()),
            write_legacy_release_json: RwLock::new(settings.write_legacy_release_json),
            cancel_token: CancellationToken::new(),
            http_client,
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
                info!("Starting to listen for settings changes");
                loop {
                    tokio::select! {
                        _ = handle.cancel_token.cancelled() => {
                            info!("Downloader settings listener cancelled; exiting");
                            return;
                        }
                        maybe_settings = settings_stream.next() => {
                            let Some(settings) = maybe_settings else {
                                info!("Settings stream closed; exiting downloader settings listener");
                                return;
                            };
                            debug!("Downloader received settings update");
                            debug!(?settings, "New settings");

                            let new_storage = RcloneStorage::new(
                                handle.rclone_path.clone(),
                                handle.rclone_config_path.clone(),
                                handle.root_dir.clone(),
                                settings.rclone_remote_name,
                                settings.bandwidth_limit,
                                handle.config.remote_name_filter_regex.clone(),
                            );

                            if new_storage != *handle.storage.read().await {
                                info!("Rclone storage config changed, recreating and refreshing app list");
                                // Cancel any load in progress
                                handle.current_load_token.read().await.cancel();
                                let new_token = CancellationToken::new();
                                *handle.current_load_token.write().await = new_token.clone();

                                *handle.storage.write().await = new_storage;

                                match handle.storage.read().await.remotes().await {
                                    Ok(remotes) => {
                                        RcloneRemotesChanged { remotes, error: None }.send_signal_to_dart();
                                    }
                                    Err(e) => {
                                        error!(error = e.as_ref() as &dyn std::error::Error, "Failed to get rclone remotes after reload");
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
                        info!("Downloader cancelled before sending initial remotes");
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
                                    error = e.as_ref() as &dyn std::error::Error,
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

        // On init, load cloud apps list in the background
        tokio::spawn({
            let handle = handle.clone();
            async move {
                let token = handle.current_load_token.read().await.clone();
                handle.load_app_list(false, token).await;
            }
        });
        handle
    }

    /// Returns the cached CloudApp (if any) that matches the given full name
    pub async fn get_app_by_full_name(&self, full_name: &str) -> Option<CloudApp> {
        let cache = self.cloud_apps.lock().await;
        cache.iter().find(|a| a.full_name == full_name).cloned()
    }

    /// Returns all cached CloudApps for a given package name
    pub async fn get_apps_by_package(&self, package_name: &str) -> Vec<CloudApp> {
        let cache = self.cloud_apps.lock().await;
        cache.iter().filter(|a| a.package_name == package_name).cloned().collect()
    }

    /// Returns the current downloads directory
    pub async fn get_download_dir(&self) -> PathBuf {
        self.download_dir.read().await.clone()
    }

    #[instrument(skip(self))]
    pub async fn receive_commands(&self) {
        let load_cloud_apps_receiver = LoadCloudAppsRequest::get_dart_signal_receiver();
        let get_rclone_remotes_receiver = GetRcloneRemotesRequest::get_dart_signal_receiver();
        let get_app_details_receiver = GetAppDetailsRequest::get_dart_signal_receiver();
        let get_app_reviews_receiver = GetAppReviewsRequest::get_dart_signal_receiver();
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    info!("Downloader command loop cancelled; exiting");
                    return;
                }
                request = load_cloud_apps_receiver.recv() => {
                    if let Some(request) = request {
                        debug!(refresh = request.message.refresh, "Received LoadCloudAppsRequest");
                        let token = self.current_load_token.read().await.clone();
                        self.load_app_list(request.message.refresh, token).await;
                    } else {
                        info!("LoadCloudAppsRequest receiver closed; shutting down downloader command loop");
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
                        info!("GetRcloneRemotesRequest receiver closed; shutting down downloader command loop");
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
                        info!("GetAppDetailsRequest receiver closed; shutting down downloader command loop");
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
                        info!("GetAppReviewsRequest receiver closed; shutting down downloader command loop");
                        return;
                    }
                }
            }
        }
    }

    pub async fn stop(&self) {
        self.cancel_token.cancel();
        // Cancel any ongoing load
        self.current_load_token.read().await.cancel();
    }

    #[instrument(skip(self, cancellation_token))]
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

        // Hard timeout to avoid hanging UI if rclone stalls
        let timeout = Duration::from_secs(30);
        let fut =
            cloud_list::fetch_app_list(storage, list_path, cache_dir, cancellation_token.clone());

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
    pub async fn download_app(
        &self,
        app_full_name: String,
        progress_tx: UnboundedSender<RcloneTransferStats>,
        cancellation_token: CancellationToken,
    ) -> Result<String> {
        let dst_dir = self.download_dir.read().await.join(&app_full_name);
        info!(app = %app_full_name, dest = %dst_dir.display(), "Starting app download");

        self.storage
            .read()
            .await
            .clone()
            .download_dir_with_stats(
                app_full_name.clone(),
                dst_dir.clone(),
                progress_tx,
                cancellation_token,
            )
            .await?;

        // Prepare metadata inputs without holding long locks
        let cached = self.get_app_by_full_name(&app_full_name).await;
        let write_legacy = *self.write_legacy_release_json.read().await;

        if let Err(e) =
            metadata::write_download_metadata(cached, &app_full_name, &dst_dir, write_legacy).await
        {
            warn!(
                error = e.as_ref() as &dyn Error,
                dir = %dst_dir.display(),
                "Failed to write download metadata"
            );
        }
        // Notify UI that downloads may have changed
        DownloadsChanged {}.send_signal_to_dart();

        Ok(dst_dir.display().to_string())
    }
}

/// Initialize the downloader from a config loaded off disk and attach it to the TaskManager.
/// Sends availability and progress signals appropriately.
#[tracing::instrument(skip(settings_handler, task_manager))]
pub async fn init_from_disk(
    app_dir: PathBuf,
    settings_handler: Arc<SettingsHandler>,
    task_manager: Arc<TaskManager>,
) -> Result<()> {
    let cfg = DownloaderConfig::load_from_path("downloader.json")?;
    init_with_config(cfg, app_dir, settings_handler, task_manager).await
}

/// Initialize the downloader from a provided config and attach it to the TaskManager.
/// This function emits DownloaderAvailabilityChanged before and after initialization.
#[tracing::instrument(skip(settings_handler, task_manager))]
pub async fn init_with_config(
    cfg: DownloaderConfig,
    app_dir: PathBuf,
    settings_handler: Arc<SettingsHandler>,
    task_manager: Arc<TaskManager>,
) -> Result<()> {
    DownloaderAvailabilityChanged { available: false, initializing: true, error: None }
        .send_signal_to_dart();

    let cache_dir = app_dir.join("downloader_cache");
    fs::create_dir_all(&cache_dir).await.ok();

    match artifacts::prepare_artifacts(&cache_dir, &cfg).await {
        Ok((rclone_path, rclone_config_path)) => {
            let downloader = Downloader::new(
                Arc::new(cfg),
                cache_dir,
                rclone_path,
                rclone_config_path,
                settings_handler.clone(),
                WatchStream::new(settings_handler.subscribe()),
            )
            .await;
            task_manager.set_downloader(Some(downloader)).await;
            DownloaderAvailabilityChanged { available: true, initializing: false, error: None }
                .send_signal_to_dart();
            Ok(())
        }
        Err(e) => {
            DownloaderAvailabilityChanged {
                available: false,
                initializing: false,
                error: Some(format!("Failed to prepare downloader: {:#}", e)),
            }
            .send_signal_to_dart();
            Err(e)
        }
    }
}
