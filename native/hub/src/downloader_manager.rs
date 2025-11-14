use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock};
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, info, instrument};

use crate::{
    downloader::{self, Downloader},
    models::{
        DownloaderConfig,
        signals::{
            downloader::{
                availability::DownloaderAvailabilityChanged,
                setup::{
                    DownloaderConfigInstallResult, InstallDownloaderConfigRequest,
                    RetryDownloaderInitRequest,
                },
            },
            system::Toast,
        },
    },
    settings::SettingsHandler,
};

#[derive(Clone)]
pub struct DownloaderManager {
    inner: Arc<RwLock<Option<Arc<Downloader>>>>,
    init_guard: Arc<Mutex<()>>,
}

impl DownloaderManager {
    pub fn new(initial: Option<Arc<Downloader>>) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(RwLock::new(initial)),
            init_guard: Arc::new(Mutex::new(())),
        })
    }

    #[instrument(skip(self, downloader))]
    pub async fn set_downloader(&self, downloader: Option<Arc<Downloader>>) {
        if self.inner.read().await.is_none() && downloader.is_none() {
            return;
        }
        if downloader.is_some() {
            debug!("Setting downloader instance");
        } else {
            debug!("Removing downloader instance");
        }
        let mut guard = self.inner.write().await;
        let old = guard.take();
        *guard = downloader;
        drop(guard);
        if let Some(d) = old {
            d.stop().await;
        }
    }

    pub async fn get(&self) -> Option<Arc<Downloader>> {
        self.inner.read().await.as_ref().cloned()
    }

    pub async fn is_some(&self) -> bool {
        self.inner.read().await.is_some()
    }

    /// Start downloader lifecycle: initial init, setup handler, and retry handling.
    pub fn start(self: Arc<Self>, app_dir: PathBuf, settings_handler: Arc<SettingsHandler>) {
        if Path::new("downloader.json").exists() {
            let manager = self.clone();
            let app_dir_init = app_dir.clone();
            let settings_handler_init = settings_handler.clone();
            tokio::spawn(async move {
                if let Err(e) = manager
                    .init_from_disk(app_dir_init.clone(), settings_handler_init.clone())
                    .await
                {
                    tracing::error!("Failed to initialize downloader: {:#}", e);
                }
            });
        } else {
            // Attempt to fetch a default downloader.json and initialize
            info!("No downloader.json found, attempting to fetch default");
            let manager = self.clone();
            let app_dir_dl = app_dir.clone();
            let settings_handler_dl = settings_handler.clone();
            tokio::spawn(async move {
                match download_default_downloader_config(&app_dir_dl).await {
                    Ok(()) => {
                        if let Err(e) = manager
                            .init_from_disk(app_dir_dl.clone(), settings_handler_dl.clone())
                            .await
                        {
                            tracing::error!(
                                "Failed to initialize downloader after default config: {:#}",
                                e
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = e.as_ref() as &dyn Error,
                            "Couldn't fetch default downloader.json, cloud features disabled"
                        );
                        DownloaderAvailabilityChanged {
                            available: false,
                            initializing: false,
                            error: Some("Failed to fetch default downloader config".into()),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }

        // Drag&drop config installer handled by the manager
        self.clone().start_setup_handler(app_dir.clone(), settings_handler.clone());

        // Retry init loop (from UI)
        tokio::spawn({
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            let manager = self.clone();
            async move {
                let rx = RetryDownloaderInitRequest::get_dart_signal_receiver();
                while rx.recv().await.is_some() {
                    let _ = manager.init_from_disk(app_dir.clone(), settings_handler.clone()).await;
                }
            }
        });
    }

    pub async fn init_from_disk(
        &self,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) -> Result<()> {
        let _g = self.init_guard.lock().await;
        let cfg = DownloaderConfig::load_from_path("downloader.json")?;
        self.init_with_config(cfg, app_dir, settings_handler).await
    }

    pub async fn init_with_config(
        &self,
        cfg: DownloaderConfig,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) -> Result<()> {
        DownloaderAvailabilityChanged { available: false, initializing: true, error: None }
            .send_signal_to_dart();

        // Drop old downloader before initializing a new one.
        self.set_downloader(None).await;

        let cache_dir = app_dir.join("downloader_cache").join(&cfg.id);
        let _ = tokio::fs::create_dir_all(&cache_dir).await;

        match downloader::artifacts::prepare_artifacts(&cache_dir, &cfg).await {
            Ok((rclone_path, rclone_config_path)) => {
                match Downloader::new(
                    Arc::new(cfg),
                    cache_dir,
                    rclone_path,
                    rclone_config_path,
                    settings_handler.clone(),
                    WatchStream::new(settings_handler.subscribe()),
                )
                .await
                {
                    Ok(downloader) => {
                        self.set_downloader(Some(downloader)).await;
                        DownloaderAvailabilityChanged {
                            available: true,
                            initializing: false,
                            error: None,
                        }
                        .send_signal_to_dart();
                        Ok(())
                    }
                    Err(e) => {
                        DownloaderAvailabilityChanged {
                            available: false,
                            initializing: false,
                            error: Some(format!("Failed to initialize downloader: {:#}", e)),
                        }
                        .send_signal_to_dart();
                        Err(e)
                    }
                }
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

    /// Start handler that watches for InstallDownloaderConfigRequest and installs config.
    pub fn start_setup_handler(
        self: Arc<Self>,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) {
        tokio::spawn(async move {
            let receiver = InstallDownloaderConfigRequest::get_dart_signal_receiver();
            loop {
                match receiver.recv().await {
                    Some(req) => {
                        let src = PathBuf::from(req.message.source_path);
                        debug!(path = %src.display(), "Received InstallDownloaderConfigRequest");
                        let res = install_config(&app_dir, &src).await;
                        match res {
                            Ok(()) => {
                                DownloaderConfigInstallResult { success: true, error: None }
                                    .send_signal_to_dart();

                                Toast::send(
                                    "Downloader config installed".into(),
                                    "Initializing cloud features...".into(),
                                    false,
                                    None,
                                );

                                if let Err(e) = self
                                    .init_from_disk(app_dir.clone(), settings_handler.clone())
                                    .await
                                {
                                    tracing::error!(error = %e, "Downloader init after install failed");
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = e.as_ref() as &dyn Error,
                                    "Failed to install downloader config"
                                );
                                DownloaderConfigInstallResult {
                                    success: false,
                                    error: Some(format!("{:#}", e)),
                                }
                                .send_signal_to_dart();
                                Toast::send(
                                    "Failed to install downloader config".into(),
                                    format!("{:#}", e),
                                    true,
                                    None,
                                );
                            }
                        }
                    }
                    None => panic!("InstallDownloaderConfigRequest receiver closed"),
                }
            }
        });
    }
}

const DEFAULT_DOWNLOADER_CONFIG_URL: &str =
    "https://github.com/skrimix/yaas/releases/download/files/downloader_vrp.json";

#[instrument(skip(app_dir), fields(url = DEFAULT_DOWNLOADER_CONFIG_URL), err)]
async fn download_default_downloader_config(app_dir: &Path) -> Result<()> {
    let tmp_src = app_dir.join("downloader.json.tmp");

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .context("Failed to build HTTP client")?;

    let resp = client
        .get(DEFAULT_DOWNLOADER_CONFIG_URL)
        .send()
        .await
        .context("Failed to request default downloader config")?;
    ensure!(
        resp.status().is_success(),
        "HTTP request failed: {} ({})",
        resp.status(),
        resp.status().canonical_reason().unwrap_or("<unknown reason>")
    );
    let bytes = resp.bytes().await.context("Failed to read response body")?;
    std::fs::write(&tmp_src, &bytes).context("Failed to write temporary downloaded config")?;

    let res = install_config(app_dir, &tmp_src).await;
    let _ = std::fs::remove_file(&tmp_src);
    res
}

#[instrument(skip(app_dir, src), fields(src = %src.display()), err)]
async fn install_config(app_dir: &Path, src: &Path) -> Result<()> {
    ensure!(src.exists(), "Source file not found");
    ensure!(src.is_file(), "Source path is not a file");

    // Validate by parsing
    let cfg = DownloaderConfig::load_from_path(src)?;
    debug!(
        rclone_path = %cfg.rclone_path,
        rclone_config_path = cfg.rclone_config_path.as_deref().unwrap_or("<none>"),
        "Validated downloader.json"
    );

    let dst = app_dir.join("downloader.json");
    let tmp = app_dir.join("downloader.json.tmp");
    let content = std::fs::read_to_string(src)
        .with_context(|| format!("Failed to read {}", src.display()))?;
    std::fs::write(&tmp, content).context("Failed to write temporary config file")?;
    std::fs::rename(&tmp, &dst).with_context(|| format!("Failed to replace {}", dst.display()))?;
    info!(path = %dst.display(), "Installed downloader.json");
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::install_config;

    fn valid_config_json() -> String {
        r#"{
            "id": "test",
            "layout": "ffa",
            "rclone_path": "/bin/echo",
            "rclone_config_path": "/tmp/rclone.conf",
            "disable_randomize_remote": false
        }"#
        .to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn install_config_copies_and_validates() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, valid_config_json()).unwrap();

        install_config(dir.path(), &src).await.expect("install ok");

        let dst = dir.path().join("downloader.json");
        assert!(dst.is_file());
        let content = std::fs::read_to_string(dst).unwrap();
        assert!(content.contains("\"rclone_path\""));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn install_config_fails_for_missing_source() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("missing.json");
        let err = install_config(dir.path(), &src).await.unwrap_err();
        assert!(format!("{:#}", err).contains("Source file not found"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn install_config_fails_for_invalid_json() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("bad.json");
        std::fs::write(&src, "not-json").unwrap();
        let err = install_config(dir.path(), &src).await.unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("Failed to parse downloader.json") || msg.contains("parse"));
    }
}
