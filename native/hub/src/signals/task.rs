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

#[derive(Clone, Copy, Serialize, Deserialize, SignalPiece)]
pub enum TaskStatus {
    Waiting,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, SignalPiece)]
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
