use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use anyhow::{Context, Result};
use rinf::{DartSignal, RustSignal};
use tokio::sync::Mutex;
use tokio_stream::wrappers::WatchStream;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::{
    downloader::{
        DownloaderSession, SensitiveUrl,
        config::{DownloaderConfig, RepoLayoutKind},
        manager::DownloaderManager,
        repo,
        sources::{
            CacheLease, ConfigFetch, RefreshReport, SourceSnapshot, SourceStore, runtime_cache_dir,
            warnings_to_message,
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
    state: Arc<Mutex<ControllerState>>,
}

#[derive(Default)]
struct ControllerState {
    revisions: HashMap<String, u64>,
    active: Option<DownloaderConfig>,
    initialization: Option<tokio::task::JoinHandle<()>>,
    initialization_cancel: CancellationToken,
    generation: u64,
    refreshing: usize,
    leases: Vec<(String, Weak<CacheLease>)>,
}

impl ControllerState {
    fn advance(&mut self, id: &str) -> u64 {
        let revision = self.revisions.entry(id.to_string()).or_default();
        *revision += 1;
        *revision
    }
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
            state: Arc::new(Mutex::new(ControllerState::default())),
        })
    }

    pub(crate) fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            self.startup().await;
        });
    }

    async fn startup(self: Arc<Self>) {
        // Migration precedes request handling, so it cannot race a source mutation.
        let mut warnings = self
            .sources
            .migrate_legacy_config_if_needed()
            .await
            .map(|error| {
                send_error_toast("Failed to migrate legacy downloader config", &error);
                format!("{error:#}")
            })
            .into_iter()
            .collect::<Vec<_>>();
        self.clone().start_request_handlers();
        let loaded = match self.sources.load() {
            Ok(loaded) => loaded,
            Err(error) => {
                send_error_toast("Failed to load downloader sources", &error);
                return;
            }
        };
        if let Some(active) = loaded.active_config() {
            warnings.extend(self.refresh_configs(vec![active]).await.warning_message());
        }
        if let Err(error) = self.sync(false, warnings).await {
            send_error_toast("Failed to initialize downloader", &error);
        }
        let inactive = self.sources.inactive_configs(&loaded);
        if !inactive.is_empty() {
            tokio::spawn(async move {
                self.refresh_configs(inactive).await;
            });
        }
    }

    async fn sync(self: &Arc<Self>, retry: bool, warnings: Vec<String>) -> Result<()> {
        let mut state = self.state.lock().await;
        self.reconcile(&mut state, retry, &warnings).await
    }

    /// Called with the controller lock held after each source mutation.
    async fn reconcile(
        self: &Arc<Self>,
        state: &mut ControllerState,
        retry: bool,
        warnings: &[String],
    ) -> Result<()> {
        let sources = self.sources.load()?;
        let persisted = self.sources.persist_active_config(&sources);
        send_sources_changed(&sources, state.refreshing > 0, warnings);
        let active = sources.active_config();
        let unchanged = match (&state.active, &active) {
            (Some(old), Some(new)) => old.same_runtime(new),
            (None, None) => true,
            _ => false,
        };
        if unchanged && !retry {
            if active.is_none() {
                DownloaderAvailabilityChanged { needs_setup: true, ..Default::default() }
                    .send_signal_to_dart();
            }
            return persisted;
        }
        state.generation += 1;
        if let Some(task) = state.initialization.take() {
            state.initialization_cancel.cancel();
            let _ = task.await;
        }
        self.manager.clear().await;
        state.active = active.clone();
        let Some(cfg) = active else {
            DownloaderAvailabilityChanged { needs_setup: true, ..Default::default() }
                .send_signal_to_dart();
            return persisted;
        };
        let availability =
            DownloaderAvailabilityReporter::new(&cfg, repo::capabilities(cfg.layout));
        availability.send_initializing();
        let generation = state.generation;
        let controller = self.clone();
        let cancel = CancellationToken::new();
        state.initialization_cancel = cancel.clone();
        state.initialization = Some(tokio::spawn(async move {
            let cache_dir = runtime_cache_dir(controller.sources.app_dir(), &cfg.id);
            let settings = controller.settings_handler.subscribe().borrow().clone();
            let result = async {
                tokio::fs::create_dir_all(&cache_dir).await?;
                repo::Repo::new(&cfg, &cache_dir, &settings, &cancel).await
            }
            .await;
            let mut state = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                state = controller.state.lock() => state,
            };
            if state.generation != generation {
                return;
            }
            let result = async {
                let (mut repo, remote) = result?;
                if let Some(remote) = remote {
                    controller
                        .settings_handler
                        .update_downloader_remote(&settings.rclone_remote_name, &remote)?;
                }
                // Settings may have changed while preparing runtime files.
                let current = controller.settings_handler.subscribe().borrow().clone();
                repo.set_bandwidth_limit(current.bandwidth_limit.clone());
                let lease = Arc::new(CacheLease(Arc::new(tokio::sync::Notify::new())));
                state.leases.retain(|(_, lease)| lease.strong_count() > 0);
                state.leases.push((cfg.id.clone(), Arc::downgrade(&lease)));
                let session = DownloaderSession::new(
                    Arc::new(cfg),
                    repo,
                    cache_dir,
                    lease,
                    controller.settings_handler.clone(),
                    WatchStream::new(controller.settings_handler.subscribe()),
                )
                .await?;
                controller.manager.replace(session).await;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            match result {
                Ok(()) => availability.send_available(),
                Err(error) => availability.send_error("initialize downloader", &error),
            }
        }));
        persisted
    }

    async fn install_from_url(self: &Arc<Self>, url: SensitiveUrl<'_>) {
        let result = async {
            let fetch =
                ConfigFetch::new(self.sources.app_dir(), url.as_str(), None)?.fetch().await?;
            let mut state = self.state.lock().await;
            let cfg = fetch.commit(self.sources.app_dir())?;
            state.advance(&cfg.id);
            self.sources.select_active(&cfg.id)?;
            self.reconcile(&mut state, false, &[]).await?;
            Ok::<_, anyhow::Error>(cfg.id)
        }
        .await;
        match result {
            Ok(id) => {
                DownloaderConfigInstallResult { success: true, error: None }.send_signal_to_dart();
                Toast::send(
                    "Downloader source added".into(),
                    format!("Added source {id}"),
                    false,
                    None,
                );
            }
            Err(error) => {
                DownloaderConfigInstallResult { success: false, error: Some(format!("{error:#}")) }
                    .send_signal_to_dart();
                send_error_toast("Failed to add downloader source", &error);
            }
        }
    }

    async fn remove_source(self: &Arc<Self>, config_id: String) {
        let result = async {
            let mut state = self.state.lock().await;
            self.sources.remove(&config_id)?;
            let revision = state.advance(&config_id);
            let reconciled = self.reconcile(&mut state, false, &[]).await;
            let leases = state
                .leases
                .iter()
                .filter(|(id, _)| id == &config_id)
                .map(|(_, lease)| lease.clone())
                .collect::<Vec<_>>();
            let controller = self.clone();
            let id = config_id.clone();
            tokio::spawn(async move {
                controller.cleanup_removed_source(id, revision, leases).await;
            });
            reconciled
        }
        .await;
        let error = result.err().map(|error| format!("{error:#}"));
        DownloaderSourceRemovedResult {
            config_id: config_id.clone(),
            success: error.is_none(),
            error: error.clone(),
        }
        .send_signal_to_dart();
        match error {
            Some(error) => send_error_toast("Failed to remove downloader source", &error),
            None => Toast::send(
                "Downloader source removed".into(),
                format!("Removed source {config_id}"),
                false,
                None,
            ),
        }
    }

    async fn cleanup_removed_source(
        self: Arc<Self>,
        id: String,
        revision: u64,
        leases: Vec<Weak<CacheLease>>,
    ) {
        for lease in leases {
            if let Some(active) = lease.upgrade() {
                let notify = active.0.clone();
                drop(active);
                loop {
                    let released = notify.notified();
                    tokio::pin!(released);
                    released.as_mut().enable();
                    if lease.strong_count() == 0 {
                        break;
                    }
                    released.await;
                }
            }
        }
        let state = self.state.lock().await;
        if state.revisions.get(&id) != Some(&revision) {
            return;
        }
        if self
            .sources
            .load()
            .map_or(true, |sources| sources.configs.iter().any(|cfg| cfg.id == id))
        {
            return;
        }
        if let Err(error) = self.sources.delete_cache_dir(&id) {
            send_error_toast("Failed to clean downloader cache", &error);
        }
    }

    async fn select_source(self: &Arc<Self>, config_id: &str) {
        let result = async {
            let mut state = self.state.lock().await;
            self.sources.select_active(config_id)?;
            self.reconcile(&mut state, false, &[]).await
        }
        .await;
        if let Err(error) = result {
            send_error_toast("Failed to switch downloader source", &error);
        }
    }

    async fn manual_refresh(self: &Arc<Self>) {
        let loaded = {
            let _state = self.state.lock().await;
            self.sources.load()
        };
        match loaded {
            Ok(loaded) => send_refresh_complete_toast(&self.refresh_configs(loaded.configs).await),
            Err(error) => send_error_toast("Failed to refresh downloader sources", &error),
        }
    }

    async fn refresh_configs(self: &Arc<Self>, configs: Vec<DownloaderConfig>) -> RefreshReport {
        let mut report = RefreshReport::default();
        let pending = {
            let mut state = self.state.lock().await;
            state.refreshing += 1;
            let loaded = self.sources.load();
            if let Err(error) = &loaded {
                report.failed.push(format!("Failed to load sources: {error:#}"));
            }
            let mut pending = Vec::new();
            if let Ok(loaded) = loaded {
                send_sources_changed(&loaded, true, &[]);
                for cfg in configs {
                    if !loaded.configs.contains(&cfg) {
                        continue;
                    }
                    let ticket = state.advance(&cfg.id);
                    match cfg
                        .config_update_url
                        .as_deref()
                        .context("Missing config_update_url")
                        .and_then(|url| {
                            ConfigFetch::new(self.sources.app_dir(), url.trim(), Some(&cfg.id))
                        }) {
                        Ok(fetch) => pending.push((cfg, ticket, fetch)),
                        Err(error) => report.failed.push(format!("{}: {error:#}", cfg.id)),
                    }
                }
            }
            pending
        };
        for (cfg, ticket, fetch) in pending {
            let result = fetch.fetch().await;
            let mut state = self.state.lock().await;
            if state.revisions.get(&cfg.id) != Some(&ticket) {
                continue;
            }
            let current = self.sources.load();
            if !current.as_ref().is_ok_and(|sources| sources.configs.contains(&cfg)) {
                continue;
            }
            match result.and_then(|fetch| fetch.commit(self.sources.app_dir())) {
                Ok(updated) => {
                    report.refreshed += 1;
                    // Runtime files can change without their URLs changing.
                    let refresh_runtime = updated.layout == RepoLayoutKind::Ffa
                        && current.as_ref().unwrap().active_config_id.as_deref()
                            == Some(updated.id.as_str());
                    if let Err(error) = self.reconcile(&mut state, refresh_runtime, &[]).await {
                        report.failed.push(format!("{}: {error:#}", cfg.id));
                    }
                }
                Err(error) => report.failed.push(format!("{}: {error:#}", cfg.id)),
            }
        }
        let mut state = self.state.lock().await;
        state.refreshing -= 1;
        if let Ok(sources) = self.sources.load() {
            send_sources_changed(
                &sources,
                state.refreshing > 0,
                &report.warning_message().into_iter().collect::<Vec<_>>(),
            );
        }
        report
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
            let config_id = req.config_id;
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
            if let Err(e) = controller.sync(true, Vec::new()).await {
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
    DownloaderSourcesChanged {
        configs: sources.installed_configs.clone(),
        active_config_id: sources.active_config_id.clone(),
        refreshing,
        error: warnings_to_message(extra_warnings),
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

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::oneshot,
    };

    use super::*;
    use crate::downloader::{
        config::RepoLayoutKind,
        sources::{managed_config_path, managed_configs_dir},
    };

    fn config_json(id: &str, update_url: &str, name: &str) -> String {
        serde_json::json!({"id": id, "layout": "new-repo", "base_url": "http://127.0.0.1:1", "config_update_url": update_url, "display_name": name}).to_string()
    }

    fn install_fixture(controller: &DownloaderController, id: &str, url: &str) -> DownloaderConfig {
        let app_dir = controller.sources.app_dir();
        std::fs::create_dir_all(managed_configs_dir(app_dir)).unwrap();
        let path = managed_config_path(app_dir, id);
        std::fs::write(&path, config_json(id, url, id)).unwrap();
        DownloaderConfig::load_from_path(path).unwrap()
    }

    async fn fixture() -> (tempfile::TempDir, Arc<DownloaderController>) {
        let dir = tempfile::tempdir().unwrap();
        let settings = SettingsHandler::new(dir.path().to_path_buf(), true).unwrap();
        let controller =
            DownloaderController::new(DownloaderManager::new(), dir.path().to_path_buf(), settings);
        (dir, controller)
    }

    async fn settled(controller: &DownloaderController) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if controller
                    .state
                    .lock()
                    .await
                    .initialization
                    .as_ref()
                    .is_none_or(|task| task.is_finished())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn delayed_config()
    -> (String, oneshot::Receiver<()>, oneshot::Sender<String>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/config", listener.local_addr().unwrap());
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel::<String>();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
            }
            entered_tx.send(()).unwrap();
            let body = release_rx.await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        (url, entered_rx, release_tx, task)
    }

    #[tokio::test]
    async fn refresh_cannot_resurrect_removed_or_reinstalled_source() {
        for reinstall in [false, true] {
            let (_dir, controller) = fixture().await;
            let (url, entered, release, server) = delayed_config().await;
            let cfg = install_fixture(&controller, "a", &url);
            let refresh = tokio::spawn({
                let controller = controller.clone();
                async move { controller.refresh_configs(vec![cfg]).await }
            });
            entered.await.unwrap();
            controller.remove_source("a".into()).await;
            if reinstall {
                let mut state = controller.state.lock().await;
                install_fixture(&controller, "a", &url);
                state.advance("a");
            }
            release.send(config_json("a", &url, "stale")).unwrap();
            assert_eq!(refresh.await.unwrap().refreshed, 0);
            server.await.unwrap();
            let sources = controller.sources.load().unwrap();
            if reinstall {
                assert_eq!(sources.configs[0].display_name.as_deref(), Some("a"));
            } else {
                assert!(sources.configs.is_empty());
            }
            assert!(!runtime_cache_dir(controller.sources.app_dir(), "a").join("source").exists());
        }
    }

    #[tokio::test]
    async fn unchanged_and_inactive_mutations_keep_session_but_retry_replaces_it() {
        let (_dir, controller) = fixture().await;
        install_fixture(&controller, "a", "http://127.0.0.1:1/a");
        install_fixture(&controller, "b", "http://127.0.0.1:1/b");
        controller.sync(false, vec![]).await.unwrap();
        settled(&controller).await;
        let session = controller.manager.require().await.unwrap();
        controller.select_source("a").await;
        controller.remove_source("b".into()).await;
        let path = managed_config_path(controller.sources.app_dir(), "a");
        std::fs::write(&path, config_json("a", "http://127.0.0.1:1/new", "renamed")).unwrap();
        controller.sync(false, vec![]).await.unwrap();
        assert!(Arc::ptr_eq(&session, &controller.manager.require().await.unwrap()));
        controller.sync(true, vec![]).await.unwrap();
        settled(&controller).await;
        assert!(!Arc::ptr_eq(&session, &controller.manager.require().await.unwrap()));
        controller.manager.clear().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn manual_refresh_updates_runtime_files_at_unchanged_urls() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };

        let (_dir, controller) = fixture().await;
        let server = MockServer::start().await;
        let config = serde_json::json!({
            "id": "a", "layout": "ffa", "disable_randomize_remote": true,
            "rclone_path": format!("{}/rclone", server.uri()),
            "rclone_config_path": format!("{}/rclone.conf", server.uri()),
            "config_update_url": format!("{}/config", server.uri()),
        })
        .to_string();
        let app_dir = controller.sources.app_dir();
        std::fs::create_dir_all(managed_configs_dir(app_dir)).unwrap();
        std::fs::write(managed_config_path(app_dir, "a"), &config).unwrap();
        Mock::given(method("GET"))
            .and(path("/config"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&config))
            .expect(1)
            .mount(&server)
            .await;
        let binary = "#!/bin/sh\ncase \"$*\" in *listremotes*) cat \"$2\" ;; esac\n";
        Mock::given(method("GET"))
            .and(path("/rclone"))
            .respond_with(ResponseTemplate::new(200).set_body_string(binary))
            .expect(2)
            .mount(&server)
            .await;
        let old_config = Mock::given(method("GET"))
            .and(path("/rclone.conf"))
            .respond_with(ResponseTemplate::new(200).set_body_string("one:\n"))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        controller.sync(false, vec![]).await.unwrap();
        settled(&controller).await;
        let old_session = controller.manager.require().await.unwrap();
        assert_eq!(controller.settings_handler.subscribe().borrow().rclone_remote_name, "one");
        drop(old_config);
        Mock::given(method("GET"))
            .and(path("/rclone.conf"))
            .respond_with(ResponseTemplate::new(200).set_body_string("two:\n"))
            .expect(1)
            .mount(&server)
            .await;

        controller.manual_refresh().await;
        settled(&controller).await;
        assert_eq!(
            std::fs::read_to_string(runtime_cache_dir(app_dir, "a").join("rclone.conf")).unwrap(),
            "two:\n"
        );
        assert_eq!(controller.settings_handler.subscribe().borrow().rclone_remote_name, "two");
        assert!(!Arc::ptr_eq(&old_session, &controller.manager.require().await.unwrap()));
        controller.manager.clear().await;
        server.verify().await;
    }

    #[tokio::test]
    async fn background_refresh_reconciles_a_source_selected_during_fetch() {
        let (_dir, controller) = fixture().await;
        install_fixture(&controller, "a", "http://127.0.0.1:1/a");
        let (url, entered, release, server) = delayed_config().await;
        let cfg = install_fixture(&controller, "b", &url);
        controller.sync(false, vec![]).await.unwrap();
        settled(&controller).await;
        let refresh = tokio::spawn({
            let controller = controller.clone();
            async move { controller.refresh_configs(vec![cfg]).await }
        });
        entered.await.unwrap();
        controller.select_source("b").await;
        settled(&controller).await;
        let previous = controller.manager.require().await.unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&config_json("b", &url, "updated")).unwrap();
        value["base_url"] = "http://127.0.0.1:2".into();
        release.send(value.to_string()).unwrap();
        assert_eq!(refresh.await.unwrap().refreshed, 1);
        server.await.unwrap();
        settled(&controller).await;
        assert!(!Arc::ptr_eq(&previous, &controller.manager.require().await.unwrap()));
        assert_eq!(
            controller.state.lock().await.active.as_ref().unwrap().base_url.as_deref(),
            Some("http://127.0.0.1:2")
        );
        controller.manager.clear().await;
    }

    #[tokio::test]
    async fn removed_cache_waits_for_all_session_users_and_ignores_reinstallation() {
        for reinstall in [false, true] {
            let (_dir, controller) = fixture().await;
            install_fixture(&controller, "a", "http://127.0.0.1:1/a");
            controller.sync(false, vec![]).await.unwrap();
            settled(&controller).await;
            let session = controller.manager.require().await.unwrap();
            let cache = runtime_cache_dir(controller.sources.app_dir(), "a");
            std::fs::write(cache.join("sentinel"), "old").unwrap();
            controller.remove_source("a".into()).await;
            assert!(cache.join("sentinel").exists());
            let state = controller.state.lock().await;
            let revision = state.revisions["a"];
            let leases = state.leases.iter().map(|(_, lease)| lease.clone()).collect();
            drop(state);
            if reinstall {
                let mut state = controller.state.lock().await;
                install_fixture(&controller, "a", "http://127.0.0.1:1/a");
                state.advance("a");
            }
            drop(session);
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                controller.clone().cleanup_removed_source("a".into(), revision, leases),
            )
            .await
            .unwrap();
            assert_eq!(cache.exists(), reinstall);
        }
    }

    #[tokio::test]
    async fn selection_cancels_obsolete_initialization() {
        let (_dir, controller) = fixture().await;
        let (url, entered, release, server) = delayed_config().await;
        let app_dir = controller.sources.app_dir();
        std::fs::create_dir_all(managed_configs_dir(app_dir)).unwrap();
        std::fs::write(
            managed_config_path(app_dir, "a"),
            serde_json::json!({
                "id": "a", "layout": "ffa", "rclone_path": format!("{url}/binary"),
                "rclone_config_path": url, "config_update_url": url,
            })
            .to_string(),
        )
        .unwrap();
        install_fixture(&controller, "b", "http://127.0.0.1:1/b");
        controller.sync(false, vec![]).await.unwrap();
        entered.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), controller.select_source("b"))
            .await
            .unwrap();
        release.send("[obsolete]".into()).unwrap();
        server.await.unwrap();
        settled(&controller).await;
        assert_eq!(controller.state.lock().await.active.as_ref().unwrap().id, "b");
        assert!(controller.manager.get().await.is_some());
        controller.manager.clear().await;
    }

    #[tokio::test]
    async fn selection_waits_for_initializer_cleanup() {
        let (_dir, controller) = fixture().await;
        install_fixture(&controller, "a", "http://127.0.0.1:1/a");
        install_fixture(&controller, "b", "http://127.0.0.1:1/b");
        controller.sync(false, vec![]).await.unwrap();
        settled(&controller).await;
        let old_session = controller.manager.require().await.unwrap();
        let (cancelled, cancellation) = oneshot::channel();
        let (finish, cleanup) = oneshot::channel();
        {
            let mut state = controller.state.lock().await;
            let cancel = state.initialization_cancel.clone();
            state.initialization = Some(tokio::spawn(async move {
                cancel.cancelled().await;
                cancelled.send(()).unwrap();
                cleanup.await.unwrap();
            }));
        }
        let selection = tokio::spawn({
            let controller = controller.clone();
            async move { controller.select_source("b").await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), cancellation)
            .await
            .unwrap()
            .unwrap();
        assert!(!selection.is_finished());
        assert!(Arc::ptr_eq(&old_session, &controller.manager.require().await.unwrap()));
        finish.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), selection).await.unwrap().unwrap();
        settled(&controller).await;
        assert!(!Arc::ptr_eq(&old_session, &controller.manager.require().await.unwrap()));
        controller.manager.clear().await;
    }

    #[tokio::test]
    async fn newer_refresh_wins_when_requests_finish_out_of_order() {
        let (_dir, controller) = fixture().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/config", listener.local_addr().unwrap());
        let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let mut tasks = Vec::new();
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let requests = requests.clone();
                tasks.push(tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buf = [0; 1024];
                    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                        let n = socket.read(&mut buf).await.unwrap();
                        assert!(n > 0);
                        request.extend_from_slice(&buf[..n]);
                    }
                    let (release, body) = oneshot::channel::<String>();
                    requests.send(release).unwrap();
                    let body = body.await.unwrap();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                }));
            }
            for task in tasks {
                task.await.unwrap();
            }
        });
        let cfg = install_fixture(&controller, "a", &url);
        let first = tokio::spawn({
            let controller = controller.clone();
            let cfg = cfg.clone();
            async move { controller.refresh_configs(vec![cfg]).await }
        });
        let release_first = received.recv().await.unwrap();
        let second = tokio::spawn({
            let controller = controller.clone();
            async move { controller.refresh_configs(vec![cfg]).await }
        });
        let release_second = received.recv().await.unwrap();
        release_second.send(config_json("a", &url, "new")).unwrap();
        assert_eq!(second.await.unwrap().refreshed, 1);
        release_first.send(config_json("a", &url, "old")).unwrap();
        assert_eq!(first.await.unwrap().refreshed, 0);
        server.await.unwrap();
        assert_eq!(
            controller.sources.load().unwrap().configs[0].display_name.as_deref(),
            Some("new")
        );
        settled(&controller).await;
        controller.manager.clear().await;
    }

    #[test]
    fn runtime_comparison_uses_only_backend_inputs() {
        let cfg = DownloaderConfig {
            layout: RepoLayoutKind::NewRepo,
            base_url: Some("http://example.com".into()),
            ..Default::default()
        };
        let mut changed = cfg.clone();
        changed.display_name = Some("Name".into());
        changed.root_dir = "unused".into();
        changed.config_update_url = Some("http://example.com/new".into());
        assert!(cfg.same_runtime(&changed));
        changed.base_url = Some("http://other.example.com".into());
        assert!(!cfg.same_runtime(&changed));
    }
}
