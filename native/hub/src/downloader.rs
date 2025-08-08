use std::{error::Error, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use futures::TryStreamExt;
use rclone::RcloneStorage;
use rinf::{DartSignal, RustSignal};
use tokio::{
    fs::File,
    sync::{Mutex, RwLock},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, error, info, instrument, warn};

use crate::models::{
    CloudApp, Settings,
    signals::download::{CloudAppsChangedEvent, LoadCloudAppsRequest},
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
        });
        handle
    }

    #[instrument(skip(self))]
    pub async fn receive_commands(&self) {
        let receiver = LoadCloudAppsRequest::get_dart_signal_receiver();
        info!("Listening for LoadCloudAppsRequest");
        while let Some(request) = receiver.recv().await {
            info!(refresh = request.message.refresh, "Received LoadCloudAppsRequest");
            let token = self.current_load_token.read().await.clone();
            self.load_app_list(request.message.refresh, token).await;
            // TODO: add timeout
        }

        panic!("LoadCloudAppsRequest receiver closed");
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
            .context("failed to download game list file")?;

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

        Ok(dst_dir.display().to_string())
    }
}
