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

use anyhow::{Context, Result, anyhow};
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, instrument, warn};

use crate::{
    adb::{AdbHandler, device::SideloadProgress},
    downloader::{Downloader, RcloneTransferStats},
    models::signals::{
        system::Toast,
        task::{TaskCancelRequest, TaskParams, TaskProgress, TaskRequest, TaskStatus, TaskType},
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
}

impl TaskManager {
    pub fn new(adb_handler: Arc<AdbHandler>, downloader: Arc<Downloader>) -> Arc<Self> {
        let handle = Arc::new(Self {
            adb_semaphore: Semaphore::new(1),
            download_semaphore: Semaphore::new(1),
            id_counter: AtomicU64::new(0),
            tasks: Mutex::new(HashMap::new()),
            adb_handler,
            downloader,
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

    #[instrument(skip(self, params, token), fields(task_id = id, task_type = %task_type))]
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
                send_progress(
                    id,
                    task_type,
                    None,
                    TaskStatus::Failed,
                    0.0,
                    format!("Failed to initialize task: {e:#}"),
                );

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

        let update_progress = |status: TaskStatus, progress: f32, message: String| {
            // debug!(
            //     task_id = id,
            //     status = ?status,
            //     progress = progress,
            //     message = %message,
            //     "Task progress update"
            // ); // TODO: limit logging frequency
            send_progress(id, task_type, Some(task_name.clone()), status, progress, message);
        };

        update_progress(TaskStatus::Waiting, 0.0, "Starting...".into());

        info!(
            task_id = id,
            task_name = %task_name,
            "Task entering execution phase"
        );

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
                update_progress(TaskStatus::Completed, 1.0, "Done".into());
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
                    update_progress(TaskStatus::Cancelled, 0.0, "Task cancelled by user".into());
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
                    update_progress(TaskStatus::Failed, 0.0, format!("Task failed: {e:#}"));
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
        update_progress: &impl Fn(TaskStatus, f32, String),
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

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start download...".into());

        let _download_permit =
            acquire_permit_or_cancel!(self.download_semaphore, token, "download");
        info!(
            download_permits_remaining = self.download_semaphore.available_permits(),
            "Acquired download semaphore"
        );

        update_progress(TaskStatus::Running, 0.0, "Starting download...".into());

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
                        step_progress * 0.5, // Scaling to 1/2 to get total
                        format!("Downloading ({:.1}%) - {}/s", step_progress * 100.0, humansize::format_size(progress.speed, humansize::DECIMAL)),
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

        update_progress(TaskStatus::Waiting, 0.5, "Waiting to start installation...".into());

        let _adb_permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for installation"
        );

        update_progress(TaskStatus::Running, 0.75, "Installing app...".into());

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

                    update_progress(TaskStatus::Running, 0.75 + step_progress * 0.25, progress.status);
                }
            }
        }

        sideload_result.unwrap()?;

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Installation completed, releasing ADB semaphore"
        );

        Ok(())
    }

    #[instrument(skip(self, params, update_progress, token))]
    async fn handle_download(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, f32, String),
        token: CancellationToken,
    ) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        info!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            "Starting download task"
        );

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start download...".into());

        let _permit = acquire_permit_or_cancel!(self.download_semaphore, token, "download");
        info!(
            download_permits_remaining = self.download_semaphore.available_permits(),
            "Acquired download semaphore"
        );

        update_progress(TaskStatus::Running, 0.0, "Starting download...".into());

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
                        step_progress,
                        format!("Downloading ({:.1}%) - {}/s", step_progress * 100.0, humansize::format_size(progress.speed, humansize::DECIMAL)),
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
        update_progress: &impl Fn(TaskStatus, f32, String),
        token: CancellationToken,
    ) -> Result<()> {
        let apk_path = params.apk_path.context("Missing apk_path parameter")?;

        info!(
            apk_path = %apk_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting APK install task"
        );

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start installation...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for APK installation"
        );

        update_progress(TaskStatus::Running, 0.0, "Installing APK...".into());

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

                    update_progress(TaskStatus::Running, step_progress, "Installing APK...".into());
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
        update_progress: &impl Fn(TaskStatus, f32, String),
        token: CancellationToken,
    ) -> Result<()> {
        let app_path = params.local_app_path.context("Missing local_app_path parameter")?;

        info!(
            app_path = %app_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting local app install task"
        );

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start installation...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for local app installation"
        );

        update_progress(TaskStatus::Running, 0.0, "Installing app...".into());

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

                    update_progress(TaskStatus::Running, step_progress, progress.status);
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
        update_progress: &impl Fn(TaskStatus, f32, String),
        token: CancellationToken,
    ) -> Result<()> {
        let package_name = params.package_name.context("Missing package_name parameter")?;

        info!(
            package_name = %package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting uninstall task"
        );

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start uninstallation...".into());

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        info!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for uninstallation"
        );

        update_progress(TaskStatus::Running, 0.0, "Uninstalling app...".into());

        info!(package_name = %package_name, "Starting uninstall operation");
        self.adb_handler.uninstall_package(&package_name).await?;
        info!(package_name = %package_name, "Uninstall operation completed");

        info!(
            adb_permits_released = self.adb_semaphore.available_permits() + 1,
            "Uninstallation completed, releasing ADB semaphore"
        );

        Ok(())
    }
}

fn send_progress(
    task_id: u64,
    task_type: TaskType,
    task_name: Option<String>,
    status: TaskStatus,
    total_progress: f32,
    message: String,
) {
    // Log significant status changes (not every progress update to avoid spam)
    match status {
        TaskStatus::Waiting
        | TaskStatus::Completed
        | TaskStatus::Failed
        | TaskStatus::Cancelled => {
            debug!(
                task_id = task_id,
                task_type = %task_type,
                task_name = ?task_name,
                status = ?status,
                progress = total_progress,
                message = %message,
                "Sending progress signal to Dart"
            );
        }
        TaskStatus::Running => {
            // Only log running status at major milestones to avoid log spam
            if total_progress == 0.0
                || (0.25..0.26).contains(&total_progress)
                || (0.5..0.51).contains(&total_progress)
                || (0.75..0.76).contains(&total_progress)
            {
                debug!(task_id = task_id, progress = total_progress, "Task progress milestone");
            }
        }
    }

    TaskProgress { task_id, task_type, task_name, status, total_progress, message }
        .send_signal_to_dart();
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
            params.package_name.as_ref().context("Missing package_name parameter")?.clone()
        }
    })
}
