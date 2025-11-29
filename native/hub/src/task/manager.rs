use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    adb::{AdbHandler, PackageName},
    downloader::{downloads_catalog::DownloadsCatalog, manager::DownloaderManager},
    models::{
        Settings,
        signals::{
            system::Toast,
            task::{Task, TaskCancelRequest, TaskKind, TaskProgress, TaskRequest, TaskStatus},
        },
    },
    task::{BackupStepConfig, ProgressUpdate},
};

pub(crate) struct TaskManager {
    pub(super) adb_semaphore: Semaphore,
    pub(super) download_semaphore: Semaphore,
    id_counter: AtomicU64,
    tasks: Mutex<HashMap<u64, (Task, CancellationToken)>>,
    pub(super) adb_handler: Arc<AdbHandler>,
    pub(super) downloader_manager: Arc<DownloaderManager>,
    pub(super) downloads_catalog: Arc<DownloadsCatalog>,
    pub(super) settings: RwLock<Settings>,
}

impl TaskManager {
    pub(crate) fn new(
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

    #[instrument(level = "debug", skip(self))]
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

    #[instrument(level = "debug", skip(self))]
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

    #[instrument(level = "debug", skip(self))]
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

    #[instrument(level = "debug", skip(self, token))]
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

        let result = async {
            match &task {
                Task::Download(app, package) => {
                    info!(task_id = id, "Executing download task");
                    self.handle_download(
                        app.clone(),
                        PackageName::parse(package.clone())?,
                        &update_progress,
                        token.clone(),
                    )
                    .await
                }
                Task::DownloadInstall(app, package) => {
                    info!(task_id = id, "Executing download and install task");
                    self.handle_download_install(
                        app.clone(),
                        PackageName::parse(package.clone())?,
                        &update_progress,
                        token.clone(),
                    )
                    .await
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
                    async {
                        let package = PackageName::parse(package_name)?;
                        self.handle_uninstall(package, &update_progress, token.clone()).await
                    }
                    .await
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
                Task::DonateApp { package_name, display_name } => {
                    info!(task_id = id, "Executing app donation task");
                    async {
                        let package = PackageName::parse(package_name)?;
                        self.handle_donate_app(
                            package,
                            display_name.clone(),
                            &update_progress,
                            token.clone(),
                        )
                        .await
                    }
                    .await
                }
            }
        }
        .await;

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
