use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use futures::TryStreamExt;
use rclone::RcloneStorage;
use rinf::{DartSignal, RustSignal};
// use nif::NifStorage;
use tokio::{fs::File, sync::Mutex};

use crate::{
    models::CloudApp,
    signals::download::{CloudAppsChangedEvent, LoadCloudAppsRequest},
};

// mod nif;
mod rclone;
// pub use nif::DirDownloadProgress;
pub use rclone::RcloneTransferStats;

pub struct Downloader {
    cloud_apps: Mutex<Vec<CloudApp>>,
    storage: RcloneStorage,
}

impl Downloader {
    pub async fn new() -> Arc<Self> {
        let handle =
            Arc::new(Self { cloud_apps: Mutex::new(Vec::new()), storage: RcloneStorage::new() });
        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_commands().await;
            }
        });
        handle
    }

    pub async fn receive_commands(&self) {
        fn send_response(apps: Vec<CloudApp>, error: Option<String>) {
            CloudAppsChangedEvent { apps, error }.send_signal_to_dart();
        }

        let receiver = LoadCloudAppsRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            let mut cache = self.cloud_apps.lock().await;
            if cache.is_empty() || request.message.refresh {
                let result = self.get_app_list().await;
                match result {
                    Ok(apps) => {
                        *cache = apps;
                        send_response(cache.clone(), None);
                    }
                    Err(e) => {
                        send_response(
                            Vec::new(),
                            Some(format!("Failed to load app list: {:#}", e)),
                        );
                    }
                }
            } else {
                send_response(cache.clone(), None);
            }
        }
    }

    async fn get_app_list(&self) -> Result<Vec<CloudApp>> {
        let path = self
            .storage
            .download_file("FFA.txt".to_string(), PathBuf::from("/home/skrimix/work/test"))
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
        let dst_dir = PathBuf::from("/home/skrimix/work/test").join(&app_full_name);
        self.storage.download_dir_with_stats(app_full_name, dst_dir.clone(), progress_tx).await?;
        Ok(dst_dir.display().to_string())
    }
}
