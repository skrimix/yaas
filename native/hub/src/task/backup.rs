use std::path::Path;

use anyhow::{Result, ensure};
use rinf::RustSignal;
use tokio_util::sync::CancellationToken;
use tracing::{debug, instrument};

use super::{AdbStepConfig, BackupStepConfig, ProgressUpdate, TaskManager};
use crate::{
    adb::{PackageName, device::BackupOptions},
    models::signals::backups::BackupsChanged,
};

impl TaskManager {
    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_backup(
        &self,
        cfg: BackupStepConfig,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        ensure!(cfg.backup_apk || cfg.backup_data || cfg.backup_obb, "No parts selected to backup");

        debug!(
            package_name = %cfg.package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting backup task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let parts = [
            if cfg.backup_data { Some("data") } else { None },
            if cfg.backup_apk { Some("apk") } else { None },
            if cfg.backup_obb { Some("obb") } else { None },
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        let backups_path = self.settings.read().await.backups_location();
        debug!(path = %backups_path.display(), "Using backups location");

        let options = BackupOptions {
            name_append: cfg.backup_name_append,
            backup_apk: cfg.backup_apk,
            backup_data: cfg.backup_data,
            backup_obb: cfg.backup_obb,
            require_private_data: false,
        };

        let pkg = PackageName::parse(&cfg.package_name)?;
        let display_name = cfg.display_name.clone();
        let options_moved = options;
        let backups_path_moved = backups_path.clone();
        let token_clone = token.clone();

        let maybe_created = self
            .run_adb_one_step(
                AdbStepConfig {
                    step_number: 1,
                    waiting_msg: "Waiting to start backup...",
                    running_msg: format!("Creating backup ({parts})..."),
                    log_context: "backup",
                },
                update_progress,
                token,
                move || {
                    let package_name = pkg.clone();
                    let display_name = display_name.clone();
                    let backups_path = backups_path_moved.clone();
                    let options = options_moved;
                    async move {
                        adb_handler
                            .backup_app(
                                &device,
                                &package_name,
                                display_name.as_deref(),
                                backups_path.as_path(),
                                &options,
                                token_clone,
                            )
                            .await
                    }
                },
            )
            .await?;

        ensure!(
            maybe_created.is_some(),
            "Nothing to back up for this app (selected parts: {})",
            parts
        );

        BackupsChanged {}.send_signal_to_dart();

        Ok(())
    }

    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_restore(
        &self,
        backup_path: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            backup_path = %backup_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting restore task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backup_path_cloned = backup_path.clone();
        self.run_adb_one_step(
            AdbStepConfig {
                step_number: 1,
                waiting_msg: "Waiting to start restore...",
                running_msg: "Restoring backup...".to_string(),
                log_context: "restore",
            },
            update_progress,
            token,
            move || {
                let path = backup_path_cloned.clone();
                async move { adb_handler.restore_backup(&device, Path::new(&path)).await }
            },
        )
        .await
        .map(|_| ())
    }
}
