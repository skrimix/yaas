use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, ensure};
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock};
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    downloader::{
        self, Downloader,
        config::{DownloaderConfig, RepoLayoutKind},
        http_cache, repo,
    },
    models::{
        InstalledDownloaderConfig, Settings,
        signals::{
            downloader::{
                availability::DownloaderAvailabilityChanged,
                setup::{
                    DownloaderConfigInstallResult, DownloaderSourceRemovedResult,
                    DownloaderSourcesChanged, InstallDownloaderConfigFromUrlRequest,
                    RefreshDownloaderSourcesRequest, RemoveDownloaderSourceRequest,
                    RetryDownloaderInitRequest, SelectDownloaderSourceRequest,
                },
            },
            system::Toast,
        },
    },
    settings::SettingsHandler,
};

const LEGACY_CONFIG_FILENAME: &str = "downloader.json";
const MANAGED_CONFIGS_DIR: &str = "downloader_configs";

#[derive(Clone)]
pub(crate) struct DownloaderManager {
    inner: Arc<RwLock<Option<Arc<Downloader>>>>,
    init_guard: Arc<Mutex<()>>,
}

#[derive(Default)]
struct LoadedManagedConfigs {
    configs: Vec<DownloaderConfig>,
    error: Option<String>,
}

struct ResolvedManagedConfigs {
    configs: Vec<DownloaderConfig>,
    active_config_id: Option<String>,
    error: Option<String>,
}

impl ResolvedManagedConfigs {
    fn load(
        app_dir: &Path,
        settings_handler: &Arc<SettingsHandler>,
        initial_error: Option<String>,
    ) -> Result<Self> {
        let loaded = load_managed_configs(app_dir)?;
        let active_config_id =
            resolve_active_config_id(&loaded.configs, current_active_config_id(settings_handler));

        Ok(Self {
            configs: loaded.configs,
            active_config_id,
            error: combine_errors(initial_error, loaded.error),
        })
    }

    fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }

    fn active_config(&self) -> Option<DownloaderConfig> {
        let active_config_id = self.active_config_id.as_deref()?;
        self.configs.iter().find(|cfg| cfg.id == active_config_id).cloned()
    }

    fn persist_active_config(&self, settings_handler: &Arc<SettingsHandler>) -> Result<()> {
        if self.active_config_id.is_none() {
            return Ok(());
        }

        let current_active_id = current_active_config_id(settings_handler);
        if self.active_config_id.as_deref() == Some(current_active_id.as_str()) {
            return Ok(());
        }

        save_active_config_id(settings_handler, self.active_config_id.as_deref())
    }

    fn send_changed(&self, refreshing: bool) {
        send_configs_changed(
            &self.configs,
            self.active_config_id.as_deref(),
            refreshing,
            self.error.clone(),
        );
    }
}

struct DownloaderAvailabilityReporter {
    config_id: String,
    is_donation_configured: bool,
    supports_remote_selection: bool,
    supports_bandwidth_limit: bool,
    supports_download_mode_selection: bool,
}

impl DownloaderAvailabilityReporter {
    fn new(cfg: &DownloaderConfig, capabilities: repo::RepoCapabilities) -> Self {
        Self {
            config_id: cfg.id.clone(),
            is_donation_configured: capabilities.supports_donation_upload
                && cfg.donation_remote_name.is_some()
                && cfg.donation_remote_path.is_some(),
            supports_remote_selection: capabilities.supports_remote_selection,
            supports_bandwidth_limit: capabilities.supports_bandwidth_limit,
            supports_download_mode_selection: capabilities.supports_download_mode_selection,
        }
    }

    fn signal(&self) -> DownloaderAvailabilityChanged {
        DownloaderAvailabilityChanged {
            config_id: Some(self.config_id.clone()),
            is_donation_configured: self.is_donation_configured,
            supports_remote_selection: self.supports_remote_selection,
            supports_bandwidth_limit: self.supports_bandwidth_limit,
            supports_download_mode_selection: self.supports_download_mode_selection,
            ..Default::default()
        }
    }

    fn send_initializing(&self) {
        let mut signal = self.signal();
        signal.initializing = true;
        signal.send_signal_to_dart();
    }

    fn send_available(&self) {
        let mut signal = self.signal();
        signal.available = true;
        signal.send_signal_to_dart();
    }

    fn send_error(&self, context: &str, error: &anyhow::Error) {
        let mut signal = self.signal();
        signal.error = Some(format!("Failed to {context}: {error:#}"));
        signal.send_signal_to_dart();
    }
}

struct RefreshOutcome {
    refreshed: usize,
    failed: Vec<String>,
}

impl RefreshOutcome {
    fn error_message(&self) -> Option<String> {
        if self.failed.is_empty() {
            None
        } else {
            Some(format!("Failed to refresh some downloader sources: {}", self.failed.join("; ")))
        }
    }
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
        tokio::spawn({
            let manager = self.clone();
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            async move {
                let migration_error = migrate_legacy_config_if_needed(&app_dir, &settings_handler)
                    .await
                    .map(|error| {
                        warn!(
                            error = error.as_ref() as &dyn Error,
                            "Failed to migrate legacy downloader config"
                        );
                        send_error_toast("Failed to migrate legacy downloader config", &error);
                        format!("{error:#}")
                    });

                if let Err(e) = manager
                    .initialize_from_managed(
                        app_dir.clone(),
                        settings_handler.clone(),
                        true,
                        migration_error,
                    )
                    .await
                {
                    error!(error = e.as_ref() as &dyn Error, "Failed to initialize downloader");
                }
            }
        });

        self.clone().start_request_handlers(app_dir.clone(), settings_handler.clone());

        tokio::spawn({
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            let manager = self.clone();
            async move {
                let rx = RetryDownloaderInitRequest::get_dart_signal_receiver();
                while rx.recv().await.is_some() {
                    let _ = manager
                        .initialize_from_managed(
                            app_dir.clone(),
                            settings_handler.clone(),
                            false,
                            None,
                        )
                        .await;
                }
            }
        });
    }

    async fn initialize_from_managed(
        &self,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
        refresh_configs: bool,
        initial_error: Option<String>,
    ) -> Result<()> {
        let _g = self.init_guard.lock().await;

        let loaded_before = load_managed_configs(&app_dir)?;
        let mut state_error = combine_errors(initial_error, loaded_before.error.clone());
        let mut configs_to_refresh_in_background = Vec::new();

        if refresh_configs && !loaded_before.configs.is_empty() {
            let active_id = resolve_active_config_id(
                &loaded_before.configs,
                current_active_config_id(&settings_handler),
            )
            .context("No active downloader config available")?;

            let active_cfg = loaded_before
                .configs
                .iter()
                .find(|cfg| cfg.id == active_id)
                .cloned()
                .context("No active downloader config available")?;
            let refresh_outcome =
                refresh_all_configs(&app_dir, std::slice::from_ref(&active_cfg)).await;
            state_error = combine_errors(state_error, refresh_outcome.error_message());
            configs_to_refresh_in_background =
                loaded_before.configs.into_iter().filter(|cfg| cfg.id != active_id).collect();
        }

        let managed = ResolvedManagedConfigs::load(&app_dir, &settings_handler, state_error)?;

        if managed.is_empty() {
            self.set_downloader(None).await;
            managed.send_changed(false);
            DownloaderAvailabilityChanged { needs_setup: true, ..Default::default() }
                .send_signal_to_dart();
            return Ok(());
        }

        managed.persist_active_config(&settings_handler)?;
        managed.send_changed(false);

        let active_cfg = managed
            .active_config()
            .context("Active downloader config disappeared during initialization")?;
        let startup_error = managed.error.clone();

        self.init_with_config(active_cfg, app_dir.clone(), settings_handler.clone()).await?;

        if !configs_to_refresh_in_background.is_empty() {
            spawn_background_refresh(
                app_dir,
                settings_handler,
                configs_to_refresh_in_background,
                startup_error,
            );
        }

        Ok(())
    }

    pub(crate) async fn init_with_config(
        &self,
        cfg: DownloaderConfig,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) -> Result<()> {
        let repo = repo::make_repo_from_config(&cfg);
        let availability = DownloaderAvailabilityReporter::new(&cfg, repo.capabilities());

        availability.send_initializing();

        self.set_downloader(None).await;

        let cache_dir = app_dir.join("downloader_cache").join(&cfg.id);
        let _ = tokio::fs::create_dir_all(&cache_dir).await;

        let (rclone_path, rclone_config_path) = prepare_downloader_runtime(&cache_dir, &cfg)
            .await
            .inspect_err(|e| availability.send_error("prepare downloader", e))?;

        let downloader = Downloader::new(
            Arc::new(cfg),
            cache_dir,
            rclone_path,
            rclone_config_path,
            settings_handler.clone(),
            WatchStream::new(settings_handler.subscribe()),
        )
        .await
        .inspect_err(|e| availability.send_error("initialize downloader", e))?;

        self.set_downloader(Some(downloader)).await;
        availability.send_available();
        Ok(())
    }

    async fn finalize_config_install(
        &self,
        app_dir: &Path,
        settings_handler: &Arc<SettingsHandler>,
        result: Result<String>,
    ) {
        match result {
            Ok(config_id) => {
                if let Err(e) = self
                    .initialize_from_managed(
                        app_dir.to_path_buf(),
                        settings_handler.clone(),
                        false,
                        None,
                    )
                    .await
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
                    Toast::send(
                        "Failed to add downloader source".into(),
                        format!("Failed to initialize downloader: {:#}", e),
                        true,
                        None,
                    );
                    return;
                }

                DownloaderConfigInstallResult { success: true, error: None }.send_signal_to_dart();
                Toast::send(
                    "Downloader source added".into(),
                    format!("Added source {config_id}"),
                    false,
                    None,
                );
            }
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to install downloader source");
                DownloaderConfigInstallResult { success: false, error: Some(format!("{:#}", e)) }
                    .send_signal_to_dart();
                Toast::send(
                    "Failed to add downloader source".into(),
                    format!("{:#}", e),
                    true,
                    None,
                );
            }
        }
    }

    async fn finalize_source_removal(
        &self,
        app_dir: &Path,
        settings_handler: &Arc<SettingsHandler>,
        config_id: String,
        result: Result<()>,
    ) {
        match result {
            Ok(()) => {
                let init_result = self
                    .initialize_from_managed(
                        app_dir.to_path_buf(),
                        settings_handler.clone(),
                        false,
                        None,
                    )
                    .await;

                let cleanup_result = delete_config_cache_dir(app_dir, &config_id);

                let mut errors = Vec::new();
                if let Err(e) = init_result {
                    error!(
                        error = e.as_ref() as &dyn Error,
                        config_id = %config_id,
                        "Downloader init after source removal failed"
                    );
                    errors.push(format!(
                        "Source removed, but failed to initialize downloader: {:#}",
                        e
                    ));
                }
                if let Err(e) = cleanup_result {
                    error!(
                        error = e.as_ref() as &dyn Error,
                        config_id = %config_id,
                        "Downloader cache cleanup after source removal failed"
                    );
                    errors.push(format!("Source removed, but failed to clean cache: {:#}", e));
                }

                let error = (!errors.is_empty()).then(|| errors.join("\n"));
                let success = error.is_none();

                DownloaderSourceRemovedResult {
                    config_id: config_id.clone(),
                    success,
                    error: error.clone(),
                }
                .send_signal_to_dart();

                match error {
                    Some(error) => {
                        Toast::send("Downloader source removed".into(), error, true, None)
                    }
                    None => Toast::send(
                        "Downloader source removed".into(),
                        format!("Removed source {config_id}"),
                        false,
                        None,
                    ),
                }
            }
            Err(e) => {
                error!(
                    error = e.as_ref() as &dyn Error,
                    config_id = %config_id,
                    "Failed to remove downloader source"
                );
                DownloaderSourceRemovedResult {
                    config_id: config_id.clone(),
                    success: false,
                    error: Some(format!("{:#}", e)),
                }
                .send_signal_to_dart();
                Toast::send(
                    "Failed to remove downloader source".into(),
                    format!("{:#}", e),
                    true,
                    None,
                );
            }
        }
    }

    fn start_request_handlers(
        self: Arc<Self>,
        app_dir: PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) {
        tokio::spawn({
            let manager = self.clone();
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            async move {
                let receiver = InstallDownloaderConfigFromUrlRequest::get_dart_signal_receiver();
                loop {
                    match receiver.recv().await {
                        Some(req) => {
                            let url = req.message.url.trim().to_string();
                            debug!(url = %url, "Received InstallDownloaderConfigFromUrlRequest");

                            let result = async {
                                let cfg =
                                    add_config_from_url(&app_dir, &settings_handler, &url, true)
                                        .await?;
                                Ok(cfg.id)
                            }
                            .await;

                            manager
                                .finalize_config_install(&app_dir, &settings_handler, result)
                                .await;
                        }
                        None => {
                            panic!("InstallDownloaderConfigFromUrlRequest receiver closed")
                        }
                    }
                }
            }
        });

        tokio::spawn({
            let manager = self.clone();
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            async move {
                let receiver = RemoveDownloaderSourceRequest::get_dart_signal_receiver();
                loop {
                    match receiver.recv().await {
                        Some(req) => {
                            let config_id = req.message.config_id.trim().to_string();
                            debug!(config_id = %config_id, "Received RemoveDownloaderSourceRequest");

                            let result =
                                delete_managed_config(&settings_handler, &app_dir, &config_id);

                            manager
                                .finalize_source_removal(
                                    &app_dir,
                                    &settings_handler,
                                    config_id,
                                    result,
                                )
                                .await;
                        }
                        None => panic!("RemoveDownloaderSourceRequest receiver closed"),
                    }
                }
            }
        });

        tokio::spawn({
            let manager = self.clone();
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            async move {
                let receiver = SelectDownloaderSourceRequest::get_dart_signal_receiver();
                loop {
                    match receiver.recv().await {
                        Some(req) => {
                            let config_id = req.message.config_id.trim().to_string();
                            let result =
                                select_active_config(&settings_handler, &app_dir, &config_id);
                            if let Err(e) = result {
                                send_error_toast("Failed to switch downloader source", &e);
                                continue;
                            }
                            if let Err(e) = manager
                                .initialize_from_managed(
                                    app_dir.clone(),
                                    settings_handler.clone(),
                                    false,
                                    None,
                                )
                                .await
                            {
                                send_error_toast("Failed to switch downloader source", &e);
                            }
                        }
                        None => panic!("SelectDownloaderSourceRequest receiver closed"),
                    }
                }
            }
        });

        tokio::spawn({
            let manager = self.clone();
            let app_dir = app_dir.clone();
            let settings_handler = settings_handler.clone();
            async move {
                let receiver = RefreshDownloaderSourcesRequest::get_dart_signal_receiver();
                loop {
                    match receiver.recv().await {
                        Some(_) => {
                            let loaded = match load_managed_configs(&app_dir) {
                                Ok(loaded) => loaded,
                                Err(e) => {
                                    send_error_toast("Failed to refresh downloader sources", &e);
                                    continue;
                                }
                            };

                            let active_id = resolve_active_config_id(
                                &loaded.configs,
                                current_active_config_id(&settings_handler),
                            );
                            send_configs_changed(
                                &loaded.configs,
                                active_id.as_deref(),
                                true,
                                loaded.error.clone(),
                            );

                            let outcome = refresh_all_configs(&app_dir, &loaded.configs).await;
                            if outcome.failed.is_empty() {
                                Toast::send(
                                    "Downloader sources refreshed".into(),
                                    format!("Updated {} source(s)", outcome.refreshed),
                                    false,
                                    None,
                                );
                            } else {
                                Toast::send(
                                    "Downloader source refresh completed".into(),
                                    format!(
                                        "Updated {} source(s), {} failed",
                                        outcome.refreshed,
                                        outcome.failed.len()
                                    ),
                                    true,
                                    None,
                                );
                            }

                            if let Err(e) = manager
                                .initialize_from_managed(
                                    app_dir.clone(),
                                    settings_handler.clone(),
                                    false,
                                    outcome.error_message(),
                                )
                                .await
                            {
                                send_error_toast("Failed to refresh downloader sources", &e);
                            }
                        }
                        None => panic!("RefreshDownloaderSourcesRequest receiver closed"),
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

fn managed_configs_dir(app_dir: &Path) -> PathBuf {
    app_dir.join(MANAGED_CONFIGS_DIR)
}

fn managed_config_path(app_dir: &Path, config_id: &str) -> PathBuf {
    managed_configs_dir(app_dir).join(format!("{config_id}.json"))
}

fn config_download_cache_path(app_dir: &Path, cache_key: &str) -> (PathBuf, PathBuf) {
    let cache_dir = app_dir.join("downloader_cache").join(cache_key);
    let cached_cfg_path = cache_dir.join("downloader_config.json");
    (cache_dir, cached_cfg_path)
}

fn current_settings(settings_handler: &Arc<SettingsHandler>) -> Settings {
    let rx = settings_handler.subscribe();
    rx.borrow().clone()
}

fn current_active_config_id(settings_handler: &Arc<SettingsHandler>) -> String {
    current_settings(settings_handler).active_downloader_config_id.trim().to_string()
}

fn save_active_config_id(
    settings_handler: &Arc<SettingsHandler>,
    config_id: Option<&str>,
) -> Result<()> {
    let mut settings = current_settings(settings_handler);
    let new_id = config_id.unwrap_or_default().to_string();
    if settings.active_downloader_config_id == new_id {
        return Ok(());
    }

    settings.active_downloader_config_id = new_id;
    settings_handler.save_settings(&settings)
}

fn send_error_toast(title: &str, error: &impl std::fmt::Display) {
    Toast::send(title.into(), format!("{error:#}"), true, None);
}

fn resolve_active_config_id(configs: &[DownloaderConfig], desired_id: String) -> Option<String> {
    if !desired_id.is_empty() && configs.iter().any(|cfg| cfg.id == desired_id) {
        return Some(desired_id);
    }

    configs.iter().min_by(|left, right| left.id.cmp(&right.id)).map(|cfg| cfg.id.clone())
}

fn combine_errors(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (None, None) => None,
        (Some(error), None) | (None, Some(error)) => Some(error),
        (Some(first), Some(second)) => Some(format!("{first}\n{second}")),
    }
}

fn send_configs_changed(
    configs: &[DownloaderConfig],
    active_config_id: Option<&str>,
    refreshing: bool,
    error: Option<String>,
) {
    DownloaderSourcesChanged {
        configs: configs
            .iter()
            .map(|cfg| InstalledDownloaderConfig {
                id: cfg.id.clone(),
                display_name: cfg.effective_display_name(),
                description: cfg.effective_description(),
            })
            .collect(),
        active_config_id: active_config_id.map(ToOwned::to_owned),
        refreshing,
        error,
    }
    .send_signal_to_dart();
}

fn load_managed_configs(app_dir: &Path) -> Result<LoadedManagedConfigs> {
    let dir = managed_configs_dir(app_dir);
    if !dir.exists() {
        return Ok(LoadedManagedConfigs::default());
    }

    let mut configs = Vec::new();
    let mut ignored = Vec::new();

    for entry in fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("Failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        match DownloaderConfig::load_from_path(&path).and_then(|cfg| {
            cfg.validate_managed_remote(None)?;
            Ok(cfg)
        }) {
            Ok(cfg) => configs.push(cfg),
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    path = %path.display(),
                    "Ignoring invalid managed downloader config"
                );
                ignored.push(format!(
                    "{}: {:#}",
                    path.file_name().and_then(|value| value.to_str()).unwrap_or("unknown"),
                    e
                ));
            }
        }
    }

    configs.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(LoadedManagedConfigs {
        configs,
        error: (!ignored.is_empty())
            .then(|| format!("Ignored invalid downloader sources: {}", ignored.join("; "))),
    })
}

#[instrument(level = "debug", skip(app_dir), err)]
async fn cache_config_from_url(app_dir: &Path, cache_key: &str, url: &str) -> Result<PathBuf> {
    ensure!(is_http_url(url), "Config update URL must start with http:// or https://");
    debug!(update_url = %url, cache_key = %cache_key, "Downloading downloader config from URL");

    let (cache_dir, cached_cfg_path) = config_download_cache_path(app_dir, cache_key);

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .context("Failed to build HTTP client for downloader config update")?;

    let _ =
        http_cache::update_file_cached(&client, url, &cached_cfg_path, &cache_dir, None).await?;

    Ok(cached_cfg_path)
}

async fn fetch_managed_config(
    app_dir: &Path,
    cache_key: &str,
    url: &str,
    source_url: Option<&str>,
    expected_id: Option<&str>,
    refuse_existing: bool,
) -> Result<DownloaderConfig> {
    let remote_cfg_path = cache_config_from_url(app_dir, cache_key, url).await?;
    write_managed_config(app_dir, &remote_cfg_path, source_url, expected_id, refuse_existing)
}

async fn prepare_downloader_runtime(
    cache_dir: &Path,
    cfg: &DownloaderConfig,
) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    match cfg.layout {
        RepoLayoutKind::Ffa => downloader::rclone::prepare_rclone_files(cache_dir, cfg)
            .await
            .map(|(rclone_path, rclone_config_path)| (Some(rclone_path), Some(rclone_config_path))),
        RepoLayoutKind::NewRepo => Ok((None, None)),
    }
}

fn write_managed_config(
    app_dir: &Path,
    src: &Path,
    source_url: Option<&str>,
    expected_id: Option<&str>,
    refuse_existing: bool,
) -> Result<DownloaderConfig> {
    let cfg = DownloaderConfig::load_from_path(src)?;
    cfg.validate_managed_remote(source_url)?;
    if let Some(expected_id) = expected_id {
        ensure!(
            cfg.id == expected_id,
            "Downloaded downloader config changed ID: expected {expected_id}, got {}",
            cfg.id
        );
    }

    let dst_dir = managed_configs_dir(app_dir);
    fs::create_dir_all(&dst_dir)
        .with_context(|| format!("Failed to create {}", dst_dir.display()))?;

    let dst = managed_config_path(app_dir, &cfg.id);
    if refuse_existing {
        ensure!(!dst.exists(), "Downloader config ID already installed: {}", cfg.id);
    }

    let tmp = dst_dir.join(format!("{}.json.tmp", cfg.id));
    let content =
        fs::read_to_string(src).with_context(|| format!("Failed to read {}", src.display()))?;
    fs::write(&tmp, content).with_context(|| format!("Failed to write {}", tmp.display()))?;
    fs::rename(&tmp, &dst).with_context(|| format!("Failed to replace {}", dst.display()))?;

    Ok(cfg)
}

async fn add_config_from_url(
    app_dir: &Path,
    settings_handler: &Arc<SettingsHandler>,
    url: &str,
    select_as_active: bool,
) -> Result<DownloaderConfig> {
    let cfg = fetch_managed_config(app_dir, "_bootstrap", url, Some(url), None, true).await?;
    if select_as_active {
        save_active_config_id(settings_handler, Some(&cfg.id))?;
    }
    Ok(cfg)
}

async fn refresh_all_configs(app_dir: &Path, configs: &[DownloaderConfig]) -> RefreshOutcome {
    let mut outcome = RefreshOutcome { refreshed: 0, failed: Vec::new() };

    for cfg in configs {
        let Some(update_url) = cfg.config_update_url.as_deref().map(str::trim) else {
            outcome.failed.push(format!("{}: missing config_update_url", cfg.id));
            continue;
        };

        let refresh_result = async {
            let _ = fetch_managed_config(app_dir, &cfg.id, update_url, None, Some(&cfg.id), false)
                .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match refresh_result {
            Ok(()) => outcome.refreshed += 1,
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    config_id = %cfg.id,
                    "Failed to refresh downloader config"
                );
                outcome.failed.push(format!("{}: {:#}", cfg.id, e));
            }
        }
    }

    outcome
}

fn select_active_config(
    settings_handler: &Arc<SettingsHandler>,
    app_dir: &Path,
    config_id: &str,
) -> Result<()> {
    ensure!(!config_id.is_empty(), "Downloader config ID must not be empty");

    let loaded = load_managed_configs(app_dir)?;
    ensure!(
        loaded.configs.iter().any(|cfg| cfg.id == config_id),
        "Downloader config is not installed: {config_id}"
    );

    save_active_config_id(settings_handler, Some(config_id))
}

fn delete_managed_config(
    settings_handler: &Arc<SettingsHandler>,
    app_dir: &Path,
    config_id: &str,
) -> Result<()> {
    ensure!(!config_id.is_empty(), "Downloader config ID must not be empty");

    let path = managed_config_path(app_dir, config_id);
    ensure!(path.exists(), "Downloader config is not installed: {config_id}");

    fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))?;

    let loaded = load_managed_configs(app_dir)?;
    let next_active_id =
        resolve_active_config_id(&loaded.configs, current_active_config_id(settings_handler));
    save_active_config_id(settings_handler, next_active_id.as_deref())
}

fn delete_config_cache_dir(app_dir: &Path, config_id: &str) -> Result<()> {
    let cache_dir = app_dir.join("downloader_cache").join(config_id);
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir)
            .with_context(|| format!("Failed to delete {}", cache_dir.display()))?;
    }

    Ok(())
}

fn spawn_background_refresh(
    app_dir: PathBuf,
    settings_handler: Arc<SettingsHandler>,
    configs: Vec<DownloaderConfig>,
    startup_error: Option<String>,
) {
    tokio::spawn(async move {
        let refresh_outcome = refresh_all_configs(&app_dir, &configs).await;
        let state_error = combine_errors(startup_error, refresh_outcome.error_message());

        let managed = match ResolvedManagedConfigs::load(&app_dir, &settings_handler, state_error) {
            Ok(managed) => managed,
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    "Failed to reload downloader sources after background refresh"
                );
                return;
            }
        };

        managed.send_changed(false);
    });
}

async fn migrate_legacy_config_if_needed(
    app_dir: &Path,
    settings_handler: &Arc<SettingsHandler>,
) -> Option<anyhow::Error> {
    let legacy_path = app_dir.join(LEGACY_CONFIG_FILENAME);
    if !legacy_path.exists() {
        return None;
    }

    info!(path = %legacy_path.display(), "Migrating legacy downloader config");

    let migration_result = async {
        let legacy_cfg = DownloaderConfig::load_from_path(&legacy_path)?;
        let update_url = legacy_cfg
            .config_update_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("Legacy downloader config has no usable config_update_url")?;

        let select_as_active = current_active_config_id(settings_handler).is_empty();

        if managed_config_path(app_dir, &legacy_cfg.id).exists() {
            if select_as_active {
                save_active_config_id(settings_handler, Some(&legacy_cfg.id))?;
            }
            return Ok::<(), anyhow::Error>(());
        }

        let _ =
            add_config_from_url(app_dir, settings_handler, update_url, select_as_active).await?;
        Ok(())
    }
    .await;

    match migration_result {
        Ok(()) => {
            if let Err(e) = fs::remove_file(&legacy_path) {
                warn!(
                    error = &e as &dyn Error,
                    path = %legacy_path.display(),
                    "Failed to delete legacy downloader config"
                );
                return Some(anyhow!("Failed to delete legacy downloader config: {e}"));
            }
            None
        }
        Err(e) => Some(e),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    fn managed_config_json(id: &str, update_url: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "display_name": "Display {id}",
                "description": "Description {id}",
                "layout": "ffa",
                "rclone_path": "/bin/echo",
                "rclone_config_path": "/tmp/rclone.conf",
                "config_update_url": "{update_url}"
            }}"#
        )
    }

    fn legacy_config_json_without_update_url(id: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "layout": "ffa",
                "rclone_path": "/bin/echo",
                "rclone_config_path": "/tmp/rclone.conf"
            }}"#
        )
    }

    #[test]
    fn write_managed_config_requires_matching_update_url() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, managed_config_json("test", "https://example.com/downloader.json"))
            .unwrap();

        let err = write_managed_config(
            dir.path(),
            &src,
            Some("https://other.example/config.json"),
            None,
            true,
        )
        .unwrap_err();
        assert!(format!("{:#}", err).contains("Config update URL mismatch"));
    }

    #[test]
    fn write_managed_config_rejects_duplicate_id() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, managed_config_json("test", "https://example.com/downloader.json"))
            .unwrap();

        let first = write_managed_config(
            dir.path(),
            &src,
            Some("https://example.com/downloader.json"),
            None,
            true,
        )
        .unwrap();
        assert_eq!(first.id, "test");

        let err = write_managed_config(
            dir.path(),
            &src,
            Some("https://example.com/downloader.json"),
            None,
            true,
        )
        .unwrap_err();
        assert!(format!("{:#}", err).contains("Downloader config ID already installed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_all_configs_allows_update_url_change() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path();
        let managed_dir = managed_configs_dir(app_dir);
        std::fs::create_dir_all(&managed_dir).unwrap();

        let server = MockServer::start().await;
        let original_url = format!("{}/downloader.json", server.uri());
        let installed_path = managed_config_path(app_dir, "test");
        std::fs::write(&installed_path, managed_config_json("test", &original_url)).unwrap();

        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(managed_config_json(
                "test",
                "https://other.example/downloader.json",
            )))
            .mount(&server)
            .await;

        let cfg = DownloaderConfig::load_from_path(&installed_path).expect("load installed config");
        let outcome = refresh_all_configs(app_dir, &[cfg]).await;

        assert_eq!(outcome.refreshed, 1);
        assert!(outcome.failed.is_empty());
        let installed = std::fs::read_to_string(&installed_path).unwrap();
        assert!(installed.contains("https://other.example/downloader.json"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn migrate_legacy_config_imports_remote_and_deletes_file() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();

        let server = MockServer::start().await;
        let url = format!("{}/downloader.json", server.uri());
        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(managed_config_json("legacy", &url)),
            )
            .mount(&server)
            .await;

        let legacy_path = app_dir.join(LEGACY_CONFIG_FILENAME);
        std::fs::write(&legacy_path, managed_config_json("legacy", &url)).unwrap();

        let warning = migrate_legacy_config_if_needed(&app_dir, &settings).await;
        assert!(warning.is_none());
        assert!(!legacy_path.exists());
        assert!(managed_config_path(&app_dir, "legacy").exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn migrate_legacy_without_update_url_keeps_file() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        let legacy_path = app_dir.join(LEGACY_CONFIG_FILENAME);
        std::fs::write(&legacy_path, legacy_config_json_without_update_url("legacy")).unwrap();

        let warning = migrate_legacy_config_if_needed(&app_dir, &settings).await;
        assert!(warning.is_some());
        assert!(legacy_path.exists());
        assert!(!managed_config_path(&app_dir, "legacy").exists());
    }

    #[test]
    fn resolve_active_config_id_falls_back_to_first_sorted_config() {
        let configs = vec![
            DownloaderConfig {
                id: "b".into(),
                display_name: None,
                description: None,
                rclone_path: Some(crate::downloader::config::RclonePath::Single(
                    "/bin/echo".into(),
                )),
                rclone_config_path: Some("/tmp/rclone.conf".into()),
                remote_name_filter_regex: None,
                disable_randomize_remote: false,
                donation_remote_name: None,
                donation_remote_path: None,
                donation_blacklist_path: None,
                layout: RepoLayoutKind::Ffa,
                base_url: None,
                root_dir: "Quest Games".into(),
                list_path: "FFA.txt".into(),
                config_update_url: Some("https://example.com/b.json".into()),
            },
            DownloaderConfig {
                id: "a".into(),
                display_name: None,
                description: None,
                rclone_path: Some(crate::downloader::config::RclonePath::Single(
                    "/bin/echo".into(),
                )),
                rclone_config_path: Some("/tmp/rclone.conf".into()),
                remote_name_filter_regex: None,
                disable_randomize_remote: false,
                donation_remote_name: None,
                donation_remote_path: None,
                donation_blacklist_path: None,
                layout: RepoLayoutKind::Ffa,
                base_url: None,
                root_dir: "Quest Games".into(),
                list_path: "FFA.txt".into(),
                config_update_url: Some("https://example.com/a.json".into()),
            },
        ];

        assert_eq!(resolve_active_config_id(&configs, String::new()).as_deref(), Some("a"));
        assert_eq!(resolve_active_config_id(&configs, "missing".into()).as_deref(), Some("a"));
        assert_eq!(resolve_active_config_id(&configs, "a".into()).as_deref(), Some("a"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_reassigns_active_source() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();

        let alpha = managed_config_path(&app_dir, "alpha");
        let beta = managed_config_path(&app_dir, "beta");
        std::fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        std::fs::write(&alpha, managed_config_json("alpha", "https://example.com/alpha.json"))
            .unwrap();
        std::fs::write(&beta, managed_config_json("beta", "https://example.com/beta.json"))
            .unwrap();

        save_active_config_id(&settings, Some("beta")).unwrap();

        delete_managed_config(&settings, &app_dir, "beta").unwrap();

        assert!(!beta.exists());
        assert!(alpha.exists());
        assert_eq!(current_active_config_id(&settings), "alpha");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_clears_active_source_when_last_removed() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();

        let only = managed_config_path(&app_dir, "only");
        std::fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        std::fs::write(&only, managed_config_json("only", "https://example.com/only.json"))
            .unwrap();

        save_active_config_id(&settings, Some("only")).unwrap();

        delete_managed_config(&settings, &app_dir, "only").unwrap();

        assert!(!only.exists());
        assert_eq!(current_active_config_id(&settings), "");
    }

    #[test]
    fn delete_config_cache_dir_removes_runtime_directory() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let cache_dir = app_dir.join("downloader_cache").join("test");

        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("downloader_config.json"), "cached").unwrap();

        delete_config_cache_dir(&app_dir, "test").unwrap();

        assert!(!cache_dir.exists());
    }
}
