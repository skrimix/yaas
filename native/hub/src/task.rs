use std::{
    collections::HashMap,
    error::Error,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, ensure};
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock, Semaphore, mpsc};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, instrument, warn};

use crate::{
    adb::{
        AdbHandler,
        device::{BackupOptions, SideloadProgress},
    },
    apk::get_apk_info,
    archive::create_zip_from_dir,
    downloader::RcloneTransferStats,
    downloader_manager::DownloaderManager,
    downloads_catalog::DownloadsCatalog,
    models::{
        Settings,
        signals::{
            backups::BackupsChanged,
            system::Toast,
            task::{Task, TaskCancelRequest, TaskKind, TaskProgress, TaskRequest, TaskStatus},
        },
    },
};

struct ProgressUpdate {
    status: TaskStatus,
    step_number: u8,
    step_progress: Option<f32>,
    message: String,
}

#[derive(Debug)]
struct InstallStepConfig<'a> {
    step_number: u8,
    log_context: &'a str,
}

#[derive(Debug)]
struct AdbStepConfig<'a> {
    step_number: u8,
    waiting_msg: &'a str,
    running_msg: String,
    log_context: &'a str,
}

#[derive(Debug)]
struct BackupStepConfig {
    package_name: String,
    display_name: Option<String>,
    backup_apk: bool,
    backup_data: bool,
    backup_obb: bool,
    backup_name_append: Option<String>,
}

macro_rules! acquire_permit_or_cancel {
    ($semaphore:expr, $token:expr, $semaphore_name:literal) => {{
        if $token.is_cancelled() {
            info!(concat!("Task already cancelled before ", $semaphore_name, " semaphore acquisition"));
            return Err(anyhow!(concat!("Task cancelled before ", $semaphore_name)));
        }

        debug!(concat!("Waiting for ", $semaphore_name, " semaphore"));
        tokio::select! {
            permit = $semaphore.acquire() => permit,
            _ = $token.cancelled() => {
                info!(concat!("Task cancelled while waiting for ", $semaphore_name, " semaphore"));
                return Err(anyhow!(concat!("Task cancelled while waiting for ", $semaphore_name, " semaphore")));
            }
        }
    }};
}

pub struct TaskManager {
    adb_semaphore: Semaphore,
    download_semaphore: Semaphore,
    id_counter: AtomicU64,
    tasks: Mutex<HashMap<u64, (Task, CancellationToken)>>,
    adb_handler: Arc<AdbHandler>,
    downloader_manager: Arc<DownloaderManager>,
    downloads_catalog: Arc<DownloadsCatalog>,
    settings: RwLock<Settings>,
}

impl TaskManager {
    pub fn new(
        adb_handler: Arc<AdbHandler>,
        downloader_manager: Arc<DownloaderManager>,
        downloads_catalog: Arc<DownloadsCatalog>,
        mut settings_stream: WatchStream<Settings>,
    ) -> Arc<Self> {
        let initial_settings = futures::executor::block_on(settings_stream.next())
            .expect("Settings stream closed on task manager init");

        let handle = Arc::new(Self {
            adb_semaphore: Semaphore::new(1),
            download_semaphore: Semaphore::new(1),
            id_counter: AtomicU64::new(0),
            tasks: Mutex::new(HashMap::new()),
            adb_handler,
            downloader_manager,
            downloads_catalog,
            settings: RwLock::new(initial_settings),
        });

        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_requests().await;
            }
        });

        // Listen for settings updates
        tokio::spawn({
            let handle = handle.clone();
            async move {
                let mut stream = settings_stream;
                while let Some(settings) = stream.next().await {
                    *handle.settings.write().await = settings;
                }
            }
        });

        handle
    }

    #[instrument(skip(self))]
    async fn receive_requests(self: Arc<Self>) {
        let request_receiver = TaskRequest::get_dart_signal_receiver();
        let cancel_request_receiver = TaskCancelRequest::get_dart_signal_receiver();

        loop {
            tokio::select! {
                request = request_receiver.recv() => {
                    if let Some(request) = request {
                        self.clone().enqueue_task(request.message.task).await;
                    } else {
                        panic!("TaskRequest receiver closed");
                    }
                }
                cancel_request = cancel_request_receiver.recv() => {
                    if let Some(cancel_request) = cancel_request {
                        self.clone().cancel_task(cancel_request.message.task_id).await;
                    } else {
                        panic!("TaskCancelRequest receiver closed");
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    async fn enqueue_task(self: Arc<Self>, task: Task) -> u64 {
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();

        debug!(task_id = id, task = ?task, "Creating new task");

        let mut tasks = self.tasks.lock().await;
        let active_tasks_count = tasks.len();
        tasks.insert(id, (task.clone(), token.clone()));
        drop(tasks);

        debug!(task_id = id, active_tasks = active_tasks_count + 1, "Task added to queue");

        tokio::spawn({
            let handle = self.clone();
            async move {
                Box::pin(handle.process_task(id, task, token)).await;

                let mut tasks = handle.tasks.lock().await;
                tasks.remove(&id);
                let remaining_tasks = tasks.len();
                drop(tasks);
                debug!(task_id = id, remaining_tasks = remaining_tasks, "Task removed from queue");
            }
        });

        id
    }

    #[instrument(skip(self))]
    async fn cancel_task(self: Arc<Self>, task_id: u64) {
        let tasks = self.tasks.lock().await;
        if let Some((task, token)) = tasks.get(&task_id) {
            info!(
                task_id = task_id,
                task = %task,
                active_tasks = tasks.len(),
                "Received cancellation request for task"
            );
            token.cancel();
        } else {
            warn!(
                task_id = task_id,
                active_tasks = tasks.len(),
                "Task not found for cancellation - may have already completed"
            );
        }
    }

    #[instrument(skip(self, token))]
    async fn process_task(&self, id: u64, task: Task, token: CancellationToken) {
        let start_time = std::time::Instant::now();
        let task_kind = TaskKind::from(&task);

        let task_name = match task.task_name() {
            Ok(name) => {
                debug!(
                    task_id = id,
                    task_name = %name,
                    "Task name resolved"
                );
                name
            }
            Err(e) => {
                error!(task_id = id, error = e.as_ref() as &dyn Error, "Failed to get task name");
                send_progress(TaskProgress {
                    task_id: id,
                    task_kind,
                    task_name: None,
                    status: TaskStatus::Failed,
                    total_progress: 0.0,
                    message: format!("Failed to initialize task: {e:#}"),
                    current_step: 1,
                    total_steps: 1,
                    step_progress: None,
                });

                // Log task cleanup
                let duration = start_time.elapsed();
                error!(
                    task_id = id,
                    duration_ms = duration.as_millis(),
                    "Task failed during initialization"
                );
                return;
            }
        };
        let total_steps = task.total_steps();

        let task_name_clone = task_name.clone();
        let update_progress = move |u: ProgressUpdate| {
            // debug!(
            //     task_id = id,
            //     status = ?status,
            //     step_index = step_index,
            //     step_progress = ?step_progress,
            //     message = %message,
            //     "Task progress update"
            // ); // TODO: limit logging frequency
            let safe_total = total_steps.max(1) as f32;
            let completed_steps = u.step_number.saturating_sub(1) as f32;
            let sp = u.step_progress.unwrap_or(0.0).clamp(0.0, 1.0);
            let total_progress = (completed_steps + sp) / safe_total;

            send_progress(TaskProgress {
                task_id: id,
                task_kind,
                task_name: Some(task_name_clone.clone()),
                status: u.status,
                total_progress,
                message: u.message,
                current_step: u.step_number.into(),
                total_steps: total_steps.into(),
                step_progress: u.step_progress,
            });
        };

        update_progress(ProgressUpdate {
            status: TaskStatus::Waiting,
            step_number: 1,
            step_progress: None,
            message: "Starting...".into(),
        });

        Toast::send(
            task_name.clone(),
            format!("{}: starting", task.kind_label()),
            false,
            Some(Duration::from_secs(2)),
        );

        let result = match &task {
            Task::Download(app) => {
                info!(task_id = id, "Executing download task");
                self.handle_download(app.clone(), &update_progress, token.clone()).await
            }
            Task::DownloadInstall(app) => {
                info!(task_id = id, "Executing download and install task");
                self.handle_download_install(app.clone(), &update_progress, token.clone()).await
            }
            Task::InstallApk(apk_path) => {
                info!(task_id = id, "Executing APK install task");
                self.handle_install_apk(apk_path.clone(), &update_progress, token.clone()).await
            }
            Task::InstallLocalApp(app_path) => {
                info!(task_id = id, "Executing local app install task");
                self.handle_install_local_app(app_path.clone(), &update_progress, token.clone())
                    .await
            }
            Task::Uninstall { package_name, .. } => {
                info!(task_id = id, "Executing uninstall task");
                self.handle_uninstall(package_name.clone(), &update_progress, token.clone()).await
            }
            Task::BackupApp {
                package_name,
                display_name,
                backup_apk,
                backup_data,
                backup_obb,
                backup_name_append,
            } => {
                info!(task_id = id, "Executing backup task");
                self.handle_backup(
                    BackupStepConfig {
                        package_name: package_name.clone(),
                        display_name: display_name.clone(),
                        backup_apk: *backup_apk,
                        backup_data: *backup_data,
                        backup_obb: *backup_obb,
                        backup_name_append: backup_name_append.clone(),
                    },
                    &update_progress,
                    token.clone(),
                )
                .await
            }
            Task::RestoreBackup(path) => {
                info!(task_id = id, "Executing restore backup task");
                self.handle_restore(path.clone(), &update_progress, token.clone()).await
            }
            Task::ShareApp { package_name, display_name } => {
                info!(task_id = id, "Executing app share task");
                self.handle_share_app(
                    package_name.clone(),
                    display_name.clone(),
                    &update_progress,
                    token.clone(),
                )
                .await
            }
        };

        let duration = start_time.elapsed();

        match result {
            Ok(_) => {
                info!(
                    task_id = id,
                    task_name = %task_name,
                    duration_secs = duration.as_secs_f64(),
                    "Task completed successfully"
                );
                update_progress(ProgressUpdate {
                    status: TaskStatus::Completed,
                    step_number: total_steps,
                    step_progress: Some(1.0),
                    message: "Done".into(),
                });
                Toast::send(task_name, format!("{}: completed", task.kind_label()), false, None);
            }
            Err(e) => {
                // TODO: check error type?
                if token.is_cancelled() {
                    warn!(
                        task_id = id,
                        task_name = %task_name,
                        duration_ms = duration.as_millis(),
                        "Task was cancelled by user"
                    );
                    update_progress(ProgressUpdate {
                        status: TaskStatus::Cancelled,
                        step_number: total_steps,
                        step_progress: None,
                        message: "Task cancelled by user".into(),
                    });
                    Toast::send(
                        task_name,
                        format!("{}: cancelled", task.kind_label()),
                        false,
                        None,
                    );
                } else {
                    error!(
                        task_id = id,
                        task_name = %task_name,
                        duration_ms = duration.as_millis(),
                        error = e.as_ref() as &dyn Error,
                        error_chain = ?e.chain().collect::<Vec<_>>(),
                        "Task failed with error"
                    );
                    update_progress(ProgressUpdate {
                        status: TaskStatus::Failed,
                        step_number: total_steps,
                        step_progress: None,
                        message: format!("Task failed: {e:#}"),
                    });
                    Toast::send(
                        task_name,
                        format!("{}: failed", task.kind_label()),
                        true,
                        Some(Duration::from_secs(10)),
                    );
                }
            }
        }
    }

    #[instrument(skip(self, update_progress, token))]
    async fn run_download_step(
        &self,
        app_full_name: &str,
        step_number: u8,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<String> {
        ensure!(
            self.downloader_manager.is_some().await,
            "Downloader is not configured. Install configuration file to initialize."
        );
        update_progress(ProgressUpdate {
            status: TaskStatus::Waiting,
            step_number,
            step_progress: None,
            message: "Waiting to start download...".into(),
        });

        let _permit = acquire_permit_or_cancel!(self.download_semaphore, token, "download");
        debug!(
            download_permits_remaining = self.download_semaphore.available_permits(),
            "Acquired download semaphore"
        );

        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number,
            step_progress: None,
            message: "Starting download...".into(),
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();
        let (stage_tx, mut stage_rx) = mpsc::unbounded_channel::<String>();

        let mut download_task = {
            let downloader = self.downloader_manager.get().await.expect("downloader missing");
            let app_full_name = app_full_name.to_string();
            let token = token.clone();
            tokio::spawn(
                async move { downloader.download_app(app_full_name, tx, stage_tx, token).await }
                    .instrument(Span::current()),
            )
        };

        debug!("Starting download monitoring");
        let mut download_result: Option<String> = None;
        let mut last_log_time = std::time::Instant::now();
        let mut last_log_progress = 0.0;
        // Marker for the "bytes" > "total_bytes" case
        // We cant calculate progress if we get to that point
        let mut unknown_progress = false;

        while download_result.is_none() {
            tokio::select! {
                result = &mut download_task => {
                    let app_path = result
                        .context("Download task failed")?
                        .context("Failed to download app")?;
                    info!("Download task completed");
                    download_result = Some(app_path);
                }
                Some(progress) = rx.recv() => {
                    if unknown_progress {
                        // We can only report the speed, skip everything else
                        update_progress(ProgressUpdate {
                            status: TaskStatus::Running,
                            step_number,
                            step_progress: None,
                            message: format!(
                                "Downloading (Unknown%) - {}/s",
                                humansize::format_size(progress.speed, humansize::DECIMAL)
                            ),
                        });
                        continue;
                    }

                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;

                    // Log download progress every 10 seconds or at major milestones
                    let now = std::time::Instant::now();
                    let should_log = now.duration_since(last_log_time) > Duration::from_secs(10)
                        || ((0.25..0.26).contains(&step_progress)
                            || (0.5..0.51).contains(&step_progress)
                            || (0.75..0.76).contains(&step_progress))
                            && last_log_progress != step_progress;
                    let progress_percent = step_progress * 100.0;

                    if should_log {
                        debug!(
                            bytes_downloaded = progress.bytes,
                            total_bytes = progress.total_bytes,
                            speed_bytes_per_sec = progress.speed,
                            progress_percent,
                            "Download progress"
                        );
                        last_log_time = now;
                        last_log_progress = step_progress;
                    }

                    let (step_progress, message): (Option<f32>, String) = if progress.bytes <= progress.total_bytes {
                        (Some(step_progress), format!(
                            "Downloading ({:.1}%) - {}/s",
                            progress_percent,
                            humansize::format_size(progress.speed, humansize::DECIMAL)
                        ))
                    } else {
                        unknown_progress = true;
                        warn!(progress.bytes, progress.total_bytes, "Download progress is unknown: bytes > total_bytes");
                        (None, format!(
                            "Downloading (Unknown%) - {}/s",
                            humansize::format_size(progress.speed, humansize::DECIMAL)
                        ))
                    };

                    update_progress(ProgressUpdate {
                        status: TaskStatus::Running,
                        step_number,
                        step_progress,
                        message,
                    });
                }
                Some(stage_msg) = stage_rx.recv() => {
                    update_progress(ProgressUpdate {
                        status: TaskStatus::Running,
                        step_number,
                        step_progress: None,
                        message: stage_msg,
                    });
                }
            }
        }

        let app_path = download_result.unwrap();
        info!(
            app_path = %app_path,
            download_permits = self.download_semaphore.available_permits() + 1,
            "Download completed, releasing download semaphore"
        );
        drop(_permit);

        Ok(app_path)
    }

    #[instrument(skip(self, update_progress, token, spawn_install))]
    async fn run_install_step<'a>(
        &self,
        cfg: InstallStepConfig<'a>,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
        spawn_install: impl FnOnce(
            mpsc::UnboundedSender<SideloadProgress>,
            CancellationToken,
        ) -> tokio::task::JoinHandle<anyhow::Result<()>>,
    ) -> Result<()> {
        update_progress(ProgressUpdate {
            status: TaskStatus::Waiting,
            step_number: cfg.step_number,
            step_progress: None,
            message: "Waiting to start installation...".into(),
        });

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        debug!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for installation"
        );

        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number: cfg.step_number,
            step_progress: None,
            message: "Installing APK...".into(),
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();
        let mut install_task = spawn_install(tx, token.clone());

        debug!("Starting {} monitoring", cfg.log_context);
        let mut install_result = None;
        let mut last_log_time = std::time::Instant::now();
        let mut cancel_requested = false;

        while install_result.is_none() {
            tokio::select! {
                result = &mut install_task => {
                    install_result = Some(result.context("Install task failed")?);
                    info!("{} task completed", cfg.log_context);
                }
                _ = token.cancelled(), if !cancel_requested => {
                    warn!("Cancellation requested for install step, requesting task abort");
                    cancel_requested = true;
                    install_task.abort();
                }
                Some(progress) = rx.recv() => {
                    let step_progress_num = progress.progress.unwrap_or(0.0);

                    // Log progress every 5 seconds
                    let now = std::time::Instant::now();
                    if now.duration_since(last_log_time) > Duration::from_secs(5) {
                        debug!(
                            install_progress = step_progress_num,
                            status = %progress.status,
                            context = cfg.log_context,
                            "Installation progress"
                        );
                        last_log_time = now;
                    }

                    update_progress(ProgressUpdate {
                        status: TaskStatus::Running,
                        step_number: cfg.step_number,
                        step_progress: progress.progress,
                        message: progress.status,
                    });
                }
            }
        }

        install_result.unwrap()?;

        info!(
            adb_permits = self.adb_semaphore.available_permits() + 1,
            context = cfg.log_context,
            "Installation completed, releasing ADB semaphore"
        );

        Ok(())
    }

    #[instrument(skip(self, update_progress, token, fut))]
    async fn run_adb_one_step<'a, F, Fut, T>(
        &self,
        cfg: AdbStepConfig<'a>,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
        fut: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        update_progress(ProgressUpdate {
            status: TaskStatus::Waiting,
            step_number: cfg.step_number,
            step_progress: None,
            message: cfg.waiting_msg.into(),
        });

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        debug!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for {}", cfg.log_context
        );

        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number: cfg.step_number,
            step_progress: None,
            message: cfg.running_msg,
        });

        debug!("Starting {} operation", cfg.log_context);
        let result = fut().await?;
        debug!("{} operation completed", cfg.log_context);

        info!(
            adb_permits = self.adb_semaphore.available_permits() + 1,
            "{} completed, releasing ADB semaphore", cfg.log_context
        );

        Ok(result)
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_download_install(
        &self,
        app_full_name: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting download and install task"
        );

        let app_path =
            self.run_download_step(&app_full_name, 1, update_progress, token.clone()).await?;

        if token.is_cancelled() {
            warn!("Task was cancelled after download completion");
            return Err(anyhow!("Task cancelled after download"));
        }

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backups_location =
            std::path::PathBuf::from(self.settings.read().await.backups_location.clone());
        let app_path_cloned = app_path.clone();
        self.run_install_step(
            InstallStepConfig { step_number: 2, log_context: "sideload" },
            update_progress,
            token.clone(),
            move |tx, token| {
                let app_path = app_path_cloned.clone();
                let backups_location = backups_location.clone();
                tokio::spawn(
                    async move {
                        adb_handler
                            .sideload_app(
                                &device,
                                Path::new(&app_path),
                                backups_location,
                                tx,
                                token,
                            )
                            .await
                    }
                    .instrument(Span::current()),
                )
            },
        )
        .await?;

        // Apply downloads cleanup policy
        if let Err(e) = self.cleanup_downloads_after_install(&app_full_name, &app_path).await {
            // Non-fatal: log but do not fail the task
            error!(
                error = e.as_ref() as &dyn Error,
                "Failed to apply downloads cleanup policy after install"
            );
        }

        Ok(())
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_download(
        &self,
        app_full_name: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            "Starting download task"
        );

        let _ = self.run_download_step(&app_full_name, 1, update_progress, token).await?;

        Ok(())
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_install_apk(
        &self,
        apk_path: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            apk_path = %apk_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting APK install task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backups_location =
            std::path::PathBuf::from(self.settings.read().await.backups_location.clone());

        self.run_install_step(
            InstallStepConfig { step_number: 1, log_context: "apk_install" },
            update_progress,
            token,
            move |tx, _token| {
                let backups_location = backups_location.clone();
                tokio::spawn(
                    async move {
                        adb_handler
                            .install_apk(&device, Path::new(&apk_path), backups_location, tx)
                            .await
                    }
                    .instrument(Span::current()),
                )
            },
        )
        .await
        .map(|_| ())
        .context("APK installation failed")
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_install_local_app(
        &self,
        app_path: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            app_path = %app_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting local app install task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backups_location =
            std::path::PathBuf::from(self.settings.read().await.backups_location.clone());
        let app_path_cloned = app_path.clone();
        self.run_install_step(
            InstallStepConfig { step_number: 1, log_context: "sideload_local" },
            update_progress,
            token,
            move |tx, token| {
                let app_path = app_path_cloned.clone();
                let backups_location = backups_location.clone();
                tokio::spawn(
                    async move {
                        adb_handler
                            .sideload_app(
                                &device,
                                Path::new(&app_path),
                                backups_location,
                                tx,
                                token,
                            )
                            .await
                    }
                    .instrument(Span::current()),
                )
            },
        )
        .await
        .map(|_| ())
        .context("Local app installation failed")
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_uninstall(
        &self,
        package_name: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            package_name = %package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting uninstall task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let pkg = package_name.clone();
        self.run_adb_one_step(
            AdbStepConfig {
                step_number: 1,
                waiting_msg: "Waiting to start uninstallation...",
                running_msg: "Uninstalling app...".to_string(),
                log_context: "uninstall",
            },
            update_progress,
            token,
            move || {
                let package_name = pkg.clone();
                async move { adb_handler.uninstall_package(&device, &package_name).await }
            },
        )
        .await
        .map(|_| ())
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_backup(
        &self,
        cfg: BackupStepConfig,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        ensure!(cfg.backup_apk || cfg.backup_data || cfg.backup_obb, "No parts selected to backup");

        debug!(
            package_name = %cfg.package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting backup task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let parts = [
            if cfg.backup_data { Some("data") } else { None },
            if cfg.backup_apk { Some("apk") } else { None },
            if cfg.backup_obb { Some("obb") } else { None },
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        let backups_dir = { self.settings.read().await.backups_location.clone() };
        let backups_path = std::path::PathBuf::from(backups_dir);
        debug!(path = %backups_path.display(), "Using backups location");

        let options = BackupOptions {
            name_append: cfg.backup_name_append,
            backup_apk: cfg.backup_apk,
            backup_data: cfg.backup_data,
            backup_obb: cfg.backup_obb,
            require_private_data: false,
        };

        let pkg = cfg.package_name.clone();
        let display_name = cfg.display_name.clone();
        let options_moved = options;
        let backups_path_moved = backups_path.clone();
        let token_clone = token.clone();

        let maybe_created = self
            .run_adb_one_step(
                AdbStepConfig {
                    step_number: 1,
                    waiting_msg: "Waiting to start backup...",
                    running_msg: format!("Creating backup ({parts})..."),
                    log_context: "backup",
                },
                update_progress,
                token,
                move || {
                    let package_name = pkg.clone();
                    let display_name = display_name.clone();
                    let backups_path = backups_path_moved.clone();
                    let options = options_moved;
                    async move {
                        adb_handler
                            .backup_app(
                                &device,
                                &package_name,
                                display_name.as_deref(),
                                backups_path.as_path(),
                                &options,
                                token_clone,
                            )
                            .await
                    }
                },
            )
            .await?;

        ensure!(
            maybe_created.is_some(),
            "Nothing to back up for this app (selected parts: {})",
            parts
        );

        BackupsChanged {}.send_signal_to_dart();

        Ok(())
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_share_app(
        &self,
        package_name: String,
        display_name: Option<String>,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        ensure!(
            self.downloader_manager.is_some().await,
            "Downloader is not configured. Install configuration file to initialize."
        );

        debug!(
            package_name = %package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting app share task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        // Use downloads location as the base for temporary share directories and archives.
        let settings = self.settings.read().await.clone();
        let downloads_root = std::path::PathBuf::from(settings.downloads_location.clone());
        let upload_root = downloads_root.join("_upload");
        tokio::fs::create_dir_all(&upload_root).await.with_context(|| {
            format!("Failed to create upload directory {}", upload_root.display())
        })?;

        let pkg_for_pull = package_name.clone();
        let dest_root_clone = upload_root.clone();
        let pulled_dir = self
            .run_adb_one_step(
                AdbStepConfig {
                    step_number: 1,
                    waiting_msg: "Waiting to start pull from device...",
                    running_msg: "Pulling app from device...".to_string(),
                    log_context: "share_app_pull",
                },
                update_progress,
                token.clone(),
                move || {
                    let device = device.clone();
                    let pkg = pkg_for_pull.clone();
                    let dest_root = dest_root_clone.clone();
                    async move { device.pull_app_for_sharing(&pkg, &dest_root).await }
                },
            )
            .await?;

        if token.is_cancelled() {
            warn!("Task was cancelled after pull step");
            return Err(anyhow!("Task cancelled after pulling app from device"));
        }

        // Step 2: prepare archive (APK metadata, HWID file, archive name and ZIP).
        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number: 2,
            step_progress: None,
            message: "Preparing archive for upload...".into(),
        });

        let apk_path = pulled_dir.join(format!("{}.apk", package_name));
        let apk_info = get_apk_info(&apk_path)
            .with_context(|| format!("Failed to read APK metadata from {}", apk_path.display()))?;

        let label = apk_info
            .application_label
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .or(display_name.clone())
            .unwrap_or_else(|| package_name.clone());

        let version_code = apk_info.version_code.with_context(|| {
            format!("Failed to get version code from APK {}", apk_path.display())
        })?;

        let base_archive_name = format!("{label} v{version_code} {}", apk_info.package_name);
        let sanitized_name = sanitize_filename::sanitize(&base_archive_name);
        ensure!(!sanitized_name.is_empty(), "Sanitized archive name is empty");
        let archive_file_name = format!("{sanitized_name}.zip");

        let hwid_hex = {
            let digest = md5::compute(settings.installation_id.as_bytes());
            format!("{:X}", digest)
        };
        tokio::fs::write(pulled_dir.join("HWID.txt"), hwid_hex.as_bytes())
            .await
            .context("Failed to write HWID.txt")?;

        let archive_path =
            create_zip_from_dir(&pulled_dir, &upload_root, &archive_file_name, Some(token.clone()))
                .await
                .context("Failed to create archive from pulled app")?;

        if let Err(e) = tokio::fs::remove_dir_all(&pulled_dir).await {
            warn!(
                error = &e as &dyn Error,
                path = %pulled_dir.display(),
                "Failed to clean up pulled app directory after share"
            );
        }

        if token.is_cancelled() {
            warn!("Task was cancelled after archive preparation step");
            return Err(anyhow!("Task cancelled after preparing archive"));
        }

        // Step 3: upload archive via rclone.
        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number: 3,
            step_progress: None,
            message: "Uploading archive...".into(),
        });

        let downloader = self
            .downloader_manager
            .get()
            .await
            .ok_or_else(|| anyhow!("Downloader is not configured"))?;

        // TODO: add progress
        downloader
            .upload_shared_archive(&archive_path, token)
            .await
            .context("Failed to upload shared app archive")?;

        Ok(())
    }

    #[instrument(skip(self, update_progress, token))]
    async fn handle_restore(
        &self,
        backup_path: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            backup_path = %backup_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting restore task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backup_path_cloned = backup_path.clone();
        self.run_adb_one_step(
            AdbStepConfig {
                step_number: 1,
                waiting_msg: "Waiting to start restore...",
                running_msg: "Restoring backup...".to_string(),
                log_context: "restore",
            },
            update_progress,
            token,
            move || {
                let path = backup_path_cloned.clone();
                async move { adb_handler.restore_backup(&device, Path::new(&path)).await }
            },
        )
        .await
        .map(|_| ())
    }

    #[instrument(skip(self), fields(app_full_name = %app_full_name, app_path = %app_path), err)]
    async fn cleanup_downloads_after_install(
        &self,
        app_full_name: &str,
        app_path: &str,
    ) -> Result<()> {
        let settings = self.settings.read().await.clone();
        self.downloads_catalog
            .apply_cleanup_policy(settings.cleanup_policy, app_full_name, app_path)
            .await
    }
}

#[cfg(test)]
impl TaskManager {
    pub async fn __test_has_downloader(&self) -> bool {
        self.downloader_manager.is_some().await
    }
}

fn send_progress(progress: TaskProgress) {
    // Log significant status changes (not every progress update to avoid spam)
    match progress.status {
        TaskStatus::Waiting
        | TaskStatus::Completed
        | TaskStatus::Failed
        | TaskStatus::Cancelled => {
            debug!(
                task_id = progress.task_id,
                task_kind = ?progress.task_kind,
                task_name = ?progress.task_name,
                status = ?progress.status,
                progress = progress.total_progress,
                progress_message = %progress.message,
                "Sending progress signal to Dart"
            );
        }
        TaskStatus::Running => {
            // if progress.total_progress == 0.0
            //     || (0.25..0.26).contains(&progress.total_progress)
            //     || (0.5..0.51).contains(&progress.total_progress)
            //     || (0.75..0.76).contains(&progress.total_progress)
            // {
            //     debug!(
            //         task_id = progress.task_id,
            //         progress = progress.total_progress,
            //         "Task progress milestone"
            //     );
            // }
        }
    }

    progress.send_signal_to_dart();
}
