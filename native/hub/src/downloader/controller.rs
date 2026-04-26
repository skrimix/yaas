use std::{error::Error, path::Path, sync::Arc};

use anyhow::{Context, Result};
use rinf::{DartSignal, RustSignal};
use tokio::sync::Mutex;
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, warn};

use crate::{
    downloader::{
        Downloader,
        config::{DownloaderConfig, RepoLayoutKind},
        manager::DownloaderManager,
        repo,
        sources::{DownloaderSources, LoadedSources, RefreshReport, runtime_cache_dir},
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
    sources: DownloaderSources,
    settings_handler: Arc<SettingsHandler>,
    reload_guard: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Copy)]
enum ReloadReason {
    Startup,
    Retry,
    Install,
    Remove,
    Select,
    ManualRefresh,
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
            sources: DownloaderSources::new(app_dir, settings_handler.clone()),
            settings_handler,
            reload_guard: Arc::new(Mutex::new(())),
        })
    }

    pub(crate) fn start(self: Arc<Self>) {
        tokio::spawn({
            let controller = self.clone();
            async move { controller.startup().await }
        });

        self.start_request_handlers();
    }

    async fn startup(self: Arc<Self>) {
        let migration_warning = self.sources.migrate_legacy_config_if_needed().await.map(|error| {
            warn!(
                error = error.as_ref() as &dyn Error,
                "Failed to migrate legacy downloader config"
            );
            send_error_toast("Failed to migrate legacy downloader config", &error);
            format!("{error:#}")
        });

        let mut warnings = migration_warning.into_iter().collect::<Vec<_>>();

        let initial_sources = match self.sources.load(warnings.clone()) {
            Ok(sources) => sources,
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to load downloader sources");
                return;
            }
        };

        if !initial_sources.is_empty() {
            let report = self.sources.refresh_active(&initial_sources).await;
            if let Some(warning) = report.warning_message() {
                warnings.push(warning);
            }
        }

        let inactive_configs = match self.reload_and_apply(ReloadReason::Startup, warnings).await {
            Ok(sources) => self.sources.inactive_configs(&sources),
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to initialize downloader");
                return;
            }
        };

        if !inactive_configs.is_empty() {
            self.clone().spawn_background_refresh(inactive_configs);
        }
    }

    async fn reload_and_apply(
        &self,
        reason: ReloadReason,
        extra_warnings: Vec<String>,
    ) -> Result<LoadedSources> {
        let _guard = self.reload_guard.lock().await;
        debug!(?reason, "Reloading downloader sources");

        let sources = self.sources.load(extra_warnings)?;
        self.sources.persist_active_config(&sources)?;
        send_sources_changed(&sources, false);

        if sources.is_empty() {
            self.manager.clear().await;
            DownloaderAvailabilityChanged { needs_setup: true, ..Default::default() }
                .send_signal_to_dart();
            return Ok(sources);
        }

        let active_cfg = sources
            .active_config()
            .context("Active downloader config disappeared during reload")?;
        self.start_downloader(active_cfg).await?;

        Ok(sources)
    }

    async fn start_downloader(&self, cfg: DownloaderConfig) -> Result<()> {
        let repo = repo::make_repo_from_config(&cfg);
        let availability = DownloaderAvailabilityReporter::new(&cfg, repo.capabilities());

        availability.send_initializing();
        self.manager.clear().await;

        let cache_dir = runtime_cache_dir(self.sources.app_dir(), &cfg.id);
        let _ = tokio::fs::create_dir_all(&cache_dir).await;

        let (rclone_path, rclone_config_path) = prepare_downloader_runtime(&cache_dir, &cfg)
            .await
            .inspect_err(|e| availability.send_error("prepare downloader", e))?;

        let downloader = Downloader::new(
            Arc::new(cfg),
            cache_dir,
            rclone_path,
            rclone_config_path,
            self.settings_handler.clone(),
            WatchStream::new(self.settings_handler.subscribe()),
        )
        .await
        .inspect_err(|e| availability.send_error("initialize downloader", e))?;

        self.manager.replace(downloader).await;
        availability.send_available();
        Ok(())
    }

    async fn install_from_url(&self, url: &str) {
        let result = async {
            let cfg = self.sources.install_from_url(url, true).await?;
            Ok::<_, anyhow::Error>(cfg.id)
        }
        .await;

        match result {
            Ok(config_id) => {
                if let Err(e) = self.reload_and_apply(ReloadReason::Install, Vec::new()).await {
                    error!(
                        error = e.as_ref() as &dyn Error,
                        "Downloader init after config install failed"
                    );
                    let message = format!("Failed to initialize downloader: {:#}", e);
                    DownloaderConfigInstallResult { success: false, error: Some(message.clone()) }
                        .send_signal_to_dart();
                    Toast::send("Failed to add downloader source".into(), message, true, None);
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

    async fn remove_source(&self, config_id: String) {
        match self.sources.remove(&config_id) {
            Ok(()) => {
                let reload_result = self.reload_and_apply(ReloadReason::Remove, Vec::new()).await;
                let cleanup_result = self.sources.delete_cache_dir(&config_id);

                let mut errors = Vec::new();
                if let Err(e) = reload_result {
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

    async fn select_source(&self, config_id: &str) {
        if let Err(e) = self.sources.select_active(config_id) {
            send_error_toast("Failed to switch downloader source", &e);
            return;
        }

        if let Err(e) = self.reload_and_apply(ReloadReason::Select, Vec::new()).await {
            send_error_toast("Failed to switch downloader source", &e);
        }
    }

    async fn manual_refresh(&self) {
        let loaded = match self.sources.load(Vec::new()) {
            Ok(sources) => sources,
            Err(e) => {
                send_error_toast("Failed to refresh downloader sources", &e);
                return;
            }
        };

        send_sources_changed(&loaded, true);

        let report = self.sources.refresh_all(&loaded.configs).await;
        send_refresh_complete_toast(&report);

        let warnings = report.warning_message().into_iter().collect();
        if let Err(e) = self.reload_and_apply(ReloadReason::ManualRefresh, warnings).await {
            send_error_toast("Failed to refresh downloader sources", &e);
        }
    }

    fn spawn_background_refresh(self: Arc<Self>, configs: Vec<DownloaderConfig>) {
        tokio::spawn(async move {
            let report = self.sources.refresh_all(&configs).await;
            let warnings: Vec<_> = report.warning_message().into_iter().collect();

            let sources = match self.sources.load(warnings) {
                Ok(sources) => sources,
                Err(e) => {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Failed to reload downloader sources after background refresh"
                    );
                    return;
                }
            };

            send_sources_changed(&sources, false);
        });
    }

    fn start_request_handlers(self: Arc<Self>) {
        tokio::spawn({
            let controller = self.clone();
            async move {
                let receiver = InstallDownloaderConfigFromUrlRequest::get_dart_signal_receiver();
                while let Some(req) = receiver.recv().await {
                    let url = req.message.url.trim().to_string();
                    debug!(url = %url, "Received InstallDownloaderConfigFromUrlRequest");
                    controller.install_from_url(&url).await;
                }

                panic!("InstallDownloaderConfigFromUrlRequest receiver closed")
            }
        });

        tokio::spawn({
            let controller = self.clone();
            async move {
                let receiver = RemoveDownloaderSourceRequest::get_dart_signal_receiver();
                while let Some(req) = receiver.recv().await {
                    let config_id = req.message.config_id.trim().to_string();
                    debug!(config_id = %config_id, "Received RemoveDownloaderSourceRequest");
                    controller.remove_source(config_id).await;
                }

                panic!("RemoveDownloaderSourceRequest receiver closed")
            }
        });

        tokio::spawn({
            let controller = self.clone();
            async move {
                let receiver = SelectDownloaderSourceRequest::get_dart_signal_receiver();
                while let Some(req) = receiver.recv().await {
                    let config_id = req.message.config_id.trim().to_string();
                    controller.select_source(&config_id).await;
                }

                panic!("SelectDownloaderSourceRequest receiver closed")
            }
        });

        tokio::spawn({
            let controller = self.clone();
            async move {
                let receiver = RefreshDownloaderSourcesRequest::get_dart_signal_receiver();
                while receiver.recv().await.is_some() {
                    controller.manual_refresh().await;
                }

                panic!("RefreshDownloaderSourcesRequest receiver closed")
            }
        });

        tokio::spawn({
            let controller = self.clone();
            async move {
                let receiver = RetryDownloaderInitRequest::get_dart_signal_receiver();
                while receiver.recv().await.is_some() {
                    if let Err(e) =
                        controller.reload_and_apply(ReloadReason::Retry, Vec::new()).await
                    {
                        send_error_toast("Failed to initialize downloader", &e);
                    }
                }

                panic!("RetryDownloaderInitRequest receiver closed")
            }
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

async fn prepare_downloader_runtime(
    cache_dir: &Path,
    cfg: &DownloaderConfig,
) -> Result<(Option<std::path::PathBuf>, Option<std::path::PathBuf>)> {
    match cfg.layout {
        RepoLayoutKind::Ffa => crate::downloader::rclone::prepare_rclone_files(cache_dir, cfg)
            .await
            .map(|(rclone_path, rclone_config_path)| (Some(rclone_path), Some(rclone_config_path))),
        RepoLayoutKind::NewRepo => Ok((None, None)),
    }
}

fn send_sources_changed(sources: &LoadedSources, refreshing: bool) {
    DownloaderSourcesChanged {
        configs: sources.installed_configs(),
        active_config_id: sources.active_config_id.clone(),
        refreshing,
        error: sources.warning_message(),
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
