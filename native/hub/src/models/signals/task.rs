use core::fmt;
use std::{fmt::Display, path::Path};

use anyhow::Result;
use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, SignalPiece)]
pub enum TaskKind {
    Download,
    DownloadInstall,
    InstallApk,
    InstallLocalApp,
    Uninstall,
    BackupApp,
    RestoreBackup,
    /// Pull an installed app from device and upload it for sharing
    ShareApp,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, SignalPiece)]
pub enum TaskStatus {
    Waiting,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Task with parameters.
#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum Task {
    /// Download an app by full name (catalog entry identifier)
    Download(String),
    /// Download and then install an app by full name
    DownloadInstall(String),
    /// Install an APK from a single-file path
    InstallApk(String),
    /// Install a local app (a directory containing APK/manifest)
    InstallLocalApp(String),
    /// Uninstall a package. Optional display name is used only for UI.
    Uninstall { package_name: String, display_name: Option<String> },
    /// Create a backup for a package with selected parts.
    BackupApp {
        package_name: String,
        display_name: Option<String>,
        backup_apk: bool,
        backup_data: bool,
        backup_obb: bool,
        backup_name_append: Option<String>,
    },
    /// Restore from a backup directory path (contains a `.backup` marker)
    RestoreBackup(String),
    /// Share (upload/donate) installed app files from the device.
    ShareApp { package_name: String, display_name: Option<String> },
}

impl Task {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Task::Download(_) => "Download",
            Task::DownloadInstall(_) => "Download & Install",
            Task::InstallApk(_) => "Install APK",
            Task::InstallLocalApp(_) => "Install Local App",
            Task::Uninstall { .. } => "Uninstall",
            Task::BackupApp { .. } => "Backup App",
            Task::RestoreBackup(_) => "Restore Backup",
            Task::ShareApp { .. } => "Share App",
        }
    }

    pub fn task_name(&self) -> Result<String> {
        Ok(match self {
            Task::Download(name) | Task::DownloadInstall(name) => name.clone(),
            Task::InstallApk(apk_path) => {
                Path::new(apk_path).file_name().unwrap_or_default().to_string_lossy().to_string()
            }
            Task::InstallLocalApp(app_path) => {
                Path::new(app_path).file_name().unwrap_or_default().to_string_lossy().to_string()
            }
            Task::Uninstall { package_name, display_name } => {
                display_name.clone().unwrap_or_else(|| package_name.clone())
            }
            Task::BackupApp { package_name, display_name, .. } => {
                display_name.clone().unwrap_or_else(|| package_name.clone())
            }
            Task::RestoreBackup(path) => {
                Path::new(path).file_name().unwrap_or_default().to_string_lossy().to_string()
            }
            Task::ShareApp { package_name, display_name } => {
                display_name.clone().unwrap_or_else(|| package_name.clone())
            }
        })
    }

    pub fn total_steps(&self) -> u8 {
        match self {
            Task::Download(..) => 1,
            Task::DownloadInstall(..) => 2,
            Task::InstallApk(..) => 1,
            Task::InstallLocalApp(..) => 1,
            Task::Uninstall { .. } => 1,
            Task::BackupApp { .. } => 1,
            Task::RestoreBackup(..) => 1,
            Task::ShareApp { .. } => 3,
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind_label())
    }
}

impl From<&Task> for TaskKind {
    fn from(value: &Task) -> Self {
        match value {
            Task::Download(_) => TaskKind::Download,
            Task::DownloadInstall(_) => TaskKind::DownloadInstall,
            Task::InstallApk(_) => TaskKind::InstallApk,
            Task::InstallLocalApp(_) => TaskKind::InstallLocalApp,
            Task::Uninstall { .. } => TaskKind::Uninstall,
            Task::BackupApp { .. } => TaskKind::BackupApp,
            Task::RestoreBackup(_) => TaskKind::RestoreBackup,
            Task::ShareApp { .. } => TaskKind::ShareApp,
        }
    }
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct TaskRequest {
    pub task: Task,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct TaskCancelRequest {
    pub task_id: u64,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct TaskProgress {
    pub task_id: u64,
    pub task_kind: TaskKind,
    pub task_name: Option<String>,
    pub status: TaskStatus,
    /// Overall progress across all steps in range [0.0, 1.0]
    pub total_progress: f32,
    /// Human-readable status for the current step
    pub message: String,
    pub current_step: u32,
    pub total_steps: u32,
    /// Progress for the current step in range [0.0, 1.0].
    /// None means this step does not report progress.
    pub step_progress: Option<f32>,
}
