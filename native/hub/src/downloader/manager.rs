use std::sync::Arc;

use anyhow::{Result, anyhow};
use tokio::sync::RwLock;
use tracing::{debug, instrument};

use crate::downloader::DownloaderSession;

#[derive(Clone, Default)]
pub(crate) struct DownloaderManager {
    current: Arc<RwLock<Option<Arc<DownloaderSession>>>>,
}

impl DownloaderManager {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self { current: Arc::new(RwLock::new(None)) })
    }

    pub(crate) async fn get(&self) -> Option<Arc<DownloaderSession>> {
        self.current.read().await.as_ref().cloned()
    }

    pub(crate) async fn require(&self) -> Result<Arc<DownloaderSession>> {
        self.get().await.ok_or_else(|| {
            anyhow!("Downloader is not configured. Install configuration file to initialize.")
        })
    }

    #[instrument(level = "debug", skip(self, session))]
    pub(crate) async fn replace(&self, session: Arc<DownloaderSession>) {
        debug!("Setting downloader session");
        self.set(Some(session)).await;
    }

    #[instrument(level = "debug", skip(self))]
    pub(crate) async fn clear(&self) {
        debug!("Removing downloader session");
        self.set(None).await;
    }

    async fn set(&self, session: Option<Arc<DownloaderSession>>) {
        let mut guard = self.current.write().await;
        let old = guard.take();
        *guard = session;
        drop(guard);

        if let Some(session) = old {
            session.stop().await;
        }
    }
}
