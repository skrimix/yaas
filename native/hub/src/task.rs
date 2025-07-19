use std::{
    error::Error,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use rinf::{DartSignal, RustSignal};
use tokio::sync::{Mutex, Semaphore, mpsc};
use tracing::error;

use crate::{
    adb::{AdbHandler, device::SideloadProgress},
    downloader::{Downloader, RcloneTransferStats},
    models::signals::{
        system::Toast,
        task::{TaskParams, TaskProgress, TaskRequest, TaskStatus, TaskType},
    },
};

pub struct TaskManager {
    adb_semaphore: Semaphore,
    download_semaphore: Semaphore,
    id_counter: AtomicU64,
    tasks: Mutex<Vec<u64>>,
    adb_handler: Arc<AdbHandler>,
    downloader: Arc<Downloader>,
}

impl TaskManager {
    pub fn new(adb_handler: Arc<AdbHandler>, downloader: Arc<Downloader>) -> Arc<Self> {
        let handle = Arc::new(Self {
            adb_semaphore: Semaphore::new(1),
            download_semaphore: Semaphore::new(1),
            id_counter: AtomicU64::new(0),
            tasks: Mutex::new(Vec::new()),
            adb_handler,
            downloader,
        });

        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_create_requests().await;
            }
        });

        handle
    }

    async fn receive_create_requests(self: Arc<Self>) {
        let receiver = TaskRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            self.clone().enqueue_task(request.message.task_type, request.message.params).await;
        }
    }

    async fn enqueue_task(self: Arc<Self>, task_type: TaskType, params: TaskParams) -> u64 {
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);

        tokio::spawn({
            let manager = self.clone();
            async move {
                manager.process_task(id, task_type, params).await;
            }
        });

        self.tasks.lock().await.push(id);

        id
    }

    async fn process_task(&self, id: u64, task_type: TaskType, params: TaskParams) {
        let task_name = match get_task_name(task_type, &params) {
            Ok(name) => name,
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Failed to get task name");
                send_progress(
                    id,
                    task_type,
                    None,
                    TaskStatus::Failed,
                    0.0,
                    format!("Failed to initialize task: {e:#}"),
                );
                return;
            }
        };

        let update_progress = |status: TaskStatus, progress: f32, message: String| {
            send_progress(id, task_type, Some(task_name.clone()), status, progress, message);
        };

        update_progress(TaskStatus::Waiting, 0.0, "Starting...".into());

        Toast::send(
            task_name.clone(),
            format!("{task_type}: starting"),
            false,
            Some(Duration::from_secs(2)),
        );

        let result = match task_type {
            TaskType::Download => self.handle_download(params, &update_progress).await,
            TaskType::DownloadInstall => {
                self.handle_download_install(params, &update_progress).await
            }
            TaskType::InstallApk => self.handle_install_apk(params, &update_progress).await,
            TaskType::InstallLocalApp => {
                self.handle_install_local_app(params, &update_progress).await
            }
            TaskType::Uninstall => self.handle_uninstall(params, &update_progress).await,
        };

        match result {
            Ok(_) => {
                update_progress(TaskStatus::Completed, 1.0, "Done".into());
                Toast::send(task_name, format!("{task_type}: completed"), false, None);
            }
            Err(e) => {
                error!(error = e.as_ref() as &dyn Error, "Task {} failed", task_name);
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

    async fn handle_download_install(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, f32, String),
    ) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start download...".into());
        let _download_permit = self.download_semaphore.acquire().await;
        update_progress(TaskStatus::Running, 0.0, "Starting download...".into());

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();

        let mut download_task = {
            let downloader = self.downloader.clone();
            let app_full_name = app_full_name.clone();
            tokio::spawn(async move { downloader.download_app(app_full_name, tx).await })
        };

        // Monitor progress while waiting for download to complete
        let mut download_result = None;
        while download_result.is_none() {
            tokio::select! {
                result = &mut download_task => {
                    download_result = Some(result.context("Download task failed")?.context("Failed to download app")?);
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;
                    update_progress(
                        TaskStatus::Running,
                        step_progress * 0.5, // Scaling to 1/2 to get total
                        format!("Downloading ({:.1}%) - {}/s", step_progress * 100.0, humansize::format_size(progress.speed, humansize::DECIMAL)),
                    );
                }
            }
        }

        let app_path = download_result.unwrap();
        drop(_download_permit);

        update_progress(TaskStatus::Waiting, 0.5, "Waiting to start installation...".into());
        let _adb_permit = self.adb_semaphore.acquire().await;
        update_progress(TaskStatus::Running, 0.75, "Installing app...".into());

        // self.adb_handler.sideload_app(Path::new(&app_path)).await?;

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();

        let mut sideload_task = {
            let adb_handler = self.adb_handler.clone();
            let app_path = app_path.clone();
            tokio::spawn(async move { adb_handler.sideload_app(Path::new(&app_path), tx).await })
        };

        let mut sideload_result = None;
        while sideload_result.is_none() {
            tokio::select! {
                result = &mut sideload_task => {
                    sideload_result = Some(result.context("Sideload task failed")?);
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.progress;
                    update_progress(TaskStatus::Running, 0.75 + step_progress * 0.25, progress.status);
                }
            }
        }

        sideload_result.unwrap()?;

        Ok(())
    }

    async fn handle_download(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, f32, String),
    ) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start download...".into());
        let _permit = self.download_semaphore.acquire().await;
        update_progress(TaskStatus::Running, 0.0, "Starting download...".into());

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();

        let download_task = {
            let downloader = self.downloader.clone();
            let app_full_name = app_full_name.clone();
            tokio::spawn(async move { downloader.download_app(app_full_name, tx).await })
        };

        tokio::select! {
            result = download_task => {
                result.context("Download task failed")?.context("Failed to download app")?;
                Ok(())
            }
            _ = async {
                while let Some(progress) = rx.recv().await {
                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;
                    update_progress(
                        TaskStatus::Running,
                        step_progress,
                        format!("Downloading ({:.1}%) - {}/s", step_progress * 100.0, humansize::format_size(progress.speed, humansize::DECIMAL)),
                    );
                }
            } => Ok(())
        }
    }

    async fn handle_install_apk(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, f32, String),
    ) -> Result<()> {
        let apk_path = params.apk_path.context("Missing apk_path parameter")?;

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start installation...".into());
        let _permit = self.adb_semaphore.acquire().await;
        update_progress(TaskStatus::Running, 0.0, "Installing APK...".into());

        let (tx, mut rx) = mpsc::unbounded_channel::<f32>();

        let mut install_task = {
            let adb_handler = self.adb_handler.clone();
            let apk_path = apk_path.clone();
            tokio::spawn(async move { adb_handler.install_apk(Path::new(&apk_path), tx).await })
        };

        let mut install_result = None;
        while install_result.is_none() {
            tokio::select! {
                result = &mut install_task => {
                    install_result = Some(result.context("Install task failed")?);
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress;
                    update_progress(TaskStatus::Running, step_progress, "Installing APK...".into());
                }
            }
        }
        install_result.unwrap()?;

        Ok(())
    }

    async fn handle_install_local_app(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, f32, String),
    ) -> Result<()> {
        let app_path = params.local_app_path.context("Missing local_app_path parameter")?;

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start installation...".into());
        let _permit = self.adb_semaphore.acquire().await;
        update_progress(TaskStatus::Running, 0.0, "Installing app...".into());

        // self.adb_handler.sideload_app(Path::new(&app_path)).await?;

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();

        let mut sideload_task = {
            let adb_handler = self.adb_handler.clone();
            let app_path = app_path.clone();
            tokio::spawn(async move { adb_handler.sideload_app(Path::new(&app_path), tx).await })
        };

        let mut sideload_result = None;
        while sideload_result.is_none() {
            tokio::select! {
                result = &mut sideload_task => {
                    sideload_result = Some(result.context("Sideload task failed")?);
                }
                Some(progress) = rx.recv() => {
                    let step_progress = progress.progress;
                    update_progress(TaskStatus::Running, step_progress, progress.status);
                }
            }
        }

        sideload_result.unwrap()?;
        Ok(())
    }

    async fn handle_uninstall(
        &self,
        params: TaskParams,
        update_progress: &impl Fn(TaskStatus, f32, String),
    ) -> Result<()> {
        let package_name = params.package_name.context("Missing package_name parameter")?;

        update_progress(TaskStatus::Waiting, 0.0, "Waiting to start uninstallation...".into());
        let _permit = self.adb_semaphore.acquire().await;
        update_progress(TaskStatus::Running, 0.0, "Uninstalling app...".into());

        self.adb_handler.uninstall_package(&package_name).await?;

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
