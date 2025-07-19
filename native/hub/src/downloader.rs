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
use tracing::{debug, error};

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
                while let Some(settings) = settings_stream.next().await {
                    let new_storage = RcloneStorage::new(
                        PathBuf::from(settings.rclone_path),
                        None,
                        settings.rclone_remote_name,
                        settings.bandwidth_limit,
                    );
                    if new_storage != *handle.storage.read().await {
                        // Cancel any load in progress
                        handle.current_load_token.read().await.cancel();
                        let new_token = CancellationToken::new();
                        *handle.current_load_token.write().await = new_token.clone();

                        *handle.storage.write().await = new_storage;

                        // Refresh app list
                        handle.load_app_list(true, new_token).await; // FIXME: this should set the UI to loading state
                    }

                    *handle.download_dir.write().await = PathBuf::from(settings.downloads_location);
                }
            }
        });
        handle
    }

    pub async fn receive_commands(&self) {
        let receiver = LoadCloudAppsRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            let token = self.current_load_token.read().await.clone();
            self.load_app_list(request.message.refresh, token).await; // TODO: add timeout
        }
    }

    async fn load_app_list(&self, force_refresh: bool, cancellation_token: CancellationToken) {
        fn send_app_list(apps: Vec<CloudApp>, error: Option<String>) {
            CloudAppsChangedEvent { apps, error }.send_signal_to_dart();
        }
        let mut cache = self.cloud_apps.lock().await;
        if cache.is_empty() || force_refresh {
            if cancellation_token.is_cancelled() {
                return;
            }

            cache.clear();

            if let Some(result) = cancellation_token.run_until_cancelled(self.get_app_list()).await
            {
                match result {
                    Ok(apps) => {
                        debug!(len = apps.len(), "Loaded app list");
                        *cache = apps;
                        send_app_list(cache.clone(), None);
                    }
                    Err(e) => {
                        error!(error = e.as_ref() as &dyn Error, "Failed to load app list");
                        send_app_list(Vec::new(), Some(format!("Failed to load app list: {e:#}")));
                    }
                }
            } else {
                // TODO: should this be an error?
            }
        } else {
            send_app_list(cache.clone(), None);
        }
    }

    async fn get_app_list(&self) -> Result<Vec<CloudApp>> {
        let path = self
            .storage
            .read()
            .await
            .clone()
            .download_file("FFA.txt".to_string(), self.download_dir.read().await.clone())
            .await
            .context("failed to download game list file")?;
        let file = File::open(path).await.context("could not open game list file")?;
        let mut reader =
            csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
        let records = reader.deserialize();
        let cloud_apps: Vec<CloudApp> =
            records.try_collect().await.context("Failed to parse game list file")?;
        Ok(cloud_apps)
    }

    pub async fn download_app(
        &self,
        app_full_name: String,
        progress_tx: tokio::sync::mpsc::UnboundedSender<RcloneTransferStats>,
    ) -> Result<String> {
        let dst_dir = self.download_dir.read().await.join(&app_full_name);
        self.storage
            .read()
            .await
            .clone()
            .download_dir_with_stats(app_full_name, dst_dir.clone(), progress_tx)
            .await?;
        Ok(dst_dir.display().to_string())
    }
}
