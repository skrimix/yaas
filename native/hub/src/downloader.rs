use std::{error::Error, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use futures::TryStreamExt;
use rclone::RcloneStorage;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use rinf::{DartSignal, RustSignal};
use tokio::{
    fs::File,
    sync::{Mutex, RwLock},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, info_span, instrument, warn};

use crate::models::{
    AppApiResponse, CloudApp, Settings,
    signals::{
        download::{
            AppDetailsResponse, AppReview, AppReviewsResponse, CloudAppsChangedEvent,
            GetAppDetailsRequest, GetAppReviewsRequest, GetRcloneRemotesRequest,
            LoadCloudAppsRequest, RcloneRemotesChanged,
        },
        downloads_local::DownloadsChanged,
    },
};

mod rclone;
pub use rclone::RcloneTransferStats;

pub struct Downloader {
    cloud_apps: Mutex<Vec<CloudApp>>,
    storage: RwLock<RcloneStorage>,
    download_dir: RwLock<PathBuf>,
    current_load_token: RwLock<CancellationToken>,
}

impl Downloader {
    #[instrument(skip(settings_stream))]
    pub async fn new(mut settings_stream: WatchStream<Settings>) -> Arc<Self> {
        let settings =
            settings_stream.next().await.expect("Settings stream closed on downloader init");
        let handle = Arc::new(Self {
            cloud_apps: Mutex::new(Vec::new()),
            storage: RwLock::new(RcloneStorage::new(
                PathBuf::from(settings.rclone_path),
                None,
                settings.rclone_remote_name,
                settings.bandwidth_limit,
            )),
            download_dir: RwLock::new(PathBuf::from(settings.downloads_location)),
            current_load_token: RwLock::new(CancellationToken::new()),
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
                while let Some(settings) = settings_stream.next().await {
                    info!("Downloader received settings update");
                    debug!(?settings, "New settings");

                    let new_storage = RcloneStorage::new(
                        PathBuf::from(settings.rclone_path),
                        None,
                        settings.rclone_remote_name,
                        settings.bandwidth_limit,
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
                        handle.load_app_list(true, new_token).await; // FIXME: this should set the UI to loading state
                    }

                    let mut download_dir = handle.download_dir.write().await;
                    let new_download_dir = PathBuf::from(settings.downloads_location);
                    if *download_dir != new_download_dir {
                        info!(new_dir = %new_download_dir.display(), "Download directory changed");
                        *download_dir = new_download_dir;
                    }
                }

                panic!("Settings stream closed for Downloader");
            }
        }.instrument(info_span!("task_handle_settings_updates")),
        );

        // On init, send rclone remotes list
        tokio::spawn({
            let handle = handle.clone();
            async move {
                match handle.storage.read().await.remotes().await {
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
                request = load_cloud_apps_receiver.recv() => {
                    if let Some(request) = request {
                        info!(refresh = request.message.refresh, "Received LoadCloudAppsRequest");
                        let token = self.current_load_token.read().await.clone();
                        self.load_app_list(request.message.refresh, token).await;
                        // TODO: add timeout
                    } else {
                        panic!("LoadCloudAppsRequest receiver closed");
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
                        panic!("GetRcloneRemotesRequest receiver closed");
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
                        panic!("GetAppDetailsRequest receiver closed");
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
                        panic!("GetAppReviewsRequest receiver closed");
                    }
                }
            }
        }
    }

    #[instrument(skip(self, cancellation_token))]
    async fn load_app_list(&self, force_refresh: bool, cancellation_token: CancellationToken) {
        fn send_app_list(apps: Vec<CloudApp>, error: Option<String>) {
            debug!(count = apps.len(), ?error, "Sending app list to UI");
            CloudAppsChangedEvent { apps, error }.send_signal_to_dart();
        }

        let mut cache = self.cloud_apps.lock().await;
        if cache.is_empty() || force_refresh {
            if cancellation_token.is_cancelled() {
                warn!("App list load cancelled before starting");
                return;
            }

            info!("Loading app list from remote");
            cache.clear();

            if let Some(result) = cancellation_token.run_until_cancelled(self.get_app_list()).await
            {
                match result {
                    Ok(apps) => {
                        info!(len = apps.len(), "Loaded app list successfully");
                        *cache = apps;
                        send_app_list(cache.clone(), None);
                    }
                    Err(e) => {
                        error!(error = e.as_ref() as &dyn Error, "Failed to load app list");
                        send_app_list(Vec::new(), Some(format!("Failed to load app list: {e:#}")));
                    }
                }
            } else {
                warn!("App list load was cancelled");
            }
        } else {
            info!(count = cache.len(), "Using cached app list");
            send_app_list(cache.clone(), None);
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
        progress_tx: tokio::sync::mpsc::UnboundedSender<RcloneTransferStats>,
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

        // Try to write release metadata for the downloaded directory
        if let Err(e) = self.write_release_metadata(&app_full_name, &dst_dir).await {
            warn!(
                error = e.as_ref() as &dyn Error,
                dir = %dst_dir.display(),
                "Failed to write release.json metadata"
            );
        }
        // Notify UI that downloads may have changed
        DownloadsChanged {}.send_signal_to_dart();

        Ok(dst_dir.display().to_string())
    }
}

#[instrument(err)]
async fn fetch_app_details(package_name: String) -> Result<Option<AppApiResponse>> {
    let url = format!("https://qloader.5698452.xyz/api/v1/oculusgames/{}", package_name);
    debug!(%url, "Fetching app details from QLoader API");

    let client = reqwest::Client::builder().user_agent("YAAS/1.0").build()?;

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
    let client = reqwest::Client::builder().user_agent("YAAS/1.0").build()?;
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
struct ReleaseMetadata {
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
    source_remote: String,
}

impl Downloader {
    #[instrument(skip(self), fields(app_full_name = %app_full_name, dir = %dst_dir.display()), err)]
    async fn write_release_metadata(&self, app_full_name: &str, dst_dir: &PathBuf) -> Result<()> {
        use time::{OffsetDateTime, format_description::well_known::Rfc3339};
        let cached = self.get_app_by_full_name(app_full_name).await;
        let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_else(|_| "".to_string());

        let meta = ReleaseMetadata {
            format_version: 1,
            full_name: app_full_name.to_string(),
            app_name: cached.as_ref().map(|a| a.app_name.clone()),
            package_name: cached.as_ref().map(|a| a.package_name.clone()),
            version_code: cached.as_ref().map(|a| a.version_code),
            last_updated: cached.as_ref().map(|a| a.last_updated.clone()),
            size: cached.as_ref().map(|a| a.size),
            downloaded_at: now,
            source_remote: self.storage.read().await.remote_name().to_string(),
        };

        let json = serde_json::to_string_pretty(&meta)?;
        let release_path = dst_dir.join("release.json");
        tokio::fs::write(&release_path, json)
            .await
            .with_context(|| format!("Failed to write {}", release_path.display()))?;
        info!(path = %release_path.display(), "Wrote release metadata");
        Ok(())
    }
}
