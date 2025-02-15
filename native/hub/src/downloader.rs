use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use futures::TryStreamExt;
use tokio::{fs::File, sync::Mutex};
use tracing::info;

use crate::{messages as proto, models::CloudApp};

mod webdav;

pub struct Downloader {
    cloud_apps: Mutex<Vec<CloudApp>>,
}

impl Downloader {
    pub fn create() -> Arc<Self> {
        let handle = Arc::new(Self { cloud_apps: Mutex::new(Vec::new()) });
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
            proto::GetCloudAppsResponse {
                apps: apps.iter().map(|app| app.into_proto()).collect(),
                error,
            }
            .send_signal_to_dart();
        }

        let receiver = proto::GetCloudAppsRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            let mut cache = self.cloud_apps.lock().await;
            if cache.is_empty() || request.message.refresh {
                let result = load_app_list().await;
                match result {
                    Ok(apps) => {
                        *cache = apps;
                        send_response(cache.clone(), None);
                    }
                    Err(e) => {
                        send_response(Vec::new(), Some(format!("Failed to load app list: {}", e)));
                    }
                }
            } else {
                send_response(cache.clone(), None);
            }
        }
    }
}

pub async fn load_app_list() -> Result<Vec<CloudApp>> {
    let path = PathBuf::from("/home/skrimix/Desktop/Loader/metadata/FFA.txt");
    let file = File::open(path).await?;
    let mut reader = csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
    let records = reader.deserialize();
    let cloud_apps: Vec<CloudApp> = records.map_ok(|r| r).try_collect().await?;
    info!("Loaded {} cloud apps", cloud_apps.len());
    Ok(cloud_apps)
}
