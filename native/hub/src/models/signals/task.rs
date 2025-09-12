use core::fmt;
use std::fmt::Display;

use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, SignalPiece)]
pub enum TaskType {
    Download,
    DownloadInstall,
    InstallApk,
    InstallLocalApp,
    Uninstall,
    BackupApp,
    RestoreBackup,
}

impl Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskType::Download => write!(f, "Download"),
            TaskType::DownloadInstall => write!(f, "Download & Install"),
            TaskType::InstallApk => write!(f, "Install APK"),
            TaskType::InstallLocalApp => write!(f, "Install Local App"),
            TaskType::Uninstall => write!(f, "Uninstall"),
            TaskType::BackupApp => write!(f, "Backup App"),
            TaskType::RestoreBackup => write!(f, "Restore Backup"),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, SignalPiece)]
pub enum TaskStatus {
    Waiting,
    Running,
    Completed,
    Failed,
    Cancelled,
}

// TODO: make enum?
#[derive(Clone, Serialize, Deserialize, Debug, SignalPiece)]
pub struct TaskParams {
    pub cloud_app_full_name: Option<String>,
    pub apk_path: Option<String>,
    pub local_app_path: Option<String>,
    pub package_name: Option<String>,
    /// Path to a backup directory (contains a `.backup` marker)
    pub backup_path: Option<String>,
    /// Backup options
    pub backup_apk: Option<bool>,
    pub backup_data: Option<bool>,
    pub backup_obb: Option<bool>,
    pub backup_name_append: Option<String>,
    /// Human-friendly name to use for task name (e.g. app label)
    pub display_name: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct TaskRequest {
    pub task_type: TaskType,
    pub params: TaskParams,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct TaskCancelRequest {
    pub task_id: u64,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct TaskProgress {
    pub task_id: u64,
    pub task_type: TaskType,
    pub task_name: Option<String>,
    pub status: TaskStatus,
    pub total_progress: f32,
    pub message: String,
    pub step: u32,
    pub total_steps: u32,
    pub step_progress: Option<f32>,
}
