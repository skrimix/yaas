use std::{
    error::Error,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use forensic_adb::{DeviceError, DirectoryTransferProgress, UnixPath};
use lazy_regex::{Lazy, Regex, lazy_regex};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, info, instrument, trace, warn};

use super::{AdbDevice, backup::BackupOptions};
use crate::{adb::PackageName, archive::decompress_all_7z_in_dir, models::apk_info::get_apk_info};

/// Regex to split command arguments - handles quoted arguments with spaces
/// Note: This is a simplified parser for install scripts and may not handle all edge cases
static COMMAND_ARGS_REGEX: Lazy<Regex> = lazy_regex!(r#""[^"]*"|'[^']*'|[^\s]+"#);

/// Progress information for sideload operations
#[derive(Debug)]
pub(crate) struct SideloadProgress {
    pub status: String,
    pub progress: Option<f32>,
}

impl AdbDevice {
    /// Executes an install script from the given path
    #[instrument(level = "debug", skip(self), err)]
    async fn execute_install_script(
        &self,
        script_path: &Path,
        backups_location: &Path,
        token: CancellationToken,
        auto_reinstall_on_conflict: bool,
    ) -> Result<()> {
        let script_content = tokio::fs::read_to_string(script_path)
            .await
            .context("Failed to read install script")?;
        let script_dir = script_path.parent().context("Failed to get script directory")?;

        // Unpack all 7z archives if present
        decompress_all_7z_in_dir(script_dir, Some(token.clone()))
            .await
            .context("Failed to decompress .7z archives in install folder")?;

        for (line_index, line) in script_content.lines().enumerate() {
            let line_num = line_index + 1;
            // Remove comments and redirections
            let line =
                line.split('#').next().unwrap_or("").split("REM").next().unwrap_or("").trim();
            if line.is_empty() {
                trace!(line_num, "Skipping empty or comment line");
                continue;
            }

            let command = line.split('>').next().unwrap_or("").trim();
            ensure!(
                !command.is_empty(),
                "Line {line_num}: Line is empty after removing redirections"
            );
            debug!(line_num, command, "Parsed command");

            let tokens: Vec<String> = COMMAND_ARGS_REGEX
                .find_iter(command)
                .map(|m| {
                    let token = m.as_str();
                    // Remove surrounding quotes but preserve the content
                    if (token.starts_with('"') && token.ends_with('"'))
                        || (token.starts_with('\'') && token.ends_with('\''))
                    {
                        token[1..token.len() - 1].to_string()
                    } else {
                        token.to_string()
                    }
                })
                .collect();

            if tokens[0] == "7z" {
                debug!(line_num, command, "Skipping 7z command");
                continue;
            }
            ensure!(tokens[0] == "adb", "Line {line_num}: Unsupported command '{command}'");

            ensure!(tokens.len() >= 2, "Line {line_num}: ADB command missing operation");
            let adb_command = &tokens[1];
            let adb_args_raw = &tokens[2..];
            let adb_args =
                adb_args_raw.iter().filter(|arg| !arg.starts_with('-')).collect::<Vec<_>>();

            match adb_command.as_str() {
                "install" => {
                    // We only care about the APK path
                    let apk_path = script_dir.join(
                        adb_args.iter().find(|arg| arg.ends_with(".apk")).with_context(|| {
                            format!("Line {line_num}: adb install: missing APK path")
                        })?,
                    );
                    debug!(apk_path = %apk_path.display(), "Line {line_num}: adb install: installing APK");
                    self.install_apk(&apk_path, backups_location, auto_reinstall_on_conflict)
                        .await
                        .with_context(|| {
                            format!(
                                "Line {line_num}: adb install: failed to install APK '{}'",
                                apk_path.display()
                            )
                        })?;
                }
                "uninstall" => {
                    ensure!(
                        adb_args.len() == 1,
                        "Line {line_num}: adb uninstall: wrong number of arguments: expected 1, \
                         got {}",
                        adb_args.len()
                    );
                    let package = &adb_args[0];
                    debug!(package, "Line {line_num}: uninstalling package");
                    let package = match PackageName::parse(package) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(
                                error = e.as_ref() as &dyn Error,
                                "Line {line_num}: adb uninstall: invalid package '{package}'"
                            );
                            return Ok(());
                        }
                    };
                    if let Err(e) = self.uninstall_package(&package).await {
                        warn!(
                            error = e.as_ref() as &dyn Error,
                            "Line {line_num}: adb uninstall: failed to uninstall package \
                             '{package}'"
                        );
                    }
                }
                "shell" => {
                    let adb_args = adb_args_raw;
                    ensure!(!adb_args.is_empty(), "Line {line_num}: adb shell: missing command");
                    // Handle special case for 'pm uninstall'
                    if adb_args.len() == 3 && adb_args[0] == "pm" && adb_args[1] == "uninstall" {
                        let package = &adb_args[2];
                        debug!(package, "Line {line_num}: uninstalling package");
                        let package = match PackageName::parse(package) {
                            Ok(p) => p,
                            Err(e) => {
                                warn!(
                                    error = e.as_ref() as &dyn Error,
                                    "Line {line_num}: adb shell uninstall: invalid package \
                                     '{package}'"
                                );
                                return Ok(());
                            }
                        };
                        if let Err(e) = self.uninstall_package(&package).await {
                            warn!(
                                error = e.as_ref() as &dyn Error,
                                "Line {line_num}: failed to uninstall package '{package}'"
                            );
                        }
                    } else {
                        let shell_cmd = adb_args
                            .iter()
                            .map(|arg| match arg.contains(' ') && !arg.starts_with(['"', '\'']) {
                                true => format!("\"{arg}\""),
                                false => arg.to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        debug!(shell_cmd, "Line {line_num}: executing shell command");
                        let output = self.shell(&shell_cmd).await.with_context(|| {
                            format!("Line {line_num}: failed to execute command '{shell_cmd}'")
                        })?;
                        debug!(output, "Line {line_num}: shell command output");
                    }
                }
                "push" => {
                    ensure!(
                        adb_args.len() == 2,
                        "Line {line_num}: adb push: wrong number of arguments: expected 2, got {}",
                        adb_args.len()
                    );
                    let source = script_dir.join(adb_args[0]);
                    let dest = UnixPath::new(&adb_args[1]);
                    debug!(source = %source.display(), dest = %dest.display(), "Line {line_num}: pushing directory");
                    if let Err(e) = self.push_any(&source, dest).await {
                        warn!(
                            error = e.as_ref() as &dyn Error,
                            "Line {line_num}: adb push: failed to push '{}' to '{}'",
                            source.display(),
                            dest.display()
                        )
                    }
                }
                "pull" => {
                    ensure!(
                        adb_args.len() == 2,
                        "Line {line_num}: adb pull: wrong number of arguments: expected 2, got {}",
                        adb_args.len()
                    );
                    let source = UnixPath::new(&adb_args[0]);
                    let dest = script_dir.join(adb_args[1]);
                    debug!(source = %source.display(), dest = %dest.display(), "Line {line_num}: pulling directory");
                    if let Err(e) = self.pull_any(source, &dest).await {
                        warn!(
                            error = e.as_ref() as &dyn Error,
                            "Line {line_num}: adb pull: failed to pull '{}' to '{}'",
                            adb_args[0],
                            adb_args[1]
                        )
                    }
                }
                _ => bail!("Line {line_num}: Unsupported ADB command '{command}'"),
            }
        }

        Ok(())
    }

    /// Sideloads an app by installing its APK and pushing OBB data if present
    ///
    /// # Arguments
    /// * `app_dir` - Path to directory containing the app files
    /// * `progress_sender` - Sender for progress updates
    #[instrument(level = "debug", skip(self, progress_sender), err)]
    pub(crate) async fn sideload_app(
        &self,
        app_dir: &Path,
        backups_location: &Path,
        progress_sender: UnboundedSender<SideloadProgress>,
        token: CancellationToken,
        auto_reinstall_on_conflict: bool,
    ) -> Result<()> {
        fn send_progress(
            progress_sender: &UnboundedSender<SideloadProgress>,
            status: &str,
            progress: Option<f32>,
        ) {
            let _ = progress_sender.send(SideloadProgress { status: status.to_string(), progress });
        }

        ensure!(app_dir.is_dir(), "App path must be a directory");

        send_progress(&progress_sender, "Enumerating files", None);
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(app_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            entries.push(entry);
        }

        if let Some(entry) = entries
            .iter()
            .find(|e| e.file_name().to_str().is_some_and(|n| n.to_lowercase() == "install.txt"))
        {
            send_progress(&progress_sender, "Executing install script", None);
            return self
                .execute_install_script(
                    &entry.path(),
                    backups_location,
                    token.clone(),
                    auto_reinstall_on_conflict,
                )
                .await
                .context("Failed to execute install script");
        }

        let apk_paths = entries
            .iter()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("apk"))
            .map(|e| e.path())
            .collect::<Vec<_>>();
        let apk_path = match apk_paths.len() {
            0 => bail!("No APK file found in app directory"),
            1 => &apk_paths[0],
            _ => bail!("Multiple APK files found in app directory"),
        };

        let apk_info = get_apk_info(apk_path).context("Failed to read APK info")?;
        let package_name = &apk_info.package_name;

        let obb_dir = entries.iter().find_map(|e| {
            if e.path().is_dir() {
                e.file_name()
                    .to_str()
                    .and_then(|n| if n == package_name { Some(e.path()) } else { None })
            } else {
                None
            }
        });

        send_progress(&progress_sender, "Installing APK", Some(0.0));
        let install_progress_scale = if obb_dir.is_some() { 0.5 } else { 1.0 };

        let (tx, mut rx) = mpsc::unbounded_channel::<SideloadProgress>();
        tokio::spawn(
            {
                let progress_sender = progress_sender.clone();
                async move {
                    while let Some(p) = rx.recv().await {
                        let scaled = p.progress.map(|v| v * install_progress_scale);
                        let status = if let Some(pr) = p.progress {
                            format!("Installing APK ({:.0}%)", pr * 100.0)
                        } else {
                            p.status
                        };
                        send_progress(&progress_sender, &status, scaled);
                    }
                }
            }
            .instrument(Span::current()),
        );
        self.install_apk_with_progress(
            apk_path,
            backups_location,
            tx,
            false,
            auto_reinstall_on_conflict,
        )
        .await?;

        if let Some(obb_dir) = obb_dir {
            let package_name = obb_dir
                .file_name()
                .and_then(|n| n.to_str())
                .context("Failed to get package name from OBB path")?;
            let remote_obb_path = UnixPath::new("/sdcard/Android/obb").join(package_name);

            let (tx, mut rx) = mpsc::unbounded_channel::<DirectoryTransferProgress>();
            tokio::spawn(
                {
                    let progress_sender = progress_sender.clone();
                    async move {
                        let mut last_update = Instant::now();
                        let mut last_file_index: Option<u64> = None;
                        while let Some(progress) = rx.recv().await {
                            let now = Instant::now();
                            if now.duration_since(last_update) < Duration::from_millis(300)
                                && (last_file_index == Some(progress.transferred_files as u64))
                            {
                                continue;
                            }
                            last_update = now;
                            last_file_index = Some(progress.transferred_files as u64);

                            let push_progress =
                                progress.transferred_bytes as f32 / progress.total_bytes as f32;
                            let file_progress = progress.current_file_progress.transferred_bytes
                                as f32
                                / progress.current_file_progress.total_bytes as f32;
                            // Show currently transferred file, but don't overflow on final progress when transferred_files==total_files
                            let current_count =
                                progress.total_files.min(progress.transferred_files + 1);
                            let status = format!(
                                "Pushing OBB {}/{} ({:.0}%)",
                                current_count,
                                progress.total_files,
                                file_progress * 100.0
                            );
                            send_progress(&progress_sender, &status, Some(push_progress * 0.5));
                        }
                    }
                }
                .instrument(Span::current()),
            );

            self.push_dir_with_progress(&obb_dir, &remote_obb_path, true, tx).await?;
        }

        Ok(())
    }

    /// Installs an APK on the device
    #[instrument(level = "debug", skip(self, apk_path, backups_location), err)]
    pub(super) async fn install_apk(
        &self,
        apk_path: &Path,
        backups_location: &Path,
        auto_reinstall_on_conflict: bool,
    ) -> Result<()> {
        info!(path = %apk_path.display(), "Installing APK");
        let (tx, mut _rx) = mpsc::unbounded_channel::<SideloadProgress>();
        // Drain in background to avoid unbounded buffer growth
        tokio::spawn(async move { while _rx.recv().await.is_some() {} });
        self.install_apk_with_progress(
            apk_path,
            backups_location,
            tx,
            false,
            auto_reinstall_on_conflict,
        )
        .await
    }

    /// Installs an APK on the device (with progress)
    #[instrument(level = "debug", skip(self, apk_path, progress_sender), err)]
    pub(crate) async fn install_apk_with_progress(
        &self,
        apk_path: &Path,
        backups_location: &Path,
        progress_sender: UnboundedSender<SideloadProgress>,
        did_reinstall: bool,
        auto_reinstall_on_conflict: bool,
    ) -> Result<()> {
        info!(path = %apk_path.display(), "Installing APK with progress");
        // Bridge inner f32 progress into SideloadProgress
        let (tx, mut rx) = mpsc::unbounded_channel::<f32>();
        tokio::spawn(
            {
                let progress_sender = progress_sender.clone();
                async move {
                    // Avoid overwriting reinstall status
                    if !did_reinstall {
                        while let Some(p) = rx.recv().await {
                            let _ = progress_sender.send(SideloadProgress {
                                status: "Installing APK".to_string(),
                                progress: Some(p),
                            });
                        }
                    }
                }
            }
            .instrument(Span::current()),
        );

        match self.inner.install_package_with_progress(apk_path, true, true, true, tx).await {
            Ok(_) => Ok(()),
            Err(DeviceError::PackageManagerError(msg)) => {
                info!(
                    error = msg,
                    "Package manager returned error, checking if reinstall is needed"
                );

                if (msg.contains("INSTALL_FAILED_VERSION_DOWNGRADE")
                    || msg.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE"))
                    && !did_reinstall
                    && auto_reinstall_on_conflict
                {
                    info!("Incompatible update, reinstalling. Reason: {}", msg);
                    let _ = progress_sender.send(SideloadProgress {
                        status: "Incompatible update, reinstalling".to_string(),
                        progress: None,
                    });
                    let apk_info =
                        get_apk_info(apk_path).context("Failed to get APK info for backup")?;
                    let package_name = PackageName::parse(&apk_info.package_name)
                        .context("Invalid package name in APK info")?;
                    let backup_path = self
                        .backup_app(
                            &package_name,
                            None,
                            backups_location,
                            &BackupOptions {
                                name_append: Some("reinstall".to_string()),
                                backup_apk: false,
                                backup_data: true,
                                backup_obb: false,
                                // Don't lose private data on reinstall, e.g. when the app is not debuggable
                                require_private_data: true,
                            },
                            CancellationToken::new(),
                        )
                        .await
                        .context("Failed to backup app for reinstall")?;
                    self.uninstall_package(&package_name)
                        .await
                        .context("Failed to uninstall package for reinstall")?;
                    Box::pin(self.install_apk_with_progress(
                        apk_path,
                        backups_location,
                        progress_sender,
                        true,
                        auto_reinstall_on_conflict,
                    ))
                    .await
                    .context("Failed to reinstall APK")?;
                    if let Some(backup_path) = backup_path {
                        self.restore_backup(&backup_path)
                            .await
                            .context("Failed to restore backup after reinstall")?;
                    }
                    Ok(())
                } else {
                    Err(DeviceError::PackageManagerError(msg).into())
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}
