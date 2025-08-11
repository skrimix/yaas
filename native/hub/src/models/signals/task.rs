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
}

impl Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskType::Download => write!(f, "Download"),
            TaskType::DownloadInstall => write!(f, "Download & Install"),
            TaskType::InstallApk => write!(f, "Install APK"),
            TaskType::InstallLocalApp => write!(f, "Install Local App"),
            TaskType::Uninstall => write!(f, "Uninstall"),
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

#[derive(Clone, Serialize, Deserialize, SignalPiece)]
pub struct TaskParams {
    pub cloud_app_full_name: Option<String>,
    pub apk_path: Option<String>,
    pub local_app_path: Option<String>,
    pub package_name: Option<String>,
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
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct TaskCreatedEvent {
    pub task_id: u64,
    pub task_type: TaskType,
    pub params: TaskParams,
}
