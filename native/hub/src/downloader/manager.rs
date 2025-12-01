use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock};
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    downloader::{self, Downloader, config::DownloaderConfig, http_cache},
    models::signals::{
        downloader::{
            availability::DownloaderAvailabilityChanged,
            setup::{
                DownloaderConfigInstallResult, InstallDownloaderConfigFromUrlRequest,
                InstallDownloaderConfigRequest, RetryDownloaderInitRequest,
            },
        },
        system::Toast,
    },
    settings::SettingsHandler,
};

#[derive(Clone)]
pub(crate) struct DownloaderManager {
    inner: Arc<RwLock<Option<Arc<Downloader>>>>,
    init_guard: Arc<Mutex<()>>,
}

impl DownloaderManager {
    pub(crate) fn new(initial: Option<Arc<Downloader>>) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(RwLock::new(initial)),
            init_guard: Arc::new(Mutex::new(())),
        })
    }

    #[instrument(level = "debug", skip(self, downloader))]
    async fn set_downloader(&self, downloader: Option<Arc<Downloader>>) {
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

    pub(crate) async fn get(&self) -> Option<Arc<Downloader>> {
        self.inner.read().await.as_ref().cloned()
    }

    pub(crate) async fn is_some(&self) -> bool {
        self.inner.read().await.is_some()
    }

    pub(crate) fn start(self: Arc<Self>, app_dir: PathBuf, settings_handler: Arc<SettingsHandler>) {
        let manager = self.clone();
        let app_dir_init = app_dir.clone();
        let settings_handler_init = settings_handler.clone();
        tokio::spawn(async move {
            if Path::new("downloader.json").exists() {
                if let Err(e) = manager
                    .init_from_disk(app_dir_init.clone(), settings_handler_init.clone())
                    .await
                {
                    error!("Failed to initialize downloader: {:#}", e);
                }
            } else {
                info!("No downloader.json found, cloud features disabled");
                DownloaderAvailabilityChanged {
                    available: false,
                    initializing: false,
                    error: None,
                    config_id: None,
                    is_donation_configured: false,
                    needs_setup: true,
                }
                .send_signal_to_dart();
            }
        });

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

    pub(crate) async fn init_from_disk(
        &self,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) -> Result<()> {
        let _g = self.init_guard.lock().await;
        let cfg_path = app_dir.join("downloader.json");
        let mut cfg = DownloaderConfig::load_from_path(&cfg_path)?;

        if let Some(update_url) = cfg.config_update_url.as_deref() {
            if let Err(e) = maybe_update_config_from_url(&app_dir, &cfg.id, update_url).await {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    "Failed to update downloader config from URL, using local copy"
                );
            } else {
                cfg = DownloaderConfig::load_from_path(&cfg_path)?;
            }
        }

        self.init_with_config(cfg, app_dir, settings_handler).await
    }

    pub(crate) async fn init_with_config(
        &self,
        cfg: DownloaderConfig,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) -> Result<()> {
        let config_id = cfg.id.clone();
        let is_donation_configured =
            cfg.donation_remote_name.is_some() && cfg.donation_remote_path.is_some();

        DownloaderAvailabilityChanged {
            available: false,
            initializing: true,
            error: None,
            config_id: Some(config_id.clone()),
            is_donation_configured,
            needs_setup: false,
        }
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
                            config_id: Some(config_id.clone()),
                            is_donation_configured,
                            needs_setup: false,
                        }
                        .send_signal_to_dart();
                        Ok(())
                    }
                    Err(e) => {
                        DownloaderAvailabilityChanged {
                            available: false,
                            initializing: false,
                            error: Some(format!("Failed to initialize downloader: {:#}", e)),
                            config_id: Some(config_id.clone()),
                            is_donation_configured: false,
                            needs_setup: false,
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
                    config_id: Some(config_id),
                    is_donation_configured: false,
                    needs_setup: false,
                }
                .send_signal_to_dart();
                Err(e)
            }
        }
    }

    async fn finalize_config_install(
        &self,
        app_dir: &Path,
        settings_handler: &Arc<SettingsHandler>,
        result: Result<()>,
    ) {
        match result {
            Ok(()) => {
                DownloaderConfigInstallResult { success: true, error: None }.send_signal_to_dart();

                Toast::send(
                    "Downloader config installed".into(),
                    "Initializing cloud features...".into(),
                    false,
                    None,
                );

                if let Err(e) =
                    self.init_from_disk(app_dir.to_path_buf(), settings_handler.clone()).await
                {
                    error!(
                        error = e.as_ref() as &dyn Error,
                        "Downloader init after config install failed"
                    );
                    DownloaderConfigInstallResult {
                        success: false,
                        error: Some(format!("Failed to initialize downloader: {:#}", e)),
                    }
                    .send_signal_to_dart();
                }
            }
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to install downloader config");
                DownloaderConfigInstallResult { success: false, error: Some(format!("{:#}", e)) }
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

    /// Start handler that watches for InstallDownloaderConfigRequest and installs config.
    fn start_setup_handler(
        self: Arc<Self>,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) {
        // Local file-based install
        tokio::spawn({
            let manager = self.clone();
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            async move {
                let receiver = InstallDownloaderConfigRequest::get_dart_signal_receiver();
                loop {
                    match receiver.recv().await {
                        Some(req) => {
                            let src = PathBuf::from(req.message.source_path);
                            debug!(path = %src.display(), "Received InstallDownloaderConfigRequest");
                            let res = install_config(&app_dir, &src).await;
                            manager.finalize_config_install(&app_dir, &settings_handler, res).await;
                        }
                        None => panic!("InstallDownloaderConfigRequest receiver closed"),
                    }
                }
            }
        });

        // URL-based install
        tokio::spawn({
            let manager = self.clone();
            async move {
                let receiver = InstallDownloaderConfigFromUrlRequest::get_dart_signal_receiver();
                loop {
                    match receiver.recv().await {
                        Some(req) => {
                            let url = req.message.url;
                            debug!(
                                url = %url,
                                "Received InstallDownloaderConfigFromUrlRequest"
                            );

                            let res = async {
                                const BOOTSTRAP_CACHE_KEY: &str = "_bootstrap";
                                let remote_cfg_path =
                                    cache_config_from_url(&app_dir, BOOTSTRAP_CACHE_KEY, &url)
                                        .await?;
                                install_config(&app_dir, &remote_cfg_path).await
                            }
                            .await;

                            manager.finalize_config_install(&app_dir, &settings_handler, res).await;
                        }
                        None => {
                            panic!("InstallDownloaderConfigFromUrlRequest receiver closed")
                        }
                    }
                }
            }
        });
    }
}
fn is_http_url(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.starts_with("http://") || v.starts_with("https://")
}

#[instrument(level = "debug", skip(app_dir), err)]
async fn cache_config_from_url(app_dir: &Path, cache_key: &str, url: &str) -> Result<PathBuf> {
    ensure!(is_http_url(url), "Config update URL must start with http:// or https://");
    debug!(update_url = %url, cache_key = %cache_key, "Downloading downloader config from URL");

    let cache_dir = app_dir.join("downloader_cache").join(cache_key);
    let cached_cfg_path = cache_dir.join("downloader_config.json");

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .context("Failed to build HTTP client for downloader config update")?;

    let _ =
        http_cache::update_file_cached(&client, url, &cached_cfg_path, &cache_dir, None).await?;

    Ok(cached_cfg_path)
}

#[instrument(level = "debug", skip(app_dir), err)]
async fn maybe_update_config_from_url(
    app_dir: &Path,
    config_id: &str,
    update_url: &str,
) -> Result<()> {
    ensure!(!update_url.is_empty(), "Update URL should not be empty");

    let cached_cfg_path = cache_config_from_url(app_dir, config_id, update_url).await?;

    install_config(app_dir, &cached_cfg_path).await?;
    Ok(())
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
    debug!(path = %dst.display(), "Installed downloader.json");
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::{cache_config_from_url, install_config, maybe_update_config_from_url};

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

    #[tokio::test(flavor = "multi_thread")]
    async fn cache_config_from_url_writes_into_cache_dir() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/downloader.json";
        const CONTENTS: &str = "{\"example\":\"ok\"}";

        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_string(CONTENTS))
            .mount(&server)
            .await;

        let url = format!("{}{}", server.uri(), url_path);
        let cache_key = "test-cache";

        let cached_path = cache_config_from_url(dir.path(), cache_key, &url).await.unwrap();

        assert!(cached_path.ends_with("downloader_cache/test-cache/downloader_config.json"));
        assert!(cached_path.exists());
        let content = std::fs::read_to_string(cached_path).unwrap();
        assert_eq!(content, CONTENTS);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn maybe_update_config_from_url_installs_to_app_dir() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path();

        let cfg_json = valid_config_json();

        let server = MockServer::start().await;
        let url_path = "/downloader.json";

        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_string(cfg_json.clone()))
            .mount(&server)
            .await;

        let url = format!("{}{}", server.uri(), url_path);

        let dst = app_dir.join("downloader.json");
        assert!(!dst.exists());

        maybe_update_config_from_url(app_dir, "config-1", &url).await.expect("update from URL");

        assert!(dst.exists());
        let installed = std::fs::read_to_string(dst).unwrap();
        assert_eq!(installed, cfg_json);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn maybe_update_config_from_url_fails_for_empty_url() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path();

        let dst = app_dir.join("downloader.json");
        assert!(!dst.exists());

        let err = maybe_update_config_from_url(app_dir, "config-empty", "").await.unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("Update URL should not be empty"));

        assert!(!dst.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cache_config_from_url_rejects_non_http_url() {
        let dir = tempdir().unwrap();

        let err =
            cache_config_from_url(dir.path(), "cache", "ftp://example.com/file").await.unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("Config update URL must start with http:// or https://"));
    }
}
