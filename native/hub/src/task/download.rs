use std::{error::Error, path::Path, time::Duration};

use anyhow::{Context, Result, anyhow, ensure};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, instrument, warn};

use super::{InstallStepConfig, ProgressUpdate, TaskManager};
use crate::{
    adb::PackageName, downloader::RcloneTransferStats, models::signals::task::TaskStatus,
    task::acquire_permit_or_cancel,
};

impl TaskManager {
    #[instrument(level = "debug", skip(self, update_progress, token))]
    async fn run_download_step(
        &self,
        app_full_name: &str,
        true_package: PackageName,
        step_number: u8,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<String> {
        ensure!(
            self.downloader_manager.is_some().await,
            "Downloader is not configured. Install configuration file to initialize."
        );
        update_progress(ProgressUpdate {
            status: TaskStatus::Waiting,
            step_number,
            step_progress: None,
            message: "Waiting to start download...".into(),
        });

        let _permit = acquire_permit_or_cancel!(self.download_semaphore, token, "download");
        debug!(
            download_permits_remaining = self.download_semaphore.available_permits(),
            "Acquired download semaphore"
        );

        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number,
            step_progress: None,
            message: "Starting download...".into(),
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();
        let (stage_tx, mut stage_rx) = mpsc::unbounded_channel::<String>();

        let mut download_task = {
            let downloader = self.downloader_manager.get().await.expect("downloader missing");
            let app_full_name = app_full_name.to_string();
            let token = token.clone();
            tokio::spawn(
                async move {
                    downloader.download_app(app_full_name, true_package, tx, stage_tx, token).await
                }
                .instrument(Span::current()),
            )
        };

        debug!("Starting download monitoring");
        let mut download_result: Option<String> = None;
        let mut last_log_time = std::time::Instant::now();
        let mut last_log_progress = 0.0;
        // Marker for the "bytes" > "total_bytes" case
        // We cant calculate progress if we get to that point
        let mut unknown_progress = false;

        while download_result.is_none() {
            tokio::select! {
                result = &mut download_task => {
                    let app_path = result
                        .context("Download task failed")?
                        .context("Failed to download app")?;
                    info!("Download task completed");
                    download_result = Some(app_path);
                }
                Some(progress) = rx.recv() => {
                    if unknown_progress {
                        // We can only report the speed, skip everything else
                        update_progress(ProgressUpdate {
                            status: TaskStatus::Running,
                            step_number,
                            step_progress: None,
                            message: format!(
                                "Downloading (Unknown%) - {}/s",
                                humansize::format_size(progress.speed, humansize::DECIMAL)
                            ),
                        });
                        continue;
                    }

                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;

                    // Log download progress every 10 seconds or at major milestones
                    let now = std::time::Instant::now();
                    let should_log = now.duration_since(last_log_time) > Duration::from_secs(10)
                        || ((0.25..0.26).contains(&step_progress)
                            || (0.5..0.51).contains(&step_progress)
                            || (0.75..0.76).contains(&step_progress))
                            && last_log_progress != step_progress;
                    let progress_percent = step_progress * 100.0;

                    if should_log {
                        debug!(
                            bytes_downloaded = progress.bytes,
                            total_bytes = progress.total_bytes,
                            speed_bytes_per_sec = progress.speed,
                            progress_percent,
                            "Download progress"
                        );
                        last_log_time = now;
                        last_log_progress = step_progress;
                    }

                    let (step_progress, message): (Option<f32>, String) = if progress.bytes <= progress.total_bytes {
                        (Some(step_progress), format!(
                            "Downloading ({:.1}%) - {}/s",
                            progress_percent,
                            humansize::format_size(progress.speed, humansize::DECIMAL)
                        ))
                    } else {
                        unknown_progress = true;
                        warn!(progress.bytes, progress.total_bytes, "Download progress is unknown: bytes > total_bytes");
                        (None, format!(
                            "Downloading (Unknown%) - {}/s",
                            humansize::format_size(progress.speed, humansize::DECIMAL)
                        ))
                    };

                    update_progress(ProgressUpdate {
                        status: TaskStatus::Running,
                        step_number,
                        step_progress,
                        message,
                    });
                }
                Some(stage_msg) = stage_rx.recv() => {
                    update_progress(ProgressUpdate {
                        status: TaskStatus::Running,
                        step_number,
                        step_progress: None,
                        message: stage_msg,
                    });
                }
            }
        }

        let app_path = download_result.unwrap();
        info!(
            app_path = %app_path,
            download_permits = self.download_semaphore.available_permits() + 1,
            "Download completed, releasing download semaphore"
        );
        drop(_permit);

        Ok(app_path)
    }

    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_download_install(
        &self,
        app_full_name: String,
        true_package: PackageName,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting download and install task"
        );

        let app_path = self
            .run_download_step(&app_full_name, true_package, 1, update_progress, token.clone())
            .await?;

        if token.is_cancelled() {
            warn!("Task was cancelled after download completion");
            return Err(anyhow!("Task cancelled after download"));
        }

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        let settings = self.settings.read().await;
        let backups_location = settings.backups_location();
        let auto_reinstall_on_conflict = settings.auto_reinstall_on_conflict;
        drop(settings);

        let app_path_cloned = app_path.clone();
        self.run_install_step(
            InstallStepConfig { step_number: 2, log_context: "sideload" },
            update_progress,
            token.clone(),
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
                                auto_reinstall_on_conflict,
                            )
                            .await
                    }
                    .instrument(Span::current()),
                )
            },
        )
        .await?;

        // Apply downloads cleanup policy
        if let Err(e) = self.cleanup_downloads_after_install(&app_full_name, &app_path).await {
            // Non-fatal: log but do not fail the task
            error!(
                error = e.as_ref() as &dyn Error,
                "Failed to apply downloads cleanup policy after install"
            );
        }

        Ok(())
    }

    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_download(
        &self,
        app_full_name: String,
        true_package: PackageName,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        debug!(
            app_name = %app_full_name,
            download_permits_available = self.download_semaphore.available_permits(),
            "Starting download task"
        );

        let _ =
            self.run_download_step(&app_full_name, true_package, 1, update_progress, token).await?;

        Ok(())
    }

    #[instrument(skip(self), fields(app_full_name = %app_full_name, app_path = %app_path), err)]
    async fn cleanup_downloads_after_install(
        &self,
        app_full_name: &str,
        app_path: &str,
    ) -> Result<()> {
        let cleanup_policy = self.settings.read().await.cleanup_policy;
        self.downloads_catalog.apply_cleanup_policy(cleanup_policy, app_full_name, app_path).await
    }
}
