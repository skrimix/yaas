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
use tokio::{
    sync::{Mutex, Notify, RwLock, Semaphore},
    time::timeout,
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    adb::{AdbService, PackageName},
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
    tasks: Mutex<TaskRegistry>,
    tasks_changed: Notify,
    shutdown_token: CancellationToken,
    pub(super) adb_service: Arc<AdbService>,
    pub(super) downloader_manager: Arc<DownloaderManager>,
    pub(super) downloads_catalog: Arc<DownloadsCatalog>,
    pub(super) settings: RwLock<Settings>,
}

struct TaskRegistry {
    accepting_tasks: bool,
    tasks: HashMap<u64, (Task, CancellationToken)>,
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self { accepting_tasks: true, tasks: HashMap::new() }
    }
}

impl TaskRegistry {
    fn insert(&mut self, id: u64, task: Task, token: CancellationToken) -> bool {
        if !self.accepting_tasks {
            debug!(task_id = id, "Ignoring task because shutdown has started");
            return false;
        }

        self.tasks.insert(id, (task, token));
        true
    }

    fn start_shutdown(&mut self) -> usize {
        self.accepting_tasks = false;
        for (_, token) in self.tasks.values() {
            token.cancel();
        }
        self.tasks.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskShutdownResult {
    pub(crate) timed_out: bool,
    pub(crate) remaining_tasks: usize,
}

impl TaskManager {
    pub(crate) fn new(
        adb_service: Arc<AdbService>,
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
            tasks: Mutex::new(TaskRegistry::default()),
            tasks_changed: Notify::new(),
            shutdown_token: CancellationToken::new(),
            adb_service,
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
                loop {
                    tokio::select! {
                        _ = handle.shutdown_token.cancelled() => break,
                        settings = stream.next() => {
                            if let Some(settings) = settings {
                                *handle.settings.write().await = settings;
                            } else {
                                break;
                            }
                        }
                    }
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
                _ = self.shutdown_token.cancelled() => {
                    debug!("Stopping task request handler");
                    break;
                }
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
    async fn enqueue_task(self: Arc<Self>, task: Task) -> Option<u64> {
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();

        debug!(task_id = id, task = ?task, "Creating new task");

        let mut registry = self.tasks.lock().await;
        let active_tasks_count = registry.tasks.len();
        if !registry.insert(id, task.clone(), token.clone()) {
            return None;
        }
        drop(registry);

        debug!(task_id = id, active_tasks = active_tasks_count + 1, "Task added to queue");

        tokio::spawn({
            let handle = self.clone();
            async move {
                handle.process_task(id, task, token).await;

                let mut registry = handle.tasks.lock().await;
                registry.tasks.remove(&id);
                let remaining_tasks = registry.tasks.len();
                drop(registry);
                handle.tasks_changed.notify_one();
                debug!(task_id = id, remaining_tasks = remaining_tasks, "Task removed from queue");
            }
        });

        Some(id)
    }

    #[instrument(level = "debug", skip(self))]
    async fn cancel_task(self: Arc<Self>, task_id: u64) {
        let registry = self.tasks.lock().await;
        if let Some((task, token)) = registry.tasks.get(&task_id) {
            info!(
                task_id = task_id,
                task = %task,
                active_tasks = registry.tasks.len(),
                "Received cancellation request for task"
            );
            token.cancel();
        } else {
            warn!(
                task_id = task_id,
                active_tasks = registry.tasks.len(),
                "Task not found for cancellation - may have already completed"
            );
        }
    }

    pub(crate) async fn shutdown(&self, wait_timeout: Duration) -> TaskShutdownResult {
        let active_tasks = {
            let mut registry = self.tasks.lock().await;
            registry.start_shutdown()
        };
        self.shutdown_token.cancel();

        info!(active_tasks, "Cancelling tasks for application shutdown");
        let result = wait_for_tasks(&self.tasks, &self.tasks_changed, wait_timeout).await;

        if result.timed_out {
            warn!(
                remaining_tasks = result.remaining_tasks,
                timeout_secs = wait_timeout.as_secs(),
                "Timed out waiting for tasks to stop"
            );
        } else {
            info!("All tasks stopped before application shutdown");
        }

        result
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
                        message: "Cancelled".into(),
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

async fn wait_for_tasks(
    tasks: &Mutex<TaskRegistry>,
    tasks_changed: &Notify,
    wait_timeout: Duration,
) -> TaskShutdownResult {
    let wait = async {
        loop {
            let notified = tasks_changed.notified();
            let remaining_tasks = tasks.lock().await.tasks.len();
            if remaining_tasks == 0 {
                return;
            }
            notified.await;
        }
    };

    let timed_out = timeout(wait_timeout, wait).await.is_err();
    let remaining_tasks = tasks.lock().await.tasks.len();
    TaskShutdownResult { timed_out, remaining_tasks }
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

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::sync::{Mutex, Notify};
    use tokio_util::sync::CancellationToken;

    use super::{TaskRegistry, wait_for_tasks};
    use crate::models::signals::task::Task;

    fn task(name: &str) -> Task {
        Task::Download(name.to_string(), "com.example.app".to_string())
    }

    #[test]
    fn shutdown_cancels_all_tasks() {
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        let mut registry = TaskRegistry::default();
        assert!(registry.insert(1, task("First"), first.clone()));
        assert!(registry.insert(2, task("Second"), second.clone()));

        assert_eq!(registry.start_shutdown(), 2);
        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
    }

    #[test]
    fn shutdown_rejects_new_tasks() {
        let mut registry = TaskRegistry::default();
        registry.start_shutdown();

        assert!(!registry.insert(1, task("Late"), CancellationToken::new()));
        assert!(registry.tasks.is_empty());
    }

    #[tokio::test]
    async fn shutdown_waits_for_tasks_to_finish() {
        let token = CancellationToken::new();
        let tasks = Arc::new(Mutex::new(TaskRegistry::default()));
        let tasks_changed = Arc::new(Notify::new());
        assert!(tasks.lock().await.insert(1, task("Running"), token.clone()));
        tasks.lock().await.start_shutdown();

        let worker_tasks = tasks.clone();
        let worker_tasks_changed = tasks_changed.clone();
        tokio::spawn(async move {
            token.cancelled().await;
            worker_tasks.lock().await.tasks.remove(&1);
            worker_tasks_changed.notify_one();
        });

        let result = wait_for_tasks(&tasks, &tasks_changed, Duration::from_secs(1)).await;
        assert!(!result.timed_out);
        assert_eq!(result.remaining_tasks, 0);
    }

    #[tokio::test]
    async fn shutdown_reports_tasks_left_after_timeout() {
        let tasks = Mutex::new(TaskRegistry::default());
        let tasks_changed = Notify::new();
        assert!(tasks.lock().await.insert(1, task("Stuck"), CancellationToken::new()));

        let result = wait_for_tasks(&tasks, &tasks_changed, Duration::from_millis(10)).await;
        assert!(result.timed_out);
        assert_eq!(result.remaining_tasks, 1);
    }

    #[tokio::test]
    async fn shutdown_does_not_miss_early_completion() {
        let tasks = Mutex::new(TaskRegistry::default());
        let tasks_changed = Notify::new();
        tasks_changed.notify_one();

        let result = wait_for_tasks(&tasks, &tasks_changed, Duration::from_millis(10)).await;
        assert!(!result.timed_out);
        assert_eq!(result.remaining_tasks, 0);
    }
}
