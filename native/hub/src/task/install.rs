use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn, Instrument, Span};

use crate::adb::device::SideloadProgress;

use super::{AdbStepConfig, InstallStepConfig, ProgressUpdate, TaskManager};

impl TaskManager {
    #[instrument(level = "debug", skip(self, update_progress, token, spawn_install))]
    pub(super) async fn run_install_step<'a>(
        &self,
        cfg: InstallStepConfig<'a>,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
        spawn_install: impl FnOnce(
            mpsc::UnboundedSender<SideloadProgress>,
            CancellationToken,
        ) -> tokio::task::JoinHandle<anyhow::Result<()>>,
    ) -> Result<()> {
        update_progress(ProgressUpdate {
            status: crate::models::signals::task::TaskStatus::Waiting,
            step_number: cfg.step_number,
            step_progress: None,
            message: "Waiting to start installation...".into(),
        });

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        debug!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for installation"
        );

        update_progress(ProgressUpdate {
            status: crate::models::signals::task::TaskStatus::Running,
            step_number: cfg.step_number,
            step_progress: None,
            message: "Installing APK...".into(),
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();
        let mut install_task = spawn_install(tx, token.clone());

        debug!("Starting {} monitoring", cfg.log_context);
        let mut install_result = None;
        let mut last_log_time = std::time::Instant::now();
        let mut cancel_requested = false;

        while install_result.is_none() {
            tokio::select! {
                result = &mut install_task => {
                    install_result = Some(result.context("Install task failed")?);
                    info!("{} task completed", cfg.log_context);
                }
                _ = token.cancelled(), if !cancel_requested => {
                    warn!("Cancellation requested for install step, requesting task abort");
                    cancel_requested = true;
                    install_task.abort();
                }
                Some(progress) = rx.recv() => {
                    let step_progress_num = progress.progress.unwrap_or(0.0);

                    // Log progress every 5 seconds
                    let now = std::time::Instant::now();
                    if now.duration_since(last_log_time) > Duration::from_secs(5) {
                        debug!(
                            install_progress = step_progress_num,
                            status = %progress.status,
                            context = cfg.log_context,
                            "Installation progress"
                        );
                        last_log_time = now;
                    }

                    update_progress(ProgressUpdate {
                        status: crate::models::signals::task::TaskStatus::Running,
                        step_number: cfg.step_number,
                        step_progress: progress.progress,
                        message: progress.status,
                    });
                }
            }
        }

        install_result.unwrap()?;

        info!(
            adb_permits = self.adb_semaphore.available_permits() + 1,
            context = cfg.log_context,
            "Installation completed, releasing ADB semaphore"
        );

        Ok(())
    }

    #[instrument(level = "debug", skip(self, update_progress, token, fut))]
    pub(super) async fn run_adb_one_step<'a, F, Fut, T>(
        &self,
        cfg: AdbStepConfig<'a>,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
        fut: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        update_progress(ProgressUpdate {
            status: crate::models::signals::task::TaskStatus::Waiting,
            step_number: cfg.step_number,
            step_progress: None,
            message: cfg.waiting_msg.into(),
        });

        let _permit = acquire_permit_or_cancel!(self.adb_semaphore, token, "ADB");
        debug!(
            adb_permits_remaining = self.adb_semaphore.available_permits(),
            "Acquired ADB semaphore for {}", cfg.log_context
        );

        update_progress(ProgressUpdate {
            status: crate::models::signals::task::TaskStatus::Running,
            step_number: cfg.step_number,
            step_progress: None,
            message: cfg.running_msg,
        });

        debug!("Starting {} operation", cfg.log_context);
        let result = fut().await?;
        debug!("{} operation completed", cfg.log_context);

        info!(
            adb_permits = self.adb_semaphore.available_permits() + 1,
            "{} completed, releasing ADB semaphore", cfg.log_context
        );

        Ok(result)
    }

    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_install_apk(
        &self,
        apk_path: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            apk_path = %apk_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting APK install task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backups_location =
            std::path::PathBuf::from(self.settings.read().await.backups_location.clone());

        self.run_install_step(
            InstallStepConfig { step_number: 1, log_context: "apk_install" },
            update_progress,
            token,
            move |tx, _token| {
                let backups_location = backups_location.clone();
                tokio::spawn(
                    async move {
                        adb_handler
                            .install_apk(&device, Path::new(&apk_path), backups_location, tx)
                            .await
                    }
                    .instrument(Span::current()),
                )
            },
        )
        .await
        .map(|_| ())
        .context("APK installation failed")
    }

    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_install_local_app(
        &self,
        app_path: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            app_path = %app_path,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting local app install task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let backups_location =
            std::path::PathBuf::from(self.settings.read().await.backups_location.clone());
        let app_path_cloned = app_path.clone();
        self.run_install_step(
            InstallStepConfig { step_number: 1, log_context: "sideload_local" },
            update_progress,
            token,
            move |tx, token| {
                let app_path = app_path_cloned.clone();
                let backups_location = backups_location.clone();
                tokio::spawn(
                    async move {
                        adb_handler
                            .sideload_app(
                                &device,
                                Path::new(&app_path),
                                backups_location,
                                tx,
                                token,
                            )
                            .await
                    }
                    .instrument(Span::current()),
                )
            },
        )
        .await
        .map(|_| ())
        .context("Local app installation failed")
    }

    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_uninstall(
        &self,
        package_name: String,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            package_name = %package_name,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting uninstall task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let pkg = package_name.clone();
        self.run_adb_one_step(
            AdbStepConfig {
                step_number: 1,
                waiting_msg: "Waiting to start uninstallation...",
                running_msg: "Uninstalling app...".to_string(),
                log_context: "uninstall",
            },
            update_progress,
            token,
            move || {
                let package_name = pkg.clone();
                async move { adb_handler.uninstall_package(&device, &package_name).await }
            },
        )
        .await
        .map(|_| ())
    }
}
