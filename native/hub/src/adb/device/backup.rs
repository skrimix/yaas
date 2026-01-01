use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use forensic_adb::UnixPath;
use time::{OffsetDateTime, macros::format_description};
use tokio::fs::{self, File};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};

use super::AdbDevice;
use crate::{
    adb::{PACKAGE_NAME_REGEX, PackageName},
    utils::{
        dir_has_any_files, first_subdirectory, remove_child_dir_if_exists, single_subdirectory,
    },
};

/// Options to control backup behavior
#[derive(Debug, Clone, Default)]
pub(crate) struct BackupOptions {
    /// String to append to backup name
    pub name_append: Option<String>,
    /// Should backup APK
    pub backup_apk: bool,
    /// Should backup data (private/shared)
    pub backup_data: bool,
    /// Should fail if private data backup fails
    pub require_private_data: bool,
    /// Should backup OBB files
    pub backup_obb: bool,
}

impl AdbDevice {
    /// Creates a backup of the given package.
    /// Returns `Ok(Some(path))` if backup was created, `Ok(None)` if nothing to back up.
    #[instrument(level = "debug", skip(self), err)]
    pub(crate) async fn backup_app(
        &self,
        package: &PackageName,
        display_name: Option<&str>,
        backups_location: &Path,
        options: &BackupOptions,
        token: CancellationToken,
    ) -> Result<Option<PathBuf>> {
        ensure!(backups_location.is_dir(), "Backups location must be a directory");
        ensure!(
            !options.require_private_data || options.backup_data,
            "require_private_data requires backup_data"
        );

        let package_str = package.as_str();
        info!(package = package_str, "Creating app backup");
        let fmt = format_description!("[year]-[month]-[day]_[hour]-[minute]-[second]");
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let timestamp = now.format(&fmt).unwrap_or_else(|_| "0000-00-00_00-00-00".into());
        // Build directory name: timestamp + sanitized display name (fallback to package name)
        let display = display_name
            .map(sanitize_filename::sanitize)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| package_str.to_string());
        let mut directory_name = format!("{}_{}", timestamp, display);
        if let Some(suffix) = &options.name_append
            && !suffix.is_empty()
        {
            let sanitized_suffix = sanitize_filename::sanitize(suffix);
            if !sanitized_suffix.is_empty() {
                directory_name.push('_');
                directory_name.push_str(&sanitized_suffix);
            }
        }
        let backup_path = backups_location.join(directory_name);
        debug!(path = %backup_path.display(), "Creating backup directory");
        fs::create_dir_all(&backup_path).await?;

        let shared_data_path = UnixPath::new("/sdcard/Android/data").join(package_str);
        let private_data_path = UnixPath::new("/data/data").join(package_str);
        let obb_path = UnixPath::new("/sdcard/Android/obb").join(package_str);
        debug!(shared_data_path = %shared_data_path.display(), private_data_path = %private_data_path.display(), obb_path = %obb_path.display(), "Built source paths");

        let shared_data_backup_path = backup_path.join("data");
        let private_data_backup_path = backup_path.join("data_private");
        let obb_backup_path = backup_path.join("obb");
        debug!(shared_data_backup_path = %shared_data_backup_path.display(), private_data_backup_path = %private_data_backup_path.display(), obb_backup_path = %obb_backup_path.display(), "Built backup paths");

        let mut backup_empty = true;

        // Backup app data
        if options.backup_data {
            debug!("Backing up app data");

            // Clean old tmp if present
            let tmp_root = UnixPath::new("/sdcard/backup_tmp");
            if self.dir_exists(tmp_root).await? {
                info!("Found old /sdcard/backup_tmp, deleting");
                self.shell("rm -rf /sdcard/backup_tmp/").await?;
            }

            // Private data via run-as
            // Pipe through tar because run-as has weird permissions
            debug!("Trying to backup private data");
            fs::create_dir_all(&private_data_backup_path).await?;
            let tmp_pkg = tmp_root.join(package_str);
            let cmd = format!(
                "mkdir -p '{tmp}'; run-as {pkg} tar -cf - -C '{priv_path}' . | tar -xvf - -C \
                 '{tmp}'",
                tmp = tmp_pkg.display(),
                pkg = package_str,
                priv_path = private_data_path.display(),
            );
            let cmd_output = await_or_cancel_backup(
                &token,
                &backup_path,
                "run-as private data tar",
                self.shell(&cmd),
                async {
                    let _ = self.shell("rm -rf /sdcard/backup_tmp/").await;
                },
            )
            .await?;
            if !cmd_output.is_empty() {
                debug!("Command output: {}", cmd_output);
            }
            if options.require_private_data && cmd_output.contains("run-as:") {
                bail!("Private data backup failed: run-as failed: {}", cmd_output);
            }
            await_or_cancel_backup(
                &token,
                &backup_path,
                "pull private data",
                self.pull_dir(&tmp_pkg, &private_data_backup_path),
                async {
                    let _ = self.shell("rm -rf /sdcard/backup_tmp/").await;
                },
            )
            .await?;
            let _ = self.shell("rm -rf /sdcard/backup_tmp/").await;

            let private_pkg_dir = private_data_backup_path.join(package_str);
            if private_pkg_dir.is_dir() {
                let _ = remove_child_dir_if_exists(&private_pkg_dir, "cache").await;
                let _ = remove_child_dir_if_exists(&private_pkg_dir, "code_cache").await;
            }

            let has_private_files = dir_has_any_files(&private_data_backup_path).await?;
            if !has_private_files {
                debug!("No files in pulled private data, deleting");
                let _ = fs::remove_dir_all(&private_data_backup_path).await;
            }
            backup_empty &= !has_private_files;

            // Shared data
            if self.dir_exists(&shared_data_path).await? {
                debug!("Backing up shared data");
                fs::create_dir_all(&shared_data_backup_path).await?;
                await_or_cancel_backup(
                    &token,
                    &backup_path,
                    "pull shared data",
                    self.pull_dir(&shared_data_path, &shared_data_backup_path),
                    async {},
                )
                .await?;

                let shared_pkg_dir = shared_data_backup_path.join(package_str);
                if shared_pkg_dir.is_dir() {
                    let _ = remove_child_dir_if_exists(&shared_pkg_dir, "cache").await;
                }

                let has_shared_files = dir_has_any_files(&shared_data_backup_path).await?;
                if !has_shared_files {
                    debug!("No files in pulled shared data, deleting");
                    let _ = fs::remove_dir_all(&shared_data_backup_path).await;
                }
                backup_empty &= !has_shared_files;
            } else {
                debug!("No shared data directory found, skipping");
            }
        }

        // Backup APK
        if options.backup_apk {
            debug!("Backing up APK");
            let apk_remote = self.get_apk_path(package).await?;
            await_or_cancel_backup(
                &token,
                &backup_path,
                "pull APK",
                self.pull(UnixPath::new(&apk_remote), &backup_path),
                async {},
            )
            .await?;
            backup_empty = false;
        }

        // Backup OBB
        if options.backup_obb {
            if self.dir_exists(&obb_path).await? {
                debug!("Backing up OBB");
                fs::create_dir_all(&obb_backup_path).await?;
                await_or_cancel_backup(
                    &token,
                    &backup_path,
                    "pull OBB",
                    self.pull_dir(&obb_path, &obb_backup_path),
                    async {},
                )
                .await?;

                let has_obb_files = dir_has_any_files(&obb_backup_path).await?;
                if !has_obb_files {
                    debug!("No files in pulled OBB, deleting");
                    let _ = fs::remove_dir_all(&obb_backup_path).await;
                }
                backup_empty &= !has_obb_files;
            } else {
                debug!("No OBB directory found, skipping");
            }
        }

        if backup_empty {
            info!("Nothing backed up, cleaning up empty directory");
            let _ = fs::remove_dir_all(&backup_path).await;
            return Ok(None);
        }

        // Marker file
        let _ = File::create(backup_path.join(".backup")).await?;
        info!(path = %backup_path.display(), "Backup created successfully");
        Ok(Some(backup_path))
    }

    /// Restores a backup from the given path
    #[instrument(level = "debug", skip(self), err)]
    pub(crate) async fn restore_backup(&self, backup_path: &Path) -> Result<()> {
        ensure!(backup_path.is_dir(), "Backup path is not a directory");
        ensure!(backup_path.join(".backup").exists(), "Backup marker not found (.backup)");

        let shared_data_backup_path = backup_path.join("data");
        let private_data_backup_path = backup_path.join("data_private");
        let obb_backup_path = backup_path.join("obb");

        // Restore APK
        {
            let mut apk_candidate: Option<PathBuf> = None;
            if backup_path.is_dir() {
                let mut rd = fs::read_dir(backup_path).await?;
                while let Some(entry) = rd.next_entry().await? {
                    if entry.file_type().await.map(|t| t.is_file()).unwrap_or(false)
                        && entry
                            .path()
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("apk"))
                    {
                        apk_candidate = Some(entry.path());
                        break;
                    }
                }
            }
            if apk_candidate.is_none() {
                // If there is no APK in the backup, ensure the app is already installed
                // Try to infer the package name from any backup subfolder (private/shared/obb)
                let mut candidate_pkg: Option<String> = None;
                for dir in [&private_data_backup_path, &shared_data_backup_path, &obb_backup_path] {
                    if dir.is_dir()
                        && let Some(sub) = first_subdirectory(dir).await?
                        && let Some(name) = sub.file_name().and_then(|n| n.to_str())
                        && PACKAGE_NAME_REGEX.is_match(name)
                    {
                        candidate_pkg = Some(name.to_string());
                        break;
                    }
                }
                if let Some(pkg) = candidate_pkg {
                    let pkg = PackageName::parse(&pkg)
                        .context("Inferred invalid package name from backup directory")?;
                    let _ = self.get_apk_path(&pkg).await.with_context(|| {
                        format!(
                            "Backup does not contain an APK and package '{pkg}' is not installed"
                        )
                    })?;
                } else {
                    bail!(
                        "Backup does not contain an APK and no package folder was found to infer \
                         the package name"
                    );
                }
            } else {
                let apk = apk_candidate.unwrap();
                info!(apk = %apk.display(), "Restoring APK");
                // Use direct install without any special handling
                Box::pin(self.inner.install_package(&apk, true, true, true))
                    .await
                    .context("Failed to install APK during restore")?;
            }
        }

        // Restore OBB
        if obb_backup_path.is_dir()
            && let Some(pkg_dir) = single_subdirectory(&obb_backup_path).await?
        {
            debug!("Restoring OBB");
            let remote_parent = UnixPath::new("/sdcard/Android/obb");
            self.push_dir(&pkg_dir, remote_parent, true).await?;
        }

        // Restore shared data
        if shared_data_backup_path.is_dir()
            && let Some(pkg_dir) = single_subdirectory(&shared_data_backup_path).await?
        {
            debug!("Restoring shared data");
            let remote_parent = UnixPath::new("/sdcard/Android/data");
            self.push_dir(&pkg_dir, remote_parent, true).await?;
        }

        // Restore private data
        if private_data_backup_path.is_dir()
            && let Some(pkg_dir) = single_subdirectory(&private_data_backup_path).await?
        {
            let package_name = pkg_dir
                .file_name()
                .and_then(|n| n.to_str())
                .context("Failed to get private data package name")?;

            debug!("Restoring private data");
            // Push to temporary dir
            let _ = self.shell("rm -rf /sdcard/restore_tmp/").await;
            self.shell("mkdir -p /sdcard/restore_tmp/").await?;
            self.push_dir(&pkg_dir, UnixPath::new("/sdcard/restore_tmp/"), false).await?;

            // Pipe through tar because run-as has weird permissions
            let cmd = format!(
                "tar -cf - -C '/sdcard/restore_tmp/{pkg}/' . | run-as {pkg} tar -xvf - -C \
                 '/data/data/{pkg}/'; rm -rf /sdcard/restore_tmp/",
                pkg = package_name
            );
            self.shell(&cmd).await?;
        }

        info!("Backup restored successfully");
        Ok(())
    }
}

/// Awaits a future or, if cancellation is requested, deletes the incomplete backup directory and
/// runs cleanup, then returns a cancellation error.
#[instrument(level = "debug", skip(token, fut, backup_path, cleanup), fields(op = op_name), err)]
async fn await_or_cancel_backup<T, F, C>(
    token: &CancellationToken,
    backup_path: &Path,
    op_name: &str,
    fut: F,
    cleanup: C,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
    C: std::future::Future<Output = ()>,
{
    tokio::select! {
        res = fut => res,
        _ = token.cancelled() => {
            cleanup.await;
            warn!(path = %backup_path.display(), op = op_name, "Backup cancelled, removing incomplete directory");
            let _ = fs::remove_dir_all(backup_path).await;
            Err(anyhow!("Backup cancelled during: {op_name}"))
        }
    }
}
