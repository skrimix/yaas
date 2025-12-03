use std::{error::Error, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow, ensure};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, info, instrument, warn};

use super::{AdbStepConfig, ProgressUpdate, TaskManager};
use crate::{
    adb::PackageName, apk::get_apk_info, archive::create_zip_from_dir,
    downloader::RcloneTransferStats, models::signals::task::TaskStatus,
};

pub(crate) const DONATE_TMP_DIR: &str = "_upload";

/// Guard that cleans up temporary files/directories when dropped.
/// Paths are removed in reverse order of addition.
struct CleanupGuard {
    paths: Vec<PathBuf>,
}

impl CleanupGuard {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn add_path(&mut self, path: PathBuf) {
        self.paths.push(path);
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        for path in self.paths.iter().rev() {
            if path.exists() {
                let result = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                if let Err(e) = result {
                    warn!(
                        error = &e as &dyn Error,
                        path = %path.display(),
                        "Failed to clean up temporary path during donation cleanup"
                    );
                } else {
                    debug!(path = %path.display(), "Cleaned up temporary path");
                }
            }
        }
    }
}

impl TaskManager {
    #[instrument(skip(self, update_progress, token))]
    pub(super) async fn handle_donate_app(
        &self,
        package: PackageName,
        display_name: Option<String>,
        update_progress: &impl Fn(ProgressUpdate),
        token: CancellationToken,
    ) -> Result<()> {
        ensure!(
            self.downloader_manager.is_some().await,
            "Downloader is not configured. Install configuration file to initialize."
        );

        debug!(
            package_name = %package,
            adb_permits_available = self.adb_semaphore.available_permits(),
            "Starting app donation task"
        );

        let adb_handler = self.adb_handler.clone();
        let device = adb_handler.current_device().await?;

        // Use downloads location as the base for temporary donation directories and archives.
        let settings = self.settings.read().await.clone();
        let downloads_root = settings.downloads_location();
        let upload_root = downloads_root.join(DONATE_TMP_DIR);
        tokio::fs::create_dir_all(&upload_root).await.with_context(|| {
            format!("Failed to create upload directory {}", upload_root.display())
        })?;

        // Guard to clean up temporary files on cancel/error/drop.
        let mut cleanup_guard = CleanupGuard::new();

        let pkg_for_pull = package.clone();
        let dest_root_clone = upload_root.clone();
        let pulled_dir = self
            .run_adb_one_step(
                AdbStepConfig {
                    step_number: 1,
                    waiting_msg: "Waiting to start pull from device...",
                    running_msg: "Pulling app from device...".to_string(),
                    log_context: "donate_app_pull",
                },
                update_progress,
                token.clone(),
                move || {
                    let adb_handler = adb_handler.clone();
                    let device = device.clone();
                    let pkg = pkg_for_pull.clone();
                    let dest_root = dest_root_clone.clone();
                    async move { adb_handler.pull_app_for_donation(&device, &pkg, &dest_root).await }
                },
            )
            .await?;

        cleanup_guard.add_path(pulled_dir.clone());

        if token.is_cancelled() {
            warn!("Task was cancelled after pull step");
            return Err(anyhow!("Task cancelled after pulling app from device"));
        }

        // Step 2: prepare archive (APK metadata, HWID file, archive name and ZIP).
        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number: 2,
            step_progress: None,
            message: "Preparing archive for upload...".into(),
        });

        let apk_path = pulled_dir.join(format!("{package}.apk"));
        let apk_info = get_apk_info(&apk_path)
            .with_context(|| format!("Failed to read APK metadata from {}", apk_path.display()))?;

        let label = apk_info
            .application_label
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .or(display_name.clone())
            .unwrap_or_else(|| package.as_str().to_owned());

        let version_code = apk_info.version_code.with_context(|| {
            format!("Failed to get version code from APK {}", apk_path.display())
        })?;

        let base_archive_name = format!("{label} v{version_code} {}", apk_info.package_name);
        let sanitized_name = sanitize_filename::sanitize(&base_archive_name);
        ensure!(!sanitized_name.is_empty(), "Sanitized archive name is empty");
        let archive_file_name = format!("{sanitized_name}.zip");

        let hwid_hex = {
            let digest = md5::compute(settings.installation_id.as_bytes());
            format!("{:X}", digest)
        };
        tokio::fs::write(pulled_dir.join("HWID.txt"), hwid_hex.as_bytes())
            .await
            .context("Failed to write HWID.txt")?;

        let archive_path =
            create_zip_from_dir(&pulled_dir, &upload_root, &archive_file_name, Some(token.clone()))
                .await
                .context("Failed to create archive from pulled app")?;

        cleanup_guard.add_path(archive_path.clone());

        if token.is_cancelled() {
            warn!("Task was cancelled after archive preparation step");
            return Err(anyhow!("Task cancelled after preparing archive"));
        }

        // Step 3: upload archive via rclone.
        update_progress(ProgressUpdate {
            status: TaskStatus::Running,
            step_number: 3,
            step_progress: None,
            message: "Uploading archive...".into(),
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<RcloneTransferStats>();

        let mut upload_task = {
            let downloader = self
                .downloader_manager
                .get()
                .await
                .ok_or_else(|| anyhow!("Downloader is not configured"))?;
            let archive_path = archive_path.clone();
            let token = token.clone();
            tokio::spawn(
                    async move {
                        downloader.upload_donation_archive(&archive_path, Some(tx), token).await
                    }
                    .instrument(Span::current()),
                )
        };

        debug!("Starting upload monitoring");
        let step_number = 3;
        let mut upload_result = None;
        let mut last_log_time = std::time::Instant::now();
        let mut last_log_progress = 0.0;
        let mut unknown_progress = false;

        while upload_result.is_none() {
            tokio::select! {
                result = &mut upload_task => {
                    result
                        .context("Upload task failed")?
                        .context("Failed to upload donation app archive")?;
                    info!("Upload task completed");
                    upload_result = Some(());
                }
                Some(progress) = rx.recv() => {
                    if unknown_progress {
                        // TODO: can we deduplicate this with the download task?
                        update_progress(ProgressUpdate {
                            status: TaskStatus::Running,
                            step_number,
                            step_progress: None,
                            message: format!(
                                "Uploading archive (Unknown%) - {}/s",
                                humansize::format_size(progress.speed, humansize::DECIMAL)
                            ),
                        });
                        continue;
                    }

                    let step_progress = progress.bytes as f32 / progress.total_bytes as f32;

                    // Log upload progress every 10 seconds or at major milestones
                    let now = std::time::Instant::now();
                    let should_log = now.duration_since(last_log_time) > Duration::from_secs(10)
                        || ((0.25..0.26).contains(&step_progress)
                            || (0.5..0.51).contains(&step_progress)
                            || (0.75..0.76).contains(&step_progress))
                            && last_log_progress != step_progress;
                    let progress_percent = step_progress * 100.0;

                    if should_log {
                        debug!(
                            bytes_uploaded = progress.bytes,
                            total_bytes = progress.total_bytes,
                            speed_bytes_per_sec = progress.speed,
                            progress_percent,
                            "Upload progress"
                        );
                        last_log_time = now;
                        last_log_progress = step_progress;
                    }

                    let (step_progress, message): (Option<f32>, String) =
                        if progress.bytes <= progress.total_bytes {
                            (Some(step_progress), format!(
                                "Uploading archive ({:.1}%) - {}/s",
                                progress_percent,
                                humansize::format_size(progress.speed, humansize::DECIMAL)
                            ))
                        } else {
                            unknown_progress = true;
                            warn!(progress.bytes, progress.total_bytes, "Upload progress is unknown: bytes > total_bytes");
                            (None, format!(
                                "Uploading archive (Unknown%) - {}/s",
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
            }
        }

        upload_result.unwrap();

        // drop(cleanup_guard);

        Ok(())
    }
}
