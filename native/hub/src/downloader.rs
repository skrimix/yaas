use std::{error::Error, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, ensure};
use futures::TryStreamExt;
use rand::seq::IndexedRandom;
use rclone::RcloneStorage;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use rinf::{DartSignal, RustSignal};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    fs::File,
    sync::{Mutex, RwLock, mpsc::UnboundedSender},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, info_span, instrument, warn};

use crate::{
    models::{
        AppApiResponse, CloudApp, DownloaderConfig, Settings,
        signals::{
            cloud_apps::{
                details::{AppDetailsResponse, GetAppDetailsRequest},
                list::{CloudAppsChangedEvent, LoadCloudAppsRequest},
                reviews::{AppReview, AppReviewsResponse, GetAppReviewsRequest},
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
pub mod setup;

pub struct Downloader {
    config: Arc<DownloaderConfig>,
    rclone_path: PathBuf,
    rclone_config_path: PathBuf,
    cloud_apps: Mutex<Vec<CloudApp>>,
    storage: RwLock<RcloneStorage>,
    download_dir: RwLock<PathBuf>,
    current_load_token: RwLock<CancellationToken>,
    write_legacy_release_json: RwLock<bool>,
    cancel_token: CancellationToken,
}

impl Downloader {
    #[instrument(skip(settings_stream))]
    pub async fn new(
        config: Arc<DownloaderConfig>,
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
            settings.rclone_remote_name.clone(),
            settings.bandwidth_limit.clone(),
            config.remote_name_filter_regex.clone(),
        );
        let storage = match storage.remotes().await {
            Ok(remotes) if config.randomize_remote => {
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

        let handle = Arc::new(Self {
            config,
            rclone_path,
            rclone_config_path,
            cloud_apps: Mutex::new(Vec::new()),
            storage: RwLock::new(storage),
            download_dir: RwLock::new(PathBuf::from(settings.downloads_location)),
            current_load_token: RwLock::new(CancellationToken::new()),
            write_legacy_release_json: RwLock::new(settings.write_legacy_release_json),
            cancel_token: CancellationToken::new(),
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
                            info!("Downloader received settings update");
                            debug!(?settings, "New settings");

                            let new_storage = RcloneStorage::new(
                                handle.rclone_path.clone(),
                                handle.rclone_config_path.clone(),
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
                                info!(new_dir = %new_download_dir.display(), "Download directory changed");
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
                        info!(refresh = request.message.refresh, "Received LoadCloudAppsRequest");
                        let token = self.current_load_token.read().await.clone();
                        self.load_app_list(request.message.refresh, token).await;
                        // TODO: add timeout
                    } else {
                        info!("LoadCloudAppsRequest receiver closed; shutting down downloader command loop");
                        return;
                    }
                }
                request = get_rclone_remotes_receiver.recv() => {
                    if request.is_some() {
                        info!("Received GetRcloneRemotesRequest");
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
                        info!(%package_name, "Received GetAppDetailsRequest");
                        tokio::spawn(async move {
                            match fetch_app_details(package_name.clone()).await {
                                Ok(Some(api)) => {
                                    let AppApiResponse {
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
                        let sort_by = request
                            .message
                            .sort_by
                            .unwrap_or_else(|| "helpful".to_string());
                        info!(%app_id, "Received GetAppReviewsRequest");
                        tokio::spawn(async move {
                            match fetch_app_reviews(&app_id, limit, offset, &sort_by).await {
                                Ok(reviews) => {
                                    AppReviewsResponse {
                                        app_id,
                                        total: Some(reviews.total),
                                        reviews: reviews.reviews,
                                        error: None,
                                    }
                                    .send_signal_to_dart();
                                }
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Failed to fetch app reviews");
                                    AppReviewsResponse {
                                        app_id,
                                        total: None,
                                        reviews: Vec::new(),
                                        error: Some(format!("Failed to fetch reviews: {:#}", e)),
                                    }
                                    .send_signal_to_dart();
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

    pub async fn shutdown(&self) {
        info!("Shutting down Downloader instance");
        // Cancel command loop and settings listener
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

        let mut cache = self.cloud_apps.lock().await;
        if cache.is_empty() || force_refresh {
            if cancellation_token.is_cancelled() {
                warn!("App list load cancelled before starting");
                return;
            }

            info!("Loading app list from remote");
            send_event(true, None, None);
            cache.clear();

            if let Some(result) = cancellation_token.run_until_cancelled(self.get_app_list()).await
            {
                match result {
                    Ok(apps) => {
                        info!(len = apps.len(), "Loaded app list successfully");
                        *cache = apps;
                        send_event(false, Some(cache.clone()), None);
                    }
                    Err(e) => {
                        error!(error = e.as_ref() as &dyn Error, "Failed to load app list");
                        send_event(false, None, Some(format!("Failed to load app list: {e:#}")));
                    }
                }
            } else {
                warn!("App list load was cancelled");
                send_event(false, None, None);
            }
        } else {
            info!(count = cache.len(), "Using cached app list");
            send_event(false, Some(cache.clone()), None);
        }
    }

    #[instrument(skip(self), fields(count))]
    async fn get_app_list(&self) -> Result<Vec<CloudApp>> {
        let path = self
            .storage
            .read()
            .await
            .clone()
            .download_file("FFA.txt".to_string(), self.download_dir.read().await.clone())
            .await
            .context("Failed to download game list file")?;

        debug!(path = %path.display(), "App list file downloaded, parsing...");
        let file = File::open(&path).await.context("could not open game list file")?;
        let mut reader =
            csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
        let records = reader.deserialize();
        let cloud_apps: Vec<CloudApp> =
            records.try_collect().await.context("Failed to parse game list file")?;

        Span::current().record("count", cloud_apps.len());
        Ok(cloud_apps)
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

        // Try to write download metadata for the downloaded directory
        if let Err(e) = self.write_download_metadata(&app_full_name, &dst_dir).await {
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

    match artifacts::prepare_artifacts(&app_dir, &cfg).await {
        Ok((rclone_path, rclone_config_path)) => {
            let downloader = Downloader::new(
                Arc::new(cfg),
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

#[instrument(err)]
async fn fetch_app_details(package_name: String) -> Result<Option<AppApiResponse>> {
    let url = format!("https://qloader.5698452.xyz/api/v1/oculusgames/{}", package_name);
    debug!(%url, "Fetching app details from QLoader API");

    let client = reqwest::Client::builder().user_agent(crate::USER_AGENT).build()?;

    let resp = client.get(&url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    resp.error_for_status_ref()?;

    let api: AppApiResponse = resp.json().await?;
    Ok(Some(api))
}

#[derive(serde::Deserialize)]
struct ReviewsResponse {
    #[serde(default)]
    reviews: Vec<AppReview>,
    #[serde(default)]
    total: u32,
}

async fn fetch_app_reviews(
    app_id: &str,
    limit: u32,
    offset: u32,
    sort_by: &str,
) -> Result<ReviewsResponse> {
    ensure!(sort_by == "helpful" || sort_by == "newest", "Invalid sort_by value: {}", sort_by);
    let client = reqwest::Client::builder().user_agent(crate::USER_AGENT).build()?;
    let url = "https://reviews.5698452.xyz";

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

    let response = client
        .get(url)
        .headers(headers)
        .query(&[
            ("appId", app_id),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("sortBy", sort_by),
        ])
        .send()
        .await?;

    response.error_for_status_ref()?;
    let payload: ReviewsResponse = response.json().await?;
    Ok(payload)
}

#[derive(serde::Serialize)]
struct DownloadMetadata {
    #[serde(default)]
    format_version: u32,
    full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    downloaded_at: String,
}

impl Downloader {
    #[instrument(skip(self), fields(app_full_name = %app_full_name, dir = %dst_dir.display()), err)]
    async fn write_download_metadata(&self, app_full_name: &str, dst_dir: &PathBuf) -> Result<()> {
        let cached = self.get_app_by_full_name(app_full_name).await;
        let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_else(|_| "".to_string());

        let meta = DownloadMetadata {
            format_version: 1,
            full_name: app_full_name.to_string(),
            app_name: cached.as_ref().map(|a| a.app_name.clone()),
            package_name: cached.as_ref().map(|a| a.package_name.clone()),
            version_code: cached.as_ref().map(|a| a.version_code),
            last_updated: cached.as_ref().map(|a| a.last_updated.clone()),
            size: cached.as_ref().map(|a| a.size),
            downloaded_at: now,
        };

        let json = serde_json::to_string_pretty(&meta)?;
        let download_path = dst_dir.join("metadata.json");
        tokio::fs::write(&download_path, json)
            .await
            .with_context(|| format!("Failed to write {}", download_path.display()))?;
        info!(path = %download_path.display(), "Wrote download metadata");

        // Optionally write legacy release.json file for compatibility
        if *self.write_legacy_release_json.read().await {
            if let Some(app) = cached.as_ref() {
                #[derive(serde::Serialize)]
                struct LegacyReleaseJson<'a> {
                    #[serde(rename = "GameName")]
                    game_name: &'a str,
                    #[serde(rename = "ReleaseName")]
                    release_name: &'a str,
                    #[serde(rename = "PackageName")]
                    package_name: &'a str,
                    #[serde(rename = "VersionCode")]
                    version_code: u32,
                    #[serde(rename = "LastUpdated")]
                    last_updated: &'a str,
                    #[serde(rename = "GameSize")]
                    game_size: u64,
                }

                let size_mb = app.size / 1_000_000;
                let legacy = LegacyReleaseJson {
                    game_name: &app.app_name,
                    release_name: app_full_name,
                    package_name: &app.package_name,
                    version_code: app.version_code,
                    last_updated: &app.last_updated,
                    game_size: size_mb,
                };

                let legacy_json = serde_json::to_string_pretty(&legacy)?;
                let legacy_path = dst_dir.join("release.json");
                tokio::fs::write(&legacy_path, legacy_json)
                    .await
                    .with_context(|| format!("Failed to write {}", legacy_path.display()))?;
                info!(path = %legacy_path.display(), "Wrote legacy release.json metadata");
            } else {
                // Not fatal; just log for visibility
                warn!(app_full_name, "Could not write legacy release.json: app not found in cache");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        downloads_catalog::DownloadsCatalog,
        settings::SettingsHandler,
        adb::AdbHandler,
    };
    use crate::models::RclonePath;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_settings(path: &Path, downloads: &Path, backups: &Path) {
        let settings = crate::models::Settings {
            // Force a harmless binary for ADB to avoid starting a real server
            adb_path: "/bin/true".to_string(),
            downloads_location: downloads.to_string_lossy().to_string(),
            backups_location: backups.to_string_lossy().to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        std::fs::write(path.join("settings.json"), json).unwrap();
    }

    fn cfg_local(bin: &str, conf: &str, randomize: bool) -> DownloaderConfig {
        DownloaderConfig {
            rclone_path: RclonePath::Single(bin.to_string()),
            rclone_config_path: conf.to_string(),
            remote_name_filter_regex: None,
            randomize_remote: randomize,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn init_with_config_success_local_paths() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let dld = app_dir.join("dl");
        let bkp = app_dir.join("bk");
        std::fs::create_dir_all(&dld).unwrap();
        std::fs::create_dir_all(&bkp).unwrap();
        write_settings(&app_dir, &dld, &bkp);

        let settings = SettingsHandler::new(app_dir.clone());

        let adb = AdbHandler::new(WatchStream::new(settings.subscribe())).await;
        let downloads = DownloadsCatalog::start(WatchStream::new(settings.subscribe()));
        let task_manager = TaskManager::new(adb, None, downloads, WatchStream::new(settings.subscribe()));

        let cfg = cfg_local("/bin/echo", "/tmp/rclone.conf", false);

        init_with_config(cfg, app_dir, settings.clone(), task_manager.clone())
            .await
            .expect("init ok");

        assert!(task_manager.__test_has_downloader().await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn init_with_config_propagates_artifact_error() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let dld = app_dir.join("dl");
        let bkp = app_dir.join("bk");
        std::fs::create_dir_all(&dld).unwrap();
        std::fs::create_dir_all(&bkp).unwrap();
        write_settings(&app_dir, &dld, &bkp);

        let settings = SettingsHandler::new(app_dir.clone());
        let adb = AdbHandler::new(WatchStream::new(settings.subscribe())).await;
        let downloads = DownloadsCatalog::start(WatchStream::new(settings.subscribe()));
        let task_manager = TaskManager::new(adb, None, downloads, WatchStream::new(settings.subscribe()));

        // Mismatched URL/local to make prepare_artifacts fail early
        let cfg = DownloaderConfig {
            rclone_path: RclonePath::Single("http://127.0.0.1/rclone".to_string()),
            rclone_config_path: "/tmp/rclone.conf".to_string(),
            remote_name_filter_regex: None,
            randomize_remote: true,
        };

        let err = init_with_config(cfg, app_dir, settings.clone(), task_manager.clone())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("must both be local or both be URLs"));
        assert!(!task_manager.__test_has_downloader().await);
    }
}
