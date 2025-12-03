use crate::models::signals::task::TaskStatus;

mod backup;
mod donate;
mod download;
mod install;
mod manager;
pub(crate) use donate::DONATE_TMP_DIR;
pub(crate) use manager::TaskManager;

macro_rules! acquire_permit_or_cancel {
    ($semaphore:expr, $token:expr, $semaphore_name:literal) => {{
        if $token.is_cancelled() {
            info!(concat!("Task already cancelled before ", $semaphore_name, " semaphore acquisition"));
            return Err(anyhow::anyhow!(concat!("Task cancelled before ", $semaphore_name)));
        }

        debug!(concat!("Waiting for ", $semaphore_name, " semaphore"));
        tokio::select! {
            permit = $semaphore.acquire() => permit,
            _ = $token.cancelled() => {
                info!(concat!("Task cancelled while waiting for ", $semaphore_name, " semaphore"));
                return Err(anyhow::anyhow!(concat!("Task cancelled while waiting for ", $semaphore_name, " semaphore")));
            }
        }
    }};
}

use acquire_permit_or_cancel;

struct ProgressUpdate {
    status: TaskStatus,
    step_number: u8,
    step_progress: Option<f32>,
    message: String,
}

#[derive(Debug)]
struct InstallStepConfig<'a> {
    step_number: u8,
    log_context: &'a str,
}

#[derive(Debug)]
struct AdbStepConfig<'a> {
    step_number: u8,
    waiting_msg: &'a str,
    running_msg: String,
    log_context: &'a str,
}

#[derive(Debug)]
struct BackupStepConfig {
    package_name: String,
    display_name: Option<String>,
    backup_apk: bool,
    backup_data: bool,
    backup_obb: bool,
    backup_name_append: Option<String>,
}
