use std::{error::Error, sync::Arc};

use anyhow::{Context, Result};
use rinf::{DartSignal, RustSignal};
use tokio::sync::Mutex;
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, warn};

use crate::{
    downloader::{
        DownloaderSession, SensitiveUrl,
        config::DownloaderConfig,
        manager::DownloaderManager,
        repo,
        sources::{
            RefreshReport, SourceSnapshot, SourceStore, runtime_cache_dir, warnings_to_message,
        },
    },
    models::signals::{
        downloader::{
            availability::{DownloaderAvailabilityChanged, RepoCapabilities},
            setup::{
                DownloaderConfigInstallResult, DownloaderSourceRemovedResult,
                DownloaderSourcesChanged, InstallDownloaderConfigFromUrlRequest,
                RefreshDownloaderSourcesRequest, RemoveDownloaderSourceRequest,
                RetryDownloaderInitRequest, SelectDownloaderSourceRequest,
            },
        },
        system::Toast,
    },
    settings::SettingsHandler,
};

#[derive(Clone)]
pub(crate) struct DownloaderController {
    manager: Arc<DownloaderManager>,
    sources: SourceStore,
    settings_handler: Arc<SettingsHandler>,
    reconcile_guard: Arc<Mutex<()>>,
}

struct DownloaderAvailabilityReporter {
    config_id: String,
    is_donation_configured: bool,
    capabilities: RepoCapabilities,
}

impl DownloaderController {
    pub(crate) fn new(
        manager: Arc<DownloaderManager>,
        app_dir: std::path::PathBuf,
        settings_handler: Arc<SettingsHandler>,
    ) -> Arc<Self> {
        Arc::new(Self {
            manager,
            sources: SourceStore::new(app_dir, settings_handler.clone()),
            settings_handler,
            reconcile_guard: Arc::new(Mutex::new(())),
        })
    }

    pub(crate) fn start(self: Arc<Self>) {
        tokio::spawn({
            let controller = self.clone();
            async move { controller.startup().await }
        });

        self.start_request_handlers();
    }

    /// Startup pipeline:
    /// 1. migrate the legacy single-config file into managed sources,
    /// 2. best-effort network refresh of the active source, so the first
    ///    session starts with the freshest config,
    /// 3. sync disk state with runtime and UI (starts the session),
    /// 4. refresh the remaining sources in the background.
    async fn startup(self: Arc<Self>) {
        let mut warnings = self.migrate_legacy_config().await;
        warnings.extend(self.refresh_active_source().await);

        match self.sync("startup", warnings).await {
            Ok(sources) => {
                let inactive_configs = self.sources.inactive_configs(&sources);
                if !inactive_configs.is_empty() {
                    self.clone().spawn_background_refresh(inactive_configs);
                }
            }
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to initialize downloader");
            }
        }
    }

    async fn migrate_legacy_config(&self) -> Vec<String> {
        let Some(error) = self.sources.migrate_legacy_config_if_needed().await else {
            return Vec::new();
        };
        warn!(error = error.as_ref() as &dyn Error, "Failed to migrate legacy downloader config");
        send_error_toast("Failed to migrate legacy downloader config", &error);
        vec![format!("{error:#}")]
    }

    async fn refresh_active_source(&self) -> Vec<String> {
        match self.sources.refresh_active().await {
            Ok(report) => report.warning_message().into_iter().collect(),
            Err(e) => {
                debug!(
                    error = e.as_ref() as &dyn Error,
                    "Skipping active source refresh during startup"
                );
                Vec::new()
            }
        }
    }

    /// Reloads sources from disk, reconciles and persists the active config,
    /// emits UI signals and (re)starts the downloader session for the active
    /// config. Every source mutation must be followed by a sync.
    async fn sync(
        &self,
        reason: &'static str,
        extra_warnings: Vec<String>,
    ) -> Result<SourceSnapshot> {
        let _guard = self.reconcile_guard.lock().await;
        debug!(reason, "Syncing downloader sources");

        let sources = self.sources.load()?;
        self.sources.persist_active_config(&sources)?;
        send_sources_changed(&sources, false, &extra_warnings);

        match sources.active_config() {
            Some(active_cfg) => self.start_session(active_cfg).await?,
            None => {
                self.manager.clear().await;
                DownloaderAvailabilityChanged { needs_setup: true, ..Default::default() }
                    .send_signal_to_dart();
            }
        }

        Ok(sources)
    }

    /// Starts a new session for the given config, replacing any running one.
    /// Reports progress to the UI via availability signals.
    async fn start_session(&self, cfg: DownloaderConfig) -> Result<()> {
        let repo = repo::make_repo_from_config(&cfg);
        let availability = DownloaderAvailabilityReporter::new(&cfg, repo.capabilities());

        availability.send_initializing();
        self.manager.clear().await;

        let cache_dir = runtime_cache_dir(self.sources.app_dir(), &cfg.id);
        let _ = tokio::fs::create_dir_all(&cache_dir).await;

        let runtime_files = repo
            .prepare_runtime(&cache_dir, &cfg)
            .await
            .inspect_err(|e| availability.send_error("prepare downloader", e))?;

        let session = DownloaderSession::new(
            Arc::new(cfg),
            repo,
            cache_dir,
            runtime_files,
            self.settings_handler.clone(),
            WatchStream::new(self.settings_handler.subscribe()),
        )
        .await
        .inspect_err(|e| availability.send_error("initialize downloader", e))?;

        self.manager.replace(session).await;
        availability.send_available();
        Ok(())
    }

    async fn install_from_url(&self, url: SensitiveUrl<'_>) {
        let outcome = async {
            let config_id = self.sources.install_from_url(url, true).await?.id;
            self.sync("install", Vec::new()).await.context("Failed to initialize downloader")?;
            Ok::<_, anyhow::Error>(config_id)
        }
        .await;

        match outcome {
            Ok(config_id) => {
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
                let message = format!("{:#}", e);
                DownloaderConfigInstallResult { success: false, error: Some(message.clone()) }
                    .send_signal_to_dart();
                Toast::send("Failed to add downloader source".into(), message, true, None);
            }
        }
    }

    async fn remove_source(&self, config_id: String) {
        if let Err(e) = self.sources.remove(&config_id) {
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
            return;
        }

        let mut errors = Vec::new();
        if let Err(e) = self.sync("remove", Vec::new()).await {
            error!(
                error = e.as_ref() as &dyn Error,
                config_id = %config_id,
                "Downloader init after source removal failed"
            );
            errors.push(format!("Source removed, but failed to initialize downloader: {:#}", e));
        }
        if let Err(e) = self.sources.delete_cache_dir(&config_id) {
            error!(
                error = e.as_ref() as &dyn Error,
                config_id = %config_id,
                "Downloader cache cleanup after source removal failed"
            );
            errors.push(format!("Source removed, but failed to clean cache: {:#}", e));
        }

        let error = (!errors.is_empty()).then(|| errors.join("\n"));

        DownloaderSourceRemovedResult {
            config_id: config_id.clone(),
            success: error.is_none(),
            error: error.clone(),
        }
        .send_signal_to_dart();

        match error {
            Some(error) => Toast::send("Downloader source removed".into(), error, true, None),
            None => Toast::send(
                "Downloader source removed".into(),
                format!("Removed source {config_id}"),
                false,
                None,
            ),
        }
    }

    async fn select_source(&self, config_id: &str) {
        let result = async {
            self.sources.select_active(config_id)?;
            self.sync("select", Vec::new()).await
        }
        .await;

        if let Err(e) = result {
            send_error_toast("Failed to switch downloader source", &e);
        }
    }

    async fn manual_refresh(&self) {
        let loaded = match self.sources.load() {
            Ok(sources) => sources,
            Err(e) => {
                send_error_toast("Failed to refresh downloader sources", &e);
                return;
            }
        };

        send_sources_changed(&loaded, true, &[]);

        let report = self.sources.refresh_all(&loaded.configs).await;
        send_refresh_complete_toast(&report);

        let warnings = report.warning_message().into_iter().collect();
        if let Err(e) = self.sync("manual refresh", warnings).await {
            send_error_toast("Failed to refresh downloader sources", &e);
        }
    }

    fn spawn_background_refresh(self: Arc<Self>, configs: Vec<DownloaderConfig>) {
        tokio::spawn(async move {
            let report = self.sources.refresh_all(&configs).await;
            let warnings: Vec<_> = report.warning_message().into_iter().collect();

            match self.sources.load() {
                Ok(sources) => send_sources_changed(&sources, false, &warnings),
                Err(e) => {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Failed to reload downloader sources after background refresh"
                    );
                }
            }
        });
    }

    fn start_request_handlers(self: Arc<Self>) {
        self.spawn_request_handler(
            |controller, req: InstallDownloaderConfigFromUrlRequest| async move {
                let raw_url = req.url.trim().to_string();
                let url = SensitiveUrl::new(&raw_url);
                debug!(url = %url, "Received InstallDownloaderConfigFromUrlRequest");
                controller.install_from_url(url).await;
            },
        );

        self.spawn_request_handler(|controller, req: RemoveDownloaderSourceRequest| async move {
            let config_id = req.config_id.trim().to_string();
            debug!(config_id = %config_id, "Received RemoveDownloaderSourceRequest");
            controller.remove_source(config_id).await;
        });

        self.spawn_request_handler(|controller, req: SelectDownloaderSourceRequest| async move {
            controller.select_source(req.config_id.trim()).await;
        });

        self.spawn_request_handler(
            |controller, _req: RefreshDownloaderSourcesRequest| async move {
                controller.manual_refresh().await;
            },
        );

        self.spawn_request_handler(|controller, _req: RetryDownloaderInitRequest| async move {
            if let Err(e) = controller.sync("retry", Vec::new()).await {
                send_error_toast("Failed to initialize downloader", &e);
            }
        });
    }

    /// Spawns a task dispatching Dart requests of type `S` to `handler`.
    fn spawn_request_handler<S, F, Fut>(self: &Arc<Self>, handler: F)
    where
        S: DartSignal + Send + 'static,
        F: Fn(Arc<Self>, S) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let controller = self.clone();
        tokio::spawn(async move {
            let receiver = S::get_dart_signal_receiver();
            while let Some(req) = receiver.recv().await {
                handler(controller.clone(), req.message).await;
            }

            panic!("{} receiver closed", std::any::type_name::<S>())
        });
    }
}

impl DownloaderAvailabilityReporter {
    fn new(cfg: &DownloaderConfig, capabilities: RepoCapabilities) -> Self {
        Self {
            config_id: cfg.id.clone(),
            is_donation_configured: capabilities.supports_donation_upload
                && cfg.donation_remote_name.is_some()
                && cfg.donation_remote_path.is_some(),
            capabilities,
        }
    }

    fn signal(&self) -> DownloaderAvailabilityChanged {
        DownloaderAvailabilityChanged {
            config_id: Some(self.config_id.clone()),
            is_donation_configured: self.is_donation_configured,
            capabilities: self.capabilities,
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

fn send_sources_changed(sources: &SourceSnapshot, refreshing: bool, extra_warnings: &[String]) {
    let mut warnings = sources.warnings.clone();
    warnings.extend(extra_warnings.iter().cloned());
    DownloaderSourcesChanged {
        configs: sources.installed_configs(),
        active_config_id: sources.active_config_id.clone(),
        refreshing,
        error: warnings_to_message(&warnings),
    }
    .send_signal_to_dart();
}

fn send_error_toast(title: &str, error: &impl std::fmt::Display) {
    Toast::send(title.into(), format!("{error:#}"), true, None);
}

fn send_refresh_complete_toast(report: &RefreshReport) {
    if report.failed.is_empty() {
        Toast::send(
            "Downloader sources refreshed".into(),
            format!("Updated {} source(s)", report.refreshed),
            false,
            None,
        );
    } else {
        Toast::send(
            "Downloader source refresh completed".into(),
            format!("Updated {} source(s), {} failed", report.refreshed, report.failed.len()),
            true,
            None,
        );
    }
}
