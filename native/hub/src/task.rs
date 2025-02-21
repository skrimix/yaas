use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result};
use tokio::sync::{Mutex, Semaphore};
use tracing::error;

use crate::{
    adb::AdbHandler,
    downloader::{DirDownloadProgress, Downloader},
    messages::{TaskParams, TaskProgress, TaskRequest, TaskStatus, TaskType},
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
            if let (Ok(task_type), Some(params)) =
                (TaskType::try_from(request.message.r#type), request.message.params)
            {
                Self::enqueue_task(self.clone(), task_type, params).await;
            } else {
                error!("Received invalid task request from Dart");
            }
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
        // Send initial task data
        let task_name = get_task_name(task_type, &params).unwrap_or("Unknown".to_string());
        send_progress(
            id,
            task_type,
            Some(task_name.clone()),
            TaskStatus::Waiting,
            0.0,
            "Initializing...".into(),
        );

        let result = match task_type {
            TaskType::Unspecified => {
                error!("Received unspecified task type from Dart");
                return;
            }
            TaskType::Download => self.handle_download(id, params).await,
            TaskType::DownloadInstall => self.handle_download_install(id, params).await,
            TaskType::InstallApk => self.handle_install_apk(id, params).await,
            TaskType::InstallLocalApp => self.handle_install_local_app(id, params).await,
            TaskType::Uninstall => self.handle_uninstall(id, params).await,
        };

        match result {
            Ok(_) => send_progress(
                id,
                task_type,
                Some(task_name),
                TaskStatus::Completed,
                1.0,
                "Done".into(),
            ),
            Err(e) => send_progress(
                id,
                task_type,
                Some(task_name),
                TaskStatus::Failed,
                0.0,
                e.to_string(),
            ),
        }
    }

    async fn handle_download_install(&self, id: u64, params: TaskParams) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        send_progress(
            id,
            TaskType::DownloadInstall,
            None,
            TaskStatus::Waiting,
            0.0,
            "Waiting to start download...".into(),
        );

        let _download_permit = self.download_semaphore.acquire().await;

        send_progress(
            id,
            TaskType::DownloadInstall,
            None,
            TaskStatus::Running,
            0.0,
            "Starting download...".into(),
        );

        // TODO: use this channel to send progress to Dart
        let (_tx, mut _rx) = tokio::sync::mpsc::unbounded_channel::<DirDownloadProgress>();

        let download_result = self.downloader.download_app(app_full_name).await;

        let app_path = match download_result {
            Ok(path) => path,
            Err(e) => return Err(e),
        };

        drop(_download_permit);

        send_progress(
            id,
            TaskType::DownloadInstall,
            None,
            TaskStatus::Waiting,
            0.0,
            "Waiting to start installation...".into(),
        );

        let _adb_permit = self.adb_semaphore.acquire().await;

        send_progress(
            id,
            TaskType::DownloadInstall,
            None,
            TaskStatus::Running,
            0.0,
            "Installing app...".into(),
        );

        self.adb_handler.sideload_app(Path::new(&app_path)).await?;

        Ok(())
    }

    async fn handle_download(&self, id: u64, params: TaskParams) -> Result<()> {
        let app_full_name =
            params.cloud_app_full_name.context("Missing cloud_app_full_name parameter")?;

        send_progress(
            id,
            TaskType::Download,
            None,
            TaskStatus::Waiting,
            0.0,
            "Waiting to start download...".into(),
        );

        let _permit = self.download_semaphore.acquire().await;

        send_progress(
            id,
            TaskType::Download,
            None,
            TaskStatus::Running,
            0.0,
            "Starting download...".into(),
        );

        // TODO: use this channel to send progress to Dart
        let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DirDownloadProgress>();

        let download_task = tokio::spawn({
            let downloader = self.downloader.clone();
            let app_full_name = app_full_name.clone();
            async move { downloader.download_app(app_full_name).await }
        });

        // Monitor download progress
        tokio::select! {
            result = download_task => {
                match result {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(e.into()),
                }
            }
            _ = async {
                while let Some(progress) = rx.recv().await {
                    let step_progress = progress.downloaded_bytes as f32 / progress.total_bytes as f32;
                    let message = format!(
                        "Downloaded {}/{} files ({:.1}%)",
                        progress.downloaded_files,
                        progress.total_files,
                        step_progress * 100.0
                    );
                    send_progress(id, TaskType::Download, None, TaskStatus::Running, step_progress, message);
                }
            } => {
                Ok(())
            }
        }
    }

    async fn handle_install_apk(&self, id: u64, params: TaskParams) -> Result<()> {
        let apk_path = params.apk_path.context("Missing apk_path parameter")?;

        send_progress(
            id,
            TaskType::InstallApk,
            None,
            TaskStatus::Waiting,
            0.0,
            "Waiting to start installation...".into(),
        );

        let _permit = self.adb_semaphore.acquire().await;

        send_progress(
            id,
            TaskType::InstallApk,
            None,
            TaskStatus::Running,
            0.0,
            "Installing APK...".into(),
        );

        self.adb_handler.install_apk(Path::new(&apk_path)).await?;

        Ok(())
    }

    async fn handle_install_local_app(&self, id: u64, params: TaskParams) -> Result<()> {
        let app_path = params.local_app_path.context("Missing local_app_path parameter")?;

        send_progress(
            id,
            TaskType::InstallLocalApp,
            None,
            TaskStatus::Waiting,
            0.0,
            "Waiting to start installation...".into(),
        );

        let _permit = self.adb_semaphore.acquire().await;

        send_progress(
            id,
            TaskType::InstallLocalApp,
            None,
            TaskStatus::Running,
            0.0,
            "Installing app...".into(),
        );

        self.adb_handler.sideload_app(Path::new(&app_path)).await?;

        Ok(())
    }

    async fn handle_uninstall(&self, id: u64, params: TaskParams) -> Result<()> {
        let package_name = params.package_name.context("Missing package_name parameter")?;

        send_progress(
            id,
            TaskType::Uninstall,
            None,
            TaskStatus::Waiting,
            0.0,
            "Waiting to start uninstallation...".into(),
        );

        let _permit = self.adb_semaphore.acquire().await;

        send_progress(
            id,
            TaskType::Uninstall,
            None,
            TaskStatus::Running,
            0.0,
            "Uninstalling app...".into(),
        );

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
    TaskProgress {
        task_id,
        r#type: task_type as i32,
        task_name,
        status: status as i32,
        total_progress,
        message,
    }
    .send_signal_to_dart();
}

fn get_task_name(task_type: TaskType, params: &TaskParams) -> Result<String> {
    Ok(match task_type {
        TaskType::Unspecified => "Unspecified".to_string(),
        TaskType::Download => params
            .cloud_app_full_name
            .as_ref()
            .context("Missing cloud_app_full_name parameter")?
            .clone(),
        TaskType::DownloadInstall => params
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
