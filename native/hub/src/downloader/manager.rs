use std::sync::Arc;

use anyhow::{Result, anyhow};
use tokio::sync::RwLock;
use tracing::{debug, instrument};

use crate::downloader::DownloaderSession;

pub(crate) struct DownloaderManager {
    current: RwLock<Option<Arc<DownloaderSession>>>,
}

impl DownloaderManager {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self { current: RwLock::new(None) })
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
    pub(crate) async fn set(&self, session: Option<Arc<DownloaderSession>>) {
        match &session {
            Some(_) => debug!("Setting downloader session"),
            None => debug!("Removing downloader session"),
        }
        let mut guard = self.current.write().await;
        let old = std::mem::replace(&mut *guard, session);
        drop(guard);

        if let Some(session) = old {
            session.stop().await;
        }
    }
}
