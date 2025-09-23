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
    downloader::{Downloader, RcloneTransferStats},
    models::{
        Settings,
        signals::{
            backups::BackupsChanged,
            system::Toast,
            task::{
                TaskCancelRequest, TaskParams, TaskProgress, TaskRequest, TaskStatus, TaskType,
            },
        },
    },
};

macro_rules! acquire_permit_or_cancel {
    ($semaphore:expr, $token:expr, $semaphore_name:literal) => {{
        if $token.is_cancelled() {
            warn!(concat!("Task already cancelled before ", $semaphore_name, " semaphore acquisition"));
            return Err(anyhow!(concat!("Task cancelled before ", $semaphore_name)));
        }

        debug!(concat!("Waiting for ", $semaphore_name, " semaphore"));
        tokio::select! {
            permit = $semaphore.acquire() => permit,
            _ = $token.cancelled() => {
                warn!(concat!("Task cancelled while waiting for ", $semaphore_name, " semaphore"));
                return Err(anyhow!(concat!("Task cancelled while waiting for ", $semaphore_name, " semaphore")));
            }
        }
    }};
}

pub struct TaskManager {
    adb_semaphore: Semaphore,
    download_semaphore: Semaphore,
    id_counter: AtomicU64,
    tasks: Mutex<HashMap<u64, (TaskType, CancellationToken)>>,
    adb_handler: Arc<AdbHandler>,
    downloader: Arc<Downloader>,
    settings: RwLock<Settings>,
}

impl TaskManager {
    pub fn new(
        adb_handler: Arc<AdbHandler>,
        downloader: Arc<Downloader>,
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
            downloader,
            settings: RwLock::new(initial_settings),
        });

        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_create_requests().await;
            }
        });

        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_cancel_requests().await;
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
    async fn receive_create_requests(self: Arc<Self>) {
        let receiver = TaskRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            self.clone().enqueue_task(request.message.task_type, request.message.params).await;
        }
    }

    #[instrument(skip(self))]
    async fn receive_cancel_requests(self: Arc<Self>) {
        let receiver = TaskCancelRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            let task_id = request.message.task_id;
            let tasks = self.tasks.lock().await;
            if let Some((task_type, token)) = tasks.get(&task_id) {
                info!(
                    task_id = task_id,
                    task_type = %task_type,
                    active_tasks = tasks.len(),
                    "Received cancellation request for task"
                );
                token.cancel();
                info!(task_id = task_id, "Cancellation signal sent to task");
            } else {
                warn!(
                    task_id = task_id,
                    active_tasks = tasks.len(),
                    "Task not found for cancellation - may have already completed"
                );
            }
        }
    }

    #[instrument(skip(self), fields(task_type = %task_type))]
    async fn enqueue_task(self: Arc<Self>, task_type: TaskType, params: TaskParams) -> u64 {
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();

        info!(
            task_id = id,
            task_type = %task_type,
            "Creating new task"
        );

        match &task_type {
            TaskType::Download | TaskType::DownloadInstall => {
                if let Some(app_name) = &params.cloud_app_full_name {
                    debug!(task_id = id, app_name = %app_name, "Task parameters");
                }
            }
            TaskType::InstallApk => {
                if let Some(apk_path) = &params.apk_path {
                    debug!(task_id = id, apk_path = %apk_path, "Task parameters");
                }
            }
            TaskType::InstallLocalApp => {
                if let Some(app_path) = &params.local_app_path {
                    debug!(task_id = id, app_path = %app_path, "Task parameters");
                }
            }
            TaskType::Uninstall => {
                if let Some(package_name) = &params.package_name {
                    debug!(task_id = id, package_name = %package_name, "Task parameters");
                }
            }
            TaskType::BackupApp => {
                if let Some(package_name) = &params.package_name {
                    debug!(task_id = id, package_name = %package_name, "Task parameters");
                }
            }
            TaskType::RestoreBackup => {
                if let Some(path) = &params.backup_path {
                    debug!(task_id = id, backup_path = %path, "Task parameters");
                }
            }
        }

        let mut tasks = self.tasks.lock().await;
        let active_tasks_count = tasks.len();
        tasks.insert(id, (task_type, token.clone()));
        drop(tasks);

        info!(task_id = id, active_tasks = active_tasks_count + 1, "Task added to queue");

        tokio::spawn({
            let manager = self.clone();
            async move {
                manager.process_task(id, task_type, params, token).await;
            }
        });

        id
    }

    #[instrument(skip(self, token), fields(task_type = %task_type))]
    async fn process_task(
        &self,
        id: u64,
        task_type: TaskType,
        params: TaskParams,
        token: CancellationToken,
    ) {
        let start_time = std::time::Instant::now();

        info!(
            task_id = id,
            task_type = %task_type,
            "Starting task processing"
        );

        let task_name = match get_task_name(task_type, &params) {
            Ok(name) => {
                info!(
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
                    task_type,
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
                self.tasks.lock().await.remove(&id);
                return;
            }
        };

        // Define step-aware progress reporting
        let total_steps: u32 = match task_type {
            TaskType::DownloadInstall => 2,
            _ => 1,
        };

        let update_progress =
            |status: TaskStatus, step_index: u32, step_progress: Option<f32>, message: String| {
                // debug!(
                //     task_id = id,
                //     status = ?status,
                //     step_index = step_index,
                //     step_progress = ?step_progress,
                //     message = %message,
                //     "Task progress update"
                // ); // TODO: limit logging frequency
                let safe_total = total_steps.max(1) as f32;
                let completed_steps = step_index.saturating_sub(1) as f32;
                let sp = step_progress.unwrap_or(0.0).clamp(0.0, 1.0);
                let total_progress = (completed_steps + sp) / safe_total;

                send_progress(TaskProgress {
                    task_id: id,
                    task_type,
                    task_name: Some(task_name.clone()),
                    status,
                    total_progress,
                    message,
                    current_step: step_index,
                    total_steps,
                    step_progress,
                });
            };

        update_progress(TaskStatus::Waiting, 1, None, "Starting...".into());

        Toast::send(
            task_name.clone(),
            format!("{task_type}: starting"),
            false,
            Some(Duration::from_secs(2)),
        );

        let result = match task_type {
            TaskType::Download => {
                info!(task_id = id, "Executing download task");
                self.handle_download(params, &update_progress, token.clone()).await
            }
            TaskType::DownloadInstall => {
                info!(task_id = id, "Executing download and install task");
                self.handle_download_install(params, &update_progress, token.clone()).await
            }
            TaskType::InstallApk => {
                info!(task_id = id, "Executing APK install task");
                self.handle_install_apk(params, &update_progress, token.clone()).await
            }
            TaskType::InstallLocalApp => {
                info!(task_id = id, "Executing local app install task");
                self.handle_install_local_app(params, &update_progress, token.clone()).await
            }
            TaskType::Uninstall => {
                info!(task_id = id, "Executing uninstall task");
                self.handle_uninstall(params, &update_progress, token.clone()).await
            }
            TaskType::BackupApp => {
                info!(task_id = id, "Executing backup task");
                self.handle_backup(params, &update_progress, token.clone()).await
            }
            TaskType::RestoreBackup => {
                info!(task_id = id, "Executing restore backup task");
                self.handle_restore(params, &update_progress, token.clone()).await
            }
        };

        let duration = start_time.elapsed();

        match result {
            Ok(_) => {
                info!(
                    task_id = id,
                    task_name = %task_name,
                    duration_ms = duration.as_millis(),
                    duration_secs = duration.as_secs_f64(),
                    "Task completed successfully"
                );
                update_progress(TaskStatus::Completed, total_steps, Some(1.0), "Done".into());
                Toast::send(task_name, format!("{task_type}: completed"), false, None);
            }
            Err(e) => {
                if token.is_cancelled() {
                    warn!(
                        task_id = id,
                        task_name = %task_name,
                        duration_ms = duration.as_millis(),
                        "Task was cancelled by user"
                    );
                    update_progress(
                        TaskStatus::Cancelled,
                        total_steps,
                        None,
                        "Task cancelled by user".into(),
                    );
                    Toast::send(task_name, format!("{task_type}: cancelled"), false, None);
                } else {
                    error!(
                        task_id = id,
                        task_name = %task_name,
                        duration_ms = duration.as_millis(),
                        error = e.as_ref() as &dyn Error,
                        error_chain = ?e.chain().collect::<Vec<_>>(),
                        "Task failed with error"
                    );
                    update_progress(
                        TaskStatus::Failed,
                        total_steps,
                        None,
                        format!("Task failed: {e:#}"),
                    );
                    Toast::send(
                        task_name,
                        format!("{task_type}: failed"),
                        true,
                        Some(Duration::from_secs(10)),
                    );
                }
            }
        }

        let final_tasks_count = {
            let mut tasks = self.tasks.lock().await;
            tasks.remove(&id);
            tasks.len()
        };

        info!(task_id = id, remaining_tasks = final_tasks_count, "Task removed from queue");
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_download_install(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        info!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting download and install task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start download...".into());

        let _download_permit =
            acquire_permit_or_cancel!(self.download_semaphore, token, "download");
        info!(
            download_permits_remaining = self.download_semaphore.available_permits(),
            "Acquired download semaphore"
        );

        update_progress(TaskStatus::Running, 1, None, "Starting download...".into());

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();

        let mut download_task = {
            let downloader = self.downloader.clone();
            let app_full_name = app_full_name.clone();
            let token = token.clone();
            tokio::spawn(
                async move { downloader.download_app(app_full_name, tx, token).await }
                    .instrument(Span::current()),
            )
        };

        // Monitor progress while waiting for download to complete
        info!("Starting download monitoring");
        let mut download_result = None;
        let mut last_log_time = std::time::Instant::now();
        let mut last_log_progress = 0.0;

        while download_result.is_none() {
            tokio::select! {
                result = &mut download_task => {
                    download_result = Some(result.context("Download task failed")?.context("Failed to download app")?);
                    info!("Download task completed");
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;

                    // Log download progress every 10 seconds or at major milestones
                    let now = std::time::Instant::now();
                    let should_log = now.duration_since(last_log_time) > Duration::from_secs(10) ||
                                   ((0.25..0.26).contains(&step_progress) ||
                                   (0.5..0.51).contains(&step_progress) ||
                                   (0.75..0.76).contains(&step_progress) && last_log_progress != step_progress);

                    if should_log {
                        info!(
                            bytes_downloaded = progress.bytes,
                            total_bytes = progress.total_bytes,
                            speed_bytes_per_sec = progress.speed,
                            progress_percent = step_progress * 100.0,
                            "Download progress"
                        );
                        last_log_time = now;
                        last_log_progress = step_progress;
                    }

                    update_progress(
                        TaskStatus::Running,
                        1,
                        Some(step_progress),
                        format!(
                            "Downloading ({:.1}%) - {}/s",
                            step_progress * 100.0,
                            humansize::format_size(progress.speed, humansize::DECIMAL)
                        ),
                    );
                }
            }
        }

        let app_path = download_result.unwrap();
        info!(
            app_path = %app_path,
            download_permits_released = self.download_semaphore.available_permits() + 1,
            "Download completed, releasing download semaphore"
        );
        drop(_download_permit);

        if token.is_cancelled() {
            warn!("Task was cancelled after download completion");
            return Err(anyhow!("Task cancelled after download"));
        }

        update_progress(TaskStatus::Waiting, 2, None, "Waiting to start installation...".into());

        let _adb_permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for installation"
        );

        update_progress(TaskStatus::Running, 2, None, "Installing app...".into());

        // self.adb_handler.sideload_app(Path::new(&app_path)).await?;

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();

        let mut sideload_task = {
            let adb_handler = self.adb_handler.clone();
            let app_path = app_path.clone();
            tokio::spawn(
                async move { adb_handler.sideload_app(Path::new(&app_path), tx).await }
                    .instrument(Span::current()),
            )
        };

        info!("Starting sideload monitoring");
        let mut sideload_result = None;
        let mut last_sideload_log = std::time::Instant::now();

        while sideload_result.is_none() {
            tokio::select! {
                result = &mut sideload_task => {
                    sideload_result = Some(result.context("Sideload task failed")?);
                    info!("Sideload task completed");
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.progress;

                    // Log installation progress every 5 seconds or at major milestones
                    let now = std::time::Instant::now();
                    if now.duration_since(last_sideload_log) > Duration::from_secs(5) {
                        info!(
                            sideload_progress = step_progress,
                            status = %progress.status,
                            "Installation progress"
                        );
                        last_sideload_log = now;
                    }

                    update_progress(TaskStatus::Running, 2, Some(step_progress), progress.status);
                }
            }
        }

        sideload_result.unwrap()?;

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Installation completed, releasing ADB semaphore"
        );

        // Apply downloads cleanup policy
        if let Err(e) = self.cleanup_downloads_after_install(&app_full_name, &app_path).await {
            // Non-fatal: log but do not fail the task
            warn!(
                error = e.as_ref() as &dyn Error,
                "Failed to apply downloads cleanup policy after install"
            );
        }

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_download(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        info!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            "Starting download task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start download...".into());

        let _permit = acquire_permit_or_cancel!(self.download_semaphore, token, "download");
        info!(
            download_permits_remaining = self.download_semaphore.available_permits(),
            "Acquired download semaphore"
        );

        update_progress(TaskStatus::Running, 1, None, "Starting download...".into());

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();

        let mut download_task = {
            let downloader = self.downloader.clone();
            let app_full_name = app_full_name.clone();
            tokio::spawn(
                async move { downloader.download_app(app_full_name, tx, token).await }
                    .instrument(Span::current()),
            )
        };

        info!("Starting download monitoring");
        let mut download_result = None;
        let mut last_log_time = std::time::Instant::now();

        while download_result.is_none() {
            tokio::select! {
                result = &mut download_task => {
                    download_result = Some(result.context("Download task failed")?.context("Failed to download app"));
                    info!("Download task completed");
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;

                    // Log download progress every 10 seconds or at major milestones
                    let now = std::time::Instant::now();
                    let should_log = now.duration_since(last_log_time) > Duration::from_secs(10) ||
                                   (0.25..0.26).contains(&step_progress) ||
                                   (0.5..0.51).contains(&step_progress) ||
                                   (0.75..0.76).contains(&step_progress);

                    if should_log {
                        info!(
                            bytes_downloaded = progress.bytes,
                            total_bytes = progress.total_bytes,
                            speed_bytes_per_sec = progress.speed,
                            progress_percent = step_progress * 100.0,
                            "Download progress"
                        );
                        last_log_time = now;
                    }

                    update_progress(
                        TaskStatus::Running,
                        1,
                        Some(step_progress),
                        format!(
                            "Downloading ({:.1}%) - {}/s",
                            step_progress * 100.0,
                            humansize::format_size(progress.speed, humansize::DECIMAL)
                        ),
                    );
                }
            }
        }

        download_result.unwrap()?;

        info!(
            download_permits_released = self.download_semaphore.available_permits() + 1,
            "Download completed, releasing download semaphore"
        );

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_install_apk(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let apk_path = params.apk_path.context("Missing apk_path parameter")?;

        info!(
            apk_path = %apk_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting APK install task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start installation...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for APK installation"
        );

        update_progress(TaskStatus::Running, 1, None, "Installing APK...".into());

        let (tx, mut rx) = mpsc::unbounded_channel::<f32>();

        let mut install_task = {
            let adb_handler = self.adb_handler.clone();
            let apk_path = apk_path.clone();
            tokio::spawn(
                async move { adb_handler.install_apk(Path::new(&apk_path), tx).await }
                    .instrument(Span::current()),
            )
        };

        info!("Starting APK install monitoring");
        let mut install_result = None;
        let mut last_log_time = std::time::Instant::now();

        while install_result.is_none() {
            tokio::select! {
                result = &mut install_task => {
                    install_result = Some(result.context("Install task failed")?);
                    info!("APK install task completed");
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress;

                    // Log install progress every 5 seconds or at major milestones
                    let now = std::time::Instant::now();
                    if now.duration_since(last_log_time) > Duration::from_secs(5) {
                        info!(
                            install_progress = step_progress,
                            "APK installation progress"
                        );
                        last_log_time = now;
                    }

                    update_progress(TaskStatus::Running, 1, Some(step_progress), "Installing APK...".into());
                }
            }
        }
        install_result.unwrap()?;

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "APK installation completed, releasing ADB semaphore"
        );

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_install_local_app(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let app_path = params.local_app_path.context("Missing local_app_path parameter")?;

        info!(
            app_path = %app_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting local app install task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start installation...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for local app installation"
        );

        update_progress(TaskStatus::Running, 1, None, "Installing app...".into());

        // self.adb_handler.sideload_app(Path::new(&app_path)).await?;

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();

        let mut sideload_task = {
            let adb_handler = self.adb_handler.clone();
            let app_path = app_path.clone();
            tokio::spawn(
                async move { adb_handler.sideload_app(Path::new(&app_path), tx).await }
                    .instrument(Span::current()),
            )
        };

        info!("Starting local app sideload monitoring");
        let mut sideload_result = None;
        let mut last_sideload_log = std::time::Instant::now();

        while sideload_result.is_none() {
            tokio::select! {
                result = &mut sideload_task => {
                    sideload_result = Some(result.context("Sideload task failed")?);
                    info!("Local app sideload task completed");
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.progress;

                    // Log installation progress every 5 seconds or at major milestones
                    let now = std::time::Instant::now();
                    if now.duration_since(last_sideload_log) > Duration::from_secs(5) {
                        info!(
                            sideload_progress = step_progress,
                            status = %progress.status,
                            "Local app installation progress"
                        );
                        last_sideload_log = now;
                    }

                    update_progress(TaskStatus::Running, 1, Some(step_progress), progress.status);
                }
            }
        }

        sideload_result.unwrap()?;

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Local app installation completed, releasing ADB semaphore"
        );

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_uninstall(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let package_name = params.package_name.context("Missing package_name parameter")?;

        info!(
            package_name = %package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting uninstall task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start uninstallation...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for uninstallation"
        );

        update_progress(TaskStatus::Running, 1, None, "Uninstalling app...".into());

        info!(package_name = %package_name, "Starting uninstall operation");
        self.adb_handler.uninstall_package(&package_name).await?;
        info!(package_name = %package_name, "Uninstall operation completed");

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Uninstallation completed, releasing ADB semaphore"
        );

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_backup(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let package_name = params.package_name.context("Missing package_name parameter")?;

        let backup_apk = params.backup_apk.unwrap_or(false);
        let backup_data = params.backup_data.unwrap_or(false);
        let backup_obb = params.backup_obb.unwrap_or(false);

        ensure!(backup_apk || backup_data || backup_obb, "No parts selected to backup");

        info!(
            package_name = %package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting backup task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start backup...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for backup"
        );

        let parts = [
            if backup_data { Some("data") } else { None },
            if backup_apk { Some("apk") } else { None },
            if backup_obb { Some("obb") } else { None },
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");

        update_progress(TaskStatus::Running, 1, None, format!("Creating backup ({parts})..."));

        let backups_dir = {
            let s = self.settings.read().await;
            s.backups_location.clone()
        };

        let backups_path = Path::new(&backups_dir);
        info!(path = %backups_path.display(), "Using backups location");

        // Build options from params
        let options = BackupOptions {
            name_append: params.backup_name_append.clone(),
            backup_apk,
            backup_data,
            backup_obb,
        };

        let maybe_created = self
            .adb_handler
            .backup_app(&package_name, params.display_name.as_deref(), backups_path, &options)
            .await?;

        ensure!(
            maybe_created.is_some(),
            "Nothing to back up for this app (selected parts: {})",
            parts
        );

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Backup completed, releasing ADB semaphore"
        );

        BackupsChanged {}.send_signal_to_dart();

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_restore(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, u32, Option<f32>, String),
        token: CancellationToken,
    ) -> Result<()> {
        let backup_path = params.backup_path.context("Missing backup_path parameter")?;

        info!(
            backup_path = %backup_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting restore task"
        );

        update_progress(TaskStatus::Waiting, 1, None, "Waiting to start restore...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for restore"
        );

        update_progress(TaskStatus::Running, 1, None, "Restoring backup...".into());

        self.adb_handler.restore_backup(Path::new(&backup_path)).await?;

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Restore completed, releasing ADB semaphore"
        );

        Ok(())
    }
}

impl TaskManager {
    #[instrument(skip(self), fields(app_full_name = %app_full_name, app_path = %app_path), err)]
    async fn cleanup_downloads_after_install(
        &self,
        app_full_name: &str,
        app_path: &str,
    ) -> Result<()> {
        use crate::models::DownloadCleanupPolicy as Policy;

        let settings = self.settings.read().await.clone();
        match settings.cleanup_policy {
            Policy::KeepAllVersions => {
                info!("Cleanup policy: keep all versions; nothing to do");
                return Ok(());
            }
            Policy::DeleteAfterInstall => {
                info!("Cleanup policy: delete after install; removing downloaded directory");
                let path = std::path::Path::new(app_path);
                if path.exists() {
                    tokio::fs::remove_dir_all(path).await.with_context(|| {
                        format!("Failed to remove downloaded directory: {}", path.display())
                    })?;
                    info!(removed = %path.display(), "Removed downloaded directory after install");
                } else {
                    debug!(missing = %path.display(), "Downloaded directory no longer exists");
                }
                return Ok(());
            }
            Policy::KeepOneVersion | Policy::KeepTwoVersions => {
                let keep_count = match settings.cleanup_policy {
                    Policy::KeepOneVersion => 1,
                    Policy::KeepTwoVersions => 2,
                    _ => unreachable!(),
                };

                // Try to resolve package and versions from cached cloud apps
                let downloader = self.downloader.clone();
                let Some(installed_app) = downloader.get_app_by_full_name(app_full_name).await
                else {
                    warn!(
                        "Installed app not found in cloud apps cache; skipping versioned cleanup"
                    );
                    return Ok(());
                };

                let mut versions =
                    downloader.get_apps_by_package(&installed_app.package_name).await;
                // Sort by version code (desc)
                versions.sort_by_key(|a| std::cmp::Reverse(a.version_code));

                // Build keep-set: always include installed version, then highest others
                let mut keep = vec![installed_app.full_name.clone()];
                for app in versions.into_iter() {
                    if keep.len() >= keep_count {
                        break;
                    }
                    if app.full_name != installed_app.full_name {
                        keep.push(app.full_name);
                    }
                }

                let downloads_dir = downloader.get_download_dir().await;
                info!(
                    keep_count,
                    package = %installed_app.package_name,
                    keep = ?keep,
                    downloads_dir = %downloads_dir.display(),
                    "Applying versioned downloads cleanup"
                );

                // Iterate known versions for the package and delete those not in keep set if present on disk
                for name in self
                    .downloader
                    .get_apps_by_package(&installed_app.package_name)
                    .await
                    .into_iter()
                    .map(|a| a.full_name)
                {
                    if keep.iter().any(|k| k == &name) {
                        continue;
                    }
                    let candidate = downloads_dir.join(&name);
                    if candidate.exists() {
                        info!(remove = %candidate.display(), "Removing older downloaded version");
                        if let Err(err) = tokio::fs::remove_dir_all(&candidate).await {
                            warn!(
                                error = &err as &dyn Error,
                                path = %candidate.display(),
                                "Failed to remove older downloaded version"
                            );
                        }
                    }
                }

                Ok(())
            }
        }
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
                task_type = %progress.task_type,
                task_name = ?progress.task_name,
                status = ?progress.status,
                progress = progress.total_progress,
                message = %progress.message,
                "Sending progress signal to Dart"
            ); // FIXME: doesn't show up on logs screen?
        }
        TaskStatus::Running => {
            // Only log running status at major milestones to avoid log spam
            if progress.total_progress == 0.0
                || (0.25..0.26).contains(&progress.total_progress)
                || (0.5..0.51).contains(&progress.total_progress)
                || (0.75..0.76).contains(&progress.total_progress)
            {
                debug!(
                    task_id = progress.task_id,
                    progress = progress.total_progress,
                    "Task progress milestone"
                );
            }
        }
    }

    progress.send_signal_to_dart();
}

fn get_task_name(task_type: TaskType, params: &TaskParams) -> Result<String> {
    Ok(match task_type {
        TaskType::Download | TaskType::DownloadInstall => params
            .cloud_app_full_name
            .as_ref()
            .context("Missing cloud_app_full_name parameter")?
            .clone(),
        TaskType::InstallApk => {
            Path::new(&params.apk_path.as_ref().context("Missing apk_path parameter")?)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        }
        TaskType::InstallLocalApp => {
            Path::new(&params.local_app_path.as_ref().context("Missing local_app_path parameter")?)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        }
        TaskType::Uninstall => {
            if let Some(name) = &params.display_name {
                name.clone()
            } else {
                params.package_name.as_ref().context("Missing package_name parameter")?.clone()
            }
        }
        TaskType::BackupApp => {
            if let Some(name) = &params.display_name {
                name.clone()
            } else {
                params.package_name.as_ref().context("Missing package_name parameter")?.clone()
            }
        }
        TaskType::RestoreBackup => {
            Path::new(&params.backup_path.as_ref().context("Missing backup_path parameter")?)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        }
    })
}
