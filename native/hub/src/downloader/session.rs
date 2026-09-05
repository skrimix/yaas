use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use anyhow::Result;
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock, mpsc::UnboundedSender};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, instrument, warn};

use crate::{
    adb::PackageName,
    downloader::{
        AppDownloadProgress, TransferStats, cloud_api, config::DownloaderConfig, download_metadata,
        repo,
    },
    models::{
        CloudApp, DownloadMode, Settings,
        signals::{
            cloud_apps::{
                details::{AppDetailsResponse, GetAppDetailsRequest},
                list::{CloudAppsChangedEvent, LoadCloudAppsRequest},
                reviews::{AppReviewsResponse, GetAppReviewsRequest},
            },
            downloads_local::DownloadsChanged,
            storage::remotes::{GetRcloneRemotesRequest, RcloneRemotesChanged},
            system::Toast,
        },
    },
    settings::SettingsHandler,
};

pub(crate) struct DownloaderSession {
    _cache_lease: Arc<crate::downloader::sources::CacheLease>,
    repo: RwLock<repo::Repo>,
    cache_dir: PathBuf,
    catalog: Mutex<Catalog>,
    load_task: Mutex<Option<CatalogLoad>>,
    tasks: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
    download_dir: RwLock<PathBuf>,
    write_legacy_release_json: RwLock<bool>,
    download_mode: RwLock<DownloadMode>,
    cancel_token: CancellationToken,
    http_client: reqwest::Client,
    installation_id: String,
}

#[derive(Default)]
struct Catalog {
    result: Option<repo::RepoAppList>,
    loading: bool,
    error: Option<String>,
}

impl Catalog {
    fn send(&self) {
        send_catalog_event(self.loading, self.result.clone(), self.error.clone());
    }
}

struct CatalogLoad {
    token: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl DownloaderSession {
    #[instrument(level = "debug", skip(repo, cache_lease, settings_handler, settings_stream))]
    pub(super) async fn new(
        config: Arc<DownloaderConfig>,
        repo: repo::Repo,
        cache_dir: PathBuf,
        cache_lease: Arc<crate::downloader::sources::CacheLease>,
        settings_handler: Arc<SettingsHandler>,
        mut settings_stream: WatchStream<Settings>,
    ) -> Result<Arc<Self>> {
        let settings =
            settings_stream.next().await.expect("Settings stream closed on downloader init");

        let http_client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let repo_capabilities = repo::capabilities(config.layout);
        let donation_remote_configured = repo_capabilities.supports_donation_upload
            && (config.donation_remote_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
                || config.donation_remote_path.as_deref().map(|s| !s.is_empty()).unwrap_or(false));
        let blacklist_path_configured = repo_capabilities.supports_donation_upload
            && config.donation_blacklist_path.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
        if donation_remote_configured && !blacklist_path_configured {
            warn!(
                "App donation remote is configured but `donation_blacklist_path` is missing; \
                 donation blacklist will be disabled"
            );
        }

        let cancel_token = CancellationToken::new();

        let handle = Arc::new(Self {
            _cache_lease: cache_lease,
            repo: RwLock::new(repo),
            cache_dir,
            catalog: Mutex::new(Catalog::default()),
            load_task: Mutex::new(None),
            tasks: StdMutex::new(Vec::new()),
            download_dir: RwLock::new(settings.downloads_location()),
            write_legacy_release_json: RwLock::new(settings.write_legacy_release_json),
            download_mode: RwLock::new(settings.download_mode),
            cancel_token,
            http_client,
            installation_id: settings.installation_id.clone(),
        });

        // Observe a remote change made during initialization on the first stream update.
        let mut previous = settings.clone();
        previous.rclone_remote_name =
            handle.repo.read().await.remote().unwrap_or(&settings.rclone_remote_name).to_string();
        handle.spawn_tasks(&previous, settings_stream, &settings_handler);

        Ok(handle)
    }

    fn spawn_tasks(
        self: &Arc<Self>,
        settings: &Settings,
        settings_stream: WatchStream<Settings>,
        settings_handler: &Arc<SettingsHandler>,
    ) {
        let handle = self.clone();
        let settings = settings.clone();
        let settings_handler = settings_handler.clone();
        let task = tokio::spawn(async move {
            handle
                .cancel_token
                .run_until_cancelled(handle.receive_commands(
                    settings,
                    settings_stream,
                    settings_handler,
                ))
                .await;
        });
        self.tasks.lock().unwrap().push(task);
    }

    async fn apply_settings(
        self: &Arc<Self>,
        settings: &Settings,
        previous: &mut Settings,
        settings_handler: &SettingsHandler,
    ) {
        *self.download_dir.write().await = settings.downloads_location();
        *self.write_legacy_release_json.write().await = settings.write_legacy_release_json;
        *self.download_mode.write().await = settings.download_mode;

        if settings.bandwidth_limit != previous.bandwidth_limit {
            self.repo.write().await.set_bandwidth_limit(settings.bandwidth_limit.clone());
        }
        if settings.rclone_remote_name != previous.rclone_remote_name
            && self
                .repo
                .read()
                .await
                .remote()
                .is_some_and(|remote| remote != settings.rclone_remote_name)
        {
            let mut repo = self.repo.read().await.clone();
            let old_remote = repo.remote().map(str::to_string);
            match repo.select_remote(&settings.rclone_remote_name).await {
                Ok(remote) => {
                    let changed = remote != old_remote;
                    if let Some(remote) = remote
                        && let Err(e) = settings_handler
                            .update_downloader_remote(&settings.rclone_remote_name, &remote)
                    {
                        warn!(error = %e, "Failed to persist selected remote");
                    }
                    if changed {
                        self.cancel_load().await;
                        *self.repo.write().await = repo;
                        *self.catalog.lock().await = Catalog::default();
                        send_catalog_event(
                            true,
                            Some(repo::RepoAppList {
                                apps: Vec::new(),
                                donation_blacklist: Vec::new(),
                            }),
                            None,
                        );
                        self.request_load(true).await;
                    }
                }
                Err(e) => error!(error = %e, "Failed to select downloader remote"),
            }
        }
        *previous = settings.clone();
    }

    async fn send_remotes(&self) {
        let repo = self.repo.read().await.clone();
        match repo.list_remotes().await {
            Ok(remotes) => {
                RcloneRemotesChanged { remotes, error: None }.send_signal_to_dart();
            }
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to get downloader remotes");
                RcloneRemotesChanged {
                    remotes: Vec::new(),
                    error: Some(format!("Failed to get remotes: {:#}", e)),
                }
                .send_signal_to_dart();
            }
        }
    }

    /// Returns the cached CloudApp (if any) that matches the given full name
    #[instrument(level = "debug", skip(self))]
    async fn get_app_by_full_name(&self, full_name: &str) -> Option<CloudApp> {
        let cache = self.catalog.lock().await;
        cache.result.as_ref()?.apps.iter().find(|a| a.full_name == full_name).cloned()
    }

    /// Upload a prepared archive used for app donation.
    ///
    /// This uses optional `donation_remote_name` and `donation_remote_path` from DownloaderConfig.
    /// If either is missing or empty, the call fails with a configuration error.
    #[instrument(skip(self, stats_tx, cancellation_token))]
    pub(crate) async fn upload_donation_archive(
        &self,
        archive_path: &Path,
        stats_tx: Option<UnboundedSender<TransferStats>>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let repo = self.repo.read().await.clone();
        repo.upload_donation_archive(archive_path, stats_tx, cancellation_token).await
    }

    #[instrument(level = "debug", skip_all)]
    async fn receive_commands(
        self: &Arc<Self>,
        mut previous: Settings,
        mut settings_stream: WatchStream<Settings>,
        settings_handler: Arc<SettingsHandler>,
    ) {
        let current = settings_handler.subscribe().borrow().clone();
        self.apply_settings(&current, &mut previous, &settings_handler).await;
        self.send_remotes().await;
        let load_cloud_apps_receiver = LoadCloudAppsRequest::get_dart_signal_receiver();
        let get_rclone_remotes_receiver = GetRcloneRemotesRequest::get_dart_signal_receiver();
        let get_app_details_receiver = GetAppDetailsRequest::get_dart_signal_receiver();
        let get_app_reviews_receiver = GetAppReviewsRequest::get_dart_signal_receiver();
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    info!("Downloader command loop cancelled, exiting");
                    return;
                }
                settings = settings_stream.next() => {
                    let Some(settings) = settings else { return };
                    self.apply_settings(&settings, &mut previous, &settings_handler).await;
                }
                request = load_cloud_apps_receiver.recv() => {
                    if let Some(request) = request {
                        debug!(refresh = request.message.refresh, "Received LoadCloudAppsRequest");
                        self.request_load(request.message.refresh).await;
                    } else {
                        info!("LoadCloudAppsRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
                request = get_rclone_remotes_receiver.recv() => {
                    if request.is_some() {
                        debug!("Received GetRcloneRemotesRequest");
                        self.send_remotes().await;
                    } else {
                        info!("GetRcloneRemotesRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
                request = get_app_details_receiver.recv() => {
                    if let Some(request) = request {
                        let package_name = request.message.package_name;
                        debug!(%package_name, "Received GetAppDetailsRequest");
                        let client = self.http_client.clone();
                        let token = self.cancel_token.clone();
                        let task = tokio::spawn(async move {
                            token.run_until_cancelled(async move {
                            let package = match PackageName::parse(&package_name) {
                                Ok(p) => p,
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Invalid package name");
                                    AppDetailsResponse::default_error(package_name, format!("Invalid package name: {:#}", e)).send_signal_to_dart();
                                    return;
                                }
                            };

                            match cloud_api::fetch_app_details(&client, package).await {
                                Ok(Some(api)) => {
                                    let crate::models::AppApiResponse {
                                        id,
                                        display_name,
                                        description,
                                        quality_rating_aggregate,
                                        rating_count,
                                    } = api;
                                    AppDetailsResponse {
                                        package_name,
                                        app_id: id,
                                        display_name,
                                        description,
                                        rating_average: quality_rating_aggregate,
                                        rating_count,
                                        not_found: false,
                                        error: None,
                                    }.send_signal_to_dart();
                                }
                                Ok(None) => {
                                    AppDetailsResponse::default_not_found(package_name).send_signal_to_dart();
                                }
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Failed to fetch app details");
                                    AppDetailsResponse::default_error(package_name, format!("Failed to fetch app details: {:#}", e)).send_signal_to_dart();
                                }
                            }
                            }).await;
                        });
                        let mut tasks = self.tasks.lock().unwrap();
                        tasks.retain(|task| !task.is_finished());
                        tasks.push(task);
                    } else {
                        info!("GetAppDetailsRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
                request = get_app_reviews_receiver.recv() => {
                    if let Some(request) = request {
                        let app_id = request.message.app_id;
                        let limit = request.message.limit.unwrap_or(5);
                        let offset = request.message.offset.unwrap_or(0);
                        let sort_by = request.message.sort_by.unwrap_or_else(|| "helpful".to_string());
                        debug!(%app_id, "Received GetAppReviewsRequest");
                        let client = self.http_client.clone();
                        let token = self.cancel_token.clone();
                        let task = tokio::spawn(async move {
                            token.run_until_cancelled(async move {
                            match cloud_api::fetch_app_reviews(&client, &app_id, limit, offset, &sort_by).await {
                                Ok(reviews) => {
                                    AppReviewsResponse { app_id, total: Some(reviews.total), reviews: reviews.reviews, error: None }.send_signal_to_dart();
                                }
                                Err(e) => {
                                    error!(error = e.as_ref() as &dyn Error, "Failed to fetch app reviews");
                                    AppReviewsResponse { app_id, total: None, reviews: Vec::new(), error: Some(format!("Failed to fetch reviews: {:#}", e)) }.send_signal_to_dart();
                                }
                            }
                            }).await;
                        });
                        let mut tasks = self.tasks.lock().unwrap();
                        tasks.retain(|task| !task.is_finished());
                        tasks.push(task);
                    } else {
                        info!("GetAppReviewsRequest receiver closed, shutting down downloader command loop");
                        return;
                    }
                }
            }
        }
    }

    pub(crate) async fn stop(&self) {
        self.cancel_token.cancel();
        loop {
            let tasks = std::mem::take(&mut *self.tasks.lock().unwrap());
            if tasks.is_empty() {
                break;
            }
            for task in tasks {
                let _ = task.await;
            }
        }
        self.cancel_load().await;
    }

    async fn cancel_load(&self) {
        let mut load = self.load_task.lock().await;
        if let Some(running) = load.as_mut() {
            running.token.cancel();
            let _ = (&mut running.task).await;
        }
        *load = None;
    }

    async fn request_load(self: &Arc<Self>, force_refresh: bool) {
        if !force_refresh {
            let catalog = self.catalog.lock().await;
            if catalog.result.is_some() {
                catalog.send();
                return;
            }
            drop(catalog);
            if self.load_task.lock().await.as_ref().is_some_and(|load| !load.task.is_finished()) {
                return;
            }
        }
        self.cancel_load().await;
        let token = self.cancel_token.child_token();
        let handle = self.clone();
        let mut load = self.load_task.lock().await;
        let task_token = token.clone();
        let task = tokio::spawn(async move {
            task_token.run_until_cancelled(handle.load_app_list(task_token.clone())).await;
        });
        *load = Some(CatalogLoad { token, task });
    }

    async fn load_app_list(&self, cancellation_token: CancellationToken) {
        {
            let mut catalog = self.catalog.lock().await;
            catalog.loading = true;
            catalog.error = None;
            catalog.send();
        }
        let repo = self.repo.read().await.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            repo.load_app_list(&self.cache_dir, &self.http_client, cancellation_token.clone()),
        )
        .await;
        let mut result = match result {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                self.catalog_error(format!("Failed to load app list: {error:#}")).await;
                return;
            }
            Err(_) => {
                self.catalog_error("Timed out while loading app list".into()).await;
                return;
            }
        };
        self.publish_catalog(result.clone()).await;
        if !result.apps.is_empty() {
            match cloud_api::load_popularity_for_apps(&self.http_client, &mut result.apps).await {
                Ok(()) => {
                    self.publish_catalog(result).await;
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load popularity data");
                    Toast::send(
                        "Error".into(),
                        format!("Failed to load popularity data: {e:#}"),
                        true,
                        Some(Duration::from_secs(5)),
                    );
                }
            }
        }
    }

    async fn publish_catalog(&self, result: repo::RepoAppList) {
        let mut catalog = self.catalog.lock().await;
        *catalog = Catalog { result: Some(result), loading: false, error: None };
        catalog.send();
    }

    async fn catalog_error(&self, error: String) {
        let mut catalog = self.catalog.lock().await;
        catalog.loading = false;
        catalog.error = Some(error);
        catalog.send();
    }

    #[instrument(skip(self, progress_tx, cancellation_token), ret)]
    pub(crate) async fn download_app(
        &self,
        app_full_name: String,
        true_package: PackageName,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<String> {
        let dst_dir = self.download_dir.read().await.join(&app_full_name);
        info!(app = %app_full_name, dest = %dst_dir.display(), "Starting app download");
        let _ = progress_tx.send(AppDownloadProgress::Status("Preparing download...".to_string()));

        let repo = self.repo.read().await.clone();
        let download_mode = *self.download_mode.read().await;
        let download_result = match repo
            .download_app(
                &app_full_name,
                &dst_dir,
                &self.http_client,
                download_mode,
                progress_tx.clone(),
                cancellation_token.clone(),
            )
            .await
        {
            Ok(result) => result,
            Err(error) if cancellation_token.is_cancelled() => {
                info!(
                    app = %app_full_name,
                    error = error.as_ref() as &dyn Error,
                    "App download cancelled"
                );
                return Err(error);
            }
            Err(error) => {
                error!(
                    app = %app_full_name,
                    error = error.as_ref() as &dyn Error,
                    "App download failed"
                );
                return Err(error);
            }
        };

        if !download_result.skipped {
            let installation_id = self.installation_id.clone();
            tokio::spawn({
                let http_client = self.http_client.clone();
                async move {
                    if let Err(e) =
                        cloud_api::track_download(&http_client, &installation_id, true_package)
                            .await
                    {
                        warn!(
                            error = e.as_ref() as &dyn Error,
                            "Failed to send download track event"
                        );
                    }
                }
                .instrument(info_span!("task_track_download"))
            });
        }

        // Prepare metadata inputs without holding long locks
        let cached = self.get_app_by_full_name(&app_full_name).await;
        let write_legacy = *self.write_legacy_release_json.read().await;
        let _ = progress_tx.send(AppDownloadProgress::Status("Writing metadata...".to_string()));

        if let Err(e) = download_metadata::write_metadata(
            cached.clone(),
            &app_full_name,
            &dst_dir,
            write_legacy,
        )
        .await
        {
            warn!(
                error = e.as_ref() as &dyn Error,
                dir = %dst_dir.display(),
                "Failed to write download metadata"
            );
        }

        // Notify UI that downloads may have changed
        DownloadsChanged {}.send_signal_to_dart();

        Ok(dst_dir.display().to_string())
    }
}

fn send_catalog_event(is_loading: bool, result: Option<repo::RepoAppList>, error: Option<String>) {
    let (apps, donation_blacklist) = match result {
        Some(result) => (Some(result.apps), Some(result.donation_blacklist)),
        None => (None, None),
    };
    CloudAppsChangedEvent { is_loading, apps, donation_blacklist, error }.send_signal_to_dart();
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::downloader::{config::RepoLayoutKind, sources::CacheLease};

    async fn fixture(url: &str) -> (tempfile::TempDir, Arc<DownloaderSession>) {
        let dir = tempfile::tempdir().unwrap();
        let config = Arc::new(DownloaderConfig {
            layout: RepoLayoutKind::NewRepo,
            base_url: Some(url.to_string()),
            ..Default::default()
        });
        let settings = Settings::new(true);
        let (repo, _) = repo::Repo::new(&config, dir.path(), &settings, &CancellationToken::new())
            .await
            .unwrap();
        let session = Arc::new(DownloaderSession {
            _cache_lease: Arc::new(CacheLease(Arc::new(tokio::sync::Notify::new()))),
            repo: RwLock::new(repo),
            cache_dir: dir.path().to_path_buf(),
            catalog: Mutex::new(Catalog::default()),
            load_task: Mutex::new(None),
            tasks: StdMutex::new(Vec::new()),
            download_dir: RwLock::new(dir.path().to_path_buf()),
            write_legacy_release_json: RwLock::new(false),
            download_mode: RwLock::new(settings.download_mode),
            cancel_token: CancellationToken::new(),
            http_client: reqwest::Client::new(),
            installation_id: settings.installation_id,
        });
        (dir, session)
    }

    fn catalog() -> repo::RepoAppList {
        repo::RepoAppList {
            apps: vec![CloudApp::new(
                "App".into(),
                "App v1".into(),
                "com.example.app".into(),
                1,
                String::new(),
                100,
            )],
            donation_blacklist: vec!["com.example.blocked".into()],
        }
    }

    async fn finish_load(session: &DownloaderSession) {
        let mut load = session.load_task.lock().await;
        if let Some(load) = load.as_mut() {
            tokio::time::timeout(Duration::from_secs(5), &mut load.task).await.unwrap().unwrap();
        }
        *load = None;
    }

    #[tokio::test]
    async fn cached_requests_leave_enrichment_running() {
        let (_dir, session) = fixture("http://127.0.0.1:1").await;
        session.publish_catalog(catalog()).await;
        let token = session.cancel_token.child_token();
        let (release, wait) = oneshot::channel();
        let handle = session.clone();
        let task_token = token.clone();
        let task = tokio::spawn(async move {
            task_token
                .run_until_cancelled(async {
                    wait.await.unwrap();
                    let mut enriched = catalog();
                    enriched.apps[0].popularity = Some(crate::models::Popularity {
                        day_1: Some(100),
                        day_7: None,
                        day_30: None,
                    });
                    handle.publish_catalog(enriched).await;
                })
                .await;
        });
        *session.load_task.lock().await = Some(CatalogLoad { token: token.clone(), task });
        session.request_load(false).await;
        assert!(!token.is_cancelled());
        release.send(()).unwrap();
        finish_load(&session).await;
        let cache = session.catalog.lock().await;
        let result = cache.result.as_ref().unwrap();
        assert_eq!(result.apps[0].popularity.as_ref().unwrap().day_1, Some(100));
        assert_eq!(result.donation_blacklist, catalog().donation_blacklist);
    }

    #[tokio::test]
    async fn repeated_requests_coalesce_and_empty_catalog_is_cached() {
        let server = MockServer::start().await;
        let key = [7; 32];
        let list =
            yarc::app_list::AppList { schema_version: 1, generated_at: 0, releases: Vec::new() };
        let mut bytes = Vec::new();
        list.to_yarc(&mut bytes, key, 16).await.unwrap();
        Mock::given(method("HEAD"))
            .and(path("/list"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-yaas-key", const_hex::encode(key)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/list"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, session) = fixture(&server.uri()).await;
        session.request_load(false).await;
        session.request_load(false).await;
        finish_load(&session).await;
        session.request_load(false).await;
        assert!(session.load_task.lock().await.is_none());
        assert!(session.catalog.lock().await.result.as_ref().unwrap().apps.is_empty());
        server.verify().await;
    }

    #[tokio::test]
    async fn replacement_and_shutdown_join_obsolete_loads() {
        for stop in [false, true] {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;
            let (_dir, session) = fixture(&server.uri()).await;
            session.publish_catalog(catalog()).await;
            let token = session.cancel_token.child_token();
            let (finished, done) = oneshot::channel();
            let task_token = token.clone();
            let task = tokio::spawn(async move {
                task_token.run_until_cancelled(std::future::pending::<()>()).await;
                finished.send(()).unwrap();
            });
            *session.load_task.lock().await = Some(CatalogLoad { token: token.clone(), task });
            if stop {
                session.stop().await;
            } else {
                session.request_load(true).await;
                finish_load(&session).await;
                assert!(session.catalog.lock().await.error.is_some());
            }
            assert!(token.is_cancelled());
            done.await.unwrap();
            let cache = session.catalog.lock().await;
            let result = cache.result.as_ref().unwrap();
            assert_eq!(result.apps[0].full_name, "App v1");
            assert_eq!(result.donation_blacklist, catalog().donation_blacklist);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffa_bandwidth_changes_preserve_catalog_and_remote_feedback_does_not_reload() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, session) = fixture("http://127.0.0.1:1").await;
        let binary = dir.path().join("rclone");
        let config_path = dir.path().join("rclone.conf");
        std::fs::write(&config_path, "").unwrap();
        std::fs::write(
            &binary,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$2.calls"
case "$*" in
  *listremotes*) printf 'one:\ntwo:\n' ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = Arc::new(DownloaderConfig {
            rclone_path: Some(crate::downloader::config::RclonePath::Single(
                binary.display().to_string(),
            )),
            rclone_config_path: Some(config_path.display().to_string()),
            disable_randomize_remote: true,
            ..Default::default()
        });
        let settings = SettingsHandler::new(dir.path().to_path_buf(), true).unwrap();
        let mut previous = settings.subscribe().borrow().clone();
        previous.rclone_remote_name = "one".into();
        let (backend, _) =
            repo::Repo::new(&config, dir.path(), &previous, &CancellationToken::new())
                .await
                .unwrap();
        let old_backend = backend.clone();
        *session.repo.write().await = backend;
        session.publish_catalog(catalog()).await;
        let calls = || std::fs::read_to_string(config_path.with_extension("conf.calls")).unwrap();
        let initial_calls = calls();
        let mut next = previous.clone();
        next.bandwidth_limit = "10M".into();
        settings.save_settings(&next).unwrap();
        session.apply_settings(&next, &mut previous, &settings).await;
        assert_eq!(calls(), initial_calls);
        assert!(session.load_task.lock().await.is_none());
        assert!(session.catalog.lock().await.result.is_some());

        next.rclone_remote_name = "two".into();
        settings.save_settings(&next).unwrap();
        session.apply_settings(&next, &mut previous, &settings).await;
        finish_load(&session).await;
        assert!(session.catalog.lock().await.result.is_none());
        assert_eq!(session.repo.read().await.remote(), Some("two"));
        assert_eq!(old_backend.remote(), Some("one"));

        next.rclone_remote_name = "missing".into();
        settings.save_settings(&next).unwrap();
        session.apply_settings(&next, &mut previous, &settings).await;
        finish_load(&session).await;
        let feedback = settings.subscribe().borrow().clone();
        assert_eq!(feedback.rclone_remote_name, "one");
        let before_feedback = calls();
        session.apply_settings(&feedback, &mut previous, &settings).await;
        assert_eq!(calls(), before_feedback);
        assert!(session.load_task.lock().await.is_none());
    }

    #[tokio::test]
    async fn newrepo_ignores_remote_and_bandwidth_changes() {
        let (dir, session) = fixture("http://127.0.0.1:1").await;
        session.publish_catalog(catalog()).await;
        let settings = SettingsHandler::new(dir.path().to_path_buf(), true).unwrap();
        let mut previous = settings.subscribe().borrow().clone();
        let mut next = previous.clone();
        next.rclone_remote_name = "other".into();
        next.bandwidth_limit = "10M".into();
        session.apply_settings(&next, &mut previous, &settings).await;
        assert!(session.load_task.lock().await.is_none());
        assert_eq!(
            session.catalog.lock().await.result.as_ref().unwrap().apps[0].full_name,
            "App v1"
        );
    }
}
