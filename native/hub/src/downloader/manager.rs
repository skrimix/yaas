use std::sync::Arc;

use anyhow::{Result, anyhow};
use tokio::sync::RwLock;
use tracing::{debug, instrument};

use crate::downloader::Downloader;

#[derive(Clone, Default)]
pub(crate) struct DownloaderManager {
    current: Arc<RwLock<Option<Arc<Downloader>>>>,
}

impl DownloaderManager {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self { current: Arc::new(RwLock::new(None)) })
    }

    pub(crate) async fn get(&self) -> Option<Arc<Downloader>> {
        self.current.read().await.as_ref().cloned()
    }

    pub(crate) async fn require(&self) -> Result<Arc<Downloader>> {
        self.get().await.ok_or_else(|| {
            anyhow!("Downloader is not configured. Install configuration file to initialize.")
        })
    }

    #[instrument(level = "debug", skip(self, downloader))]
    pub(crate) async fn replace(&self, downloader: Arc<Downloader>) {
        debug!("Setting downloader instance");
        self.set(Some(downloader)).await;
    }

    #[instrument(level = "debug", skip(self))]
    pub(crate) async fn clear(&self) {
        debug!("Removing downloader instance");
        self.set(None).await;
    }

    async fn set(&self, downloader: Option<Arc<Downloader>>) {
        let mut guard = self.current.write().await;
        let old = guard.take();
        *guard = downloader;
        drop(guard);

        if let Some(downloader) = old {
            downloader.stop().await;
        }
    }
}
