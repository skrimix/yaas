use std::sync::Arc;

use nif::NifStorage;
use tokio::sync::Mutex;

use crate::{messages as proto, models::CloudApp};

mod nif;

pub struct Downloader {
    cloud_apps: Mutex<Vec<CloudApp>>,
    storage: NifStorage,
}

impl Downloader {
    pub async fn create() -> Arc<Self> {
        let handle = Arc::new(Self {
            cloud_apps: Mutex::new(Vec::new()),
            storage: NifStorage::create().await.unwrap(), // TODO: handle error
        });
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
            proto::CloudAppsChangedEvent {
                apps: apps.iter().map(|app| app.into_proto()).collect(),
                error,
            }
            .send_signal_to_dart();
        }

        let receiver = proto::LoadCloudAppsRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            let mut cache = self.cloud_apps.lock().await;
            if cache.is_empty() || request.message.refresh {
                let result = self.storage.get_app_list().await;
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
}
