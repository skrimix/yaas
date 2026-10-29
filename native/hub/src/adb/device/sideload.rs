use std::{
    error::Error,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use forensic_adb::{DeviceError, DirectoryTransferProgress, FileTransferProgress, UnixPath};
use lazy_regex::{Lazy, Regex, lazy_regex};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, info, instrument, trace, warn};

use super::{AdbDevice, backup::BackupOptions};
use crate::{
    adb::PackageName,
    archive::{decompress_archive, list_7z_in_dir},
    models::apk_info::get_apk_info,
};

/// Regex to split command arguments - handles quoted arguments with spaces
/// Note: This is a simplified parser for install scripts and may not handle all edge cases
static COMMAND_ARGS_REGEX: Lazy<Regex> = lazy_regex!(r#""[^"]*"|'[^']*'|[^\s]+"#);

/// Minimum interval between repeated progress updates within a single step
const PROGRESS_THROTTLE: Duration = Duration::from_millis(300);

/// Progress information for sideload operations
#[derive(Debug)]
pub(crate) struct SideloadProgress {
    pub status: String,
    pub progress: Option<f32>,
}

fn progress_fraction(transferred: u64, total: u64, zero_total: f32) -> f32 {
    if total == 0 { zero_total } else { (transferred as f32 / total as f32).clamp(0.0, 1.0) }
}

fn obb_progress(progress: &DirectoryTransferProgress) -> (f32, f32) {
    let completed_file_fraction = if progress.total_files == 0 {
        1.0
    } else {
        progress.transferred_files as f32 / progress.total_files as f32
    };
    let push_progress = progress_fraction(
        progress.transferred_bytes,
        progress.total_bytes,
        completed_file_fraction,
    );
    let completed_current_file = progress.current_file.is_none() && progress.transferred_files > 0;
    let file_progress = progress_fraction(
        progress.current_file_progress.transferred_bytes,
        progress.current_file_progress.total_bytes,
        if completed_current_file { 1.0 } else { 0.0 },
    );

    (push_progress, file_progress)
}

/// Polls `operation` to completion, forwarding progress updates received meanwhile
async fn drive_with_progress<T, P>(
    operation: impl Future<Output = T>,
    mut progress_rx: UnboundedReceiver<P>,
    mut on_progress: impl FnMut(P),
) -> T {
    let mut operation = std::pin::pin!(operation);
    loop {
        tokio::select! {
            result = &mut operation => return result,
            Some(progress) = progress_rx.recv() => on_progress(progress),
        }
    }
}

/// A supported install script command
#[derive(Debug, PartialEq, Eq)]
enum ScriptCommand {
    Install { apk: String },
    Uninstall { package: String },
    Shell { command: String },
    Push { source: String, dest: String },
    Pull { source: String, dest: String },
}

/// An install script command along with its source line number
#[derive(Debug, PartialEq, Eq)]
struct ScriptStep {
    line_num: usize,
    command: ScriptCommand,
}

/// Parses an install script into executable steps
fn parse_install_script(content: &str) -> Result<Vec<ScriptStep>> {
    let mut steps = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        let line_num = line_index + 1;
        // Remove comments and redirections
        let line = line.split('#').next().unwrap_or("").split("REM").next().unwrap_or("").trim();
        if line.is_empty() {
            trace!(line_num, "Skipping empty or comment line");
            continue;
        }

        let command = line.split('>').next().unwrap_or("").trim();
        ensure!(!command.is_empty(), "Line {line_num}: Line is empty after removing redirections");
        debug!(line_num, command, "Parsed command");

        let tokens: Vec<String> = COMMAND_ARGS_REGEX
            .find_iter(command)
            .map(|m| {
                let token = m.as_str();
                // Remove surrounding quotes but preserve the content
                if token.len() >= 2
                    && ((token.starts_with('"') && token.ends_with('"'))
                        || (token.starts_with('\'') && token.ends_with('\'')))
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
        let adb_args = adb_args_raw.iter().filter(|arg| !arg.starts_with('-')).collect::<Vec<_>>();

        let command = match adb_command.as_str() {
            "install" => {
                // We only care about the APK path
                let apk = adb_args
                    .iter()
                    .find(|arg| arg.ends_with(".apk"))
                    .with_context(|| format!("Line {line_num}: adb install: missing APK path"))?;
                ScriptCommand::Install { apk: apk.to_string() }
            }
            "uninstall" => {
                ensure!(
                    adb_args.len() == 1,
                    "Line {line_num}: adb uninstall: wrong number of arguments: expected 1, got {}",
                    adb_args.len()
                );
                ScriptCommand::Uninstall { package: adb_args[0].to_string() }
            }
            "shell" => {
                let adb_args = adb_args_raw;
                ensure!(!adb_args.is_empty(), "Line {line_num}: adb shell: missing command");
                // Handle special case for 'pm uninstall'
                if adb_args.len() == 3 && adb_args[0] == "pm" && adb_args[1] == "uninstall" {
                    ScriptCommand::Uninstall { package: adb_args[2].clone() }
                } else {
                    let command = adb_args
                        .iter()
                        .map(|arg| match arg.contains(' ') && !arg.starts_with(['"', '\'']) {
                            true => format!("\"{arg}\""),
                            false => arg.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    ScriptCommand::Shell { command }
                }
            }
            "push" => {
                ensure!(
                    adb_args.len() == 2,
                    "Line {line_num}: adb push: wrong number of arguments: expected 2, got {}",
                    adb_args.len()
                );
                ScriptCommand::Push {
                    source: adb_args[0].to_string(),
                    dest: adb_args[1].to_string(),
                }
            }
            "pull" => {
                ensure!(
                    adb_args.len() == 2,
                    "Line {line_num}: adb pull: wrong number of arguments: expected 2, got {}",
                    adb_args.len()
                );
                ScriptCommand::Pull {
                    source: adb_args[0].to_string(),
                    dest: adb_args[1].to_string(),
                }
            }
            _ => bail!("Line {line_num}: Unsupported ADB command '{command}'"),
        };
        steps.push(ScriptStep { line_num, command });
    }
    Ok(steps)
}

/// Reports install script progress as a fraction of all steps
struct ScriptProgress<'a> {
    sender: &'a UnboundedSender<SideloadProgress>,
    total_steps: usize,
    current_step: usize,
    last_update: Option<Instant>,
}

impl<'a> ScriptProgress<'a> {
    fn new(sender: &'a UnboundedSender<SideloadProgress>, total_steps: usize) -> Self {
        Self { sender, total_steps, current_step: 0, last_update: None }
    }

    /// Reports the status of the current step, with optional progress within it
    fn report(&mut self, status: &str, step_progress: Option<f32>) {
        self.last_update = Some(Instant::now());
        let total = self.total_steps.max(1) as f32;
        let progress =
            (self.current_step as f32 + step_progress.unwrap_or(0.0).clamp(0.0, 1.0)) / total;
        let _ = self.sender.send(SideloadProgress {
            status: format!("Step {}/{}: {status}", self.current_step + 1, self.total_steps),
            progress: Some(progress.min(1.0)),
        });
    }

    /// Same as [`Self::report`], but skips updates arriving faster than [`PROGRESS_THROTTLE`]
    fn report_throttled(&mut self, status: &str, step_progress: Option<f32>) {
        if self.last_update.is_some_and(|t| t.elapsed() < PROGRESS_THROTTLE) {
            return;
        }
        self.report(status, step_progress);
    }

    fn next_step(&mut self) {
        self.current_step += 1;
        self.last_update = None;
    }
}

impl AdbDevice {
    /// Executes an install script from the given path
    #[instrument(level = "debug", skip(self, progress_sender, token))]
    async fn execute_install_script(
        &self,
        script_path: &Path,
        backups_location: &Path,
        progress_sender: &UnboundedSender<SideloadProgress>,
        token: CancellationToken,
        auto_reinstall_on_conflict: bool,
    ) -> Result<()> {
        let script_content = tokio::fs::read_to_string(script_path)
            .await
            .context("Failed to read install script")?;
        let script_dir = script_path.parent().context("Failed to get script directory")?;
        let steps = parse_install_script(&script_content)?;
        let archives = list_7z_in_dir(script_dir)
            .await
            .context("Failed to list .7z archives in install folder")?;

        let total_steps = steps.len() + usize::from(!archives.is_empty());
        let mut progress = ScriptProgress::new(progress_sender, total_steps);

        // Unpack all 7z archives if present
        if !archives.is_empty() {
            for (index, archive) in archives.iter().enumerate() {
                progress.report(
                    &format!("Extracting archive {}/{}", index + 1, archives.len()),
                    Some(index as f32 / archives.len() as f32),
                );
                debug!(path = %archive.display(), "Decompressing 7z archive");
                decompress_archive(archive, script_dir, None, None, Some(token.clone()))
                    .await
                    .with_context(|| {
                        format!("Failed to decompress '{}' in install folder", archive.display())
                    })?;
            }
            progress.next_step();
        }

        for step in &steps {
            self.execute_script_step(
                step,
                script_dir,
                backups_location,
                auto_reinstall_on_conflict,
                &mut progress,
            )
            .await?;
            progress.next_step();
        }

        Ok(())
    }

    /// Executes a single install script step, reporting its progress
    async fn execute_script_step(
        &self,
        step: &ScriptStep,
        script_dir: &Path,
        backups_location: &Path,
        auto_reinstall_on_conflict: bool,
        progress: &mut ScriptProgress<'_>,
    ) -> Result<()> {
        let line_num = step.line_num;
        match &step.command {
            ScriptCommand::Install { apk } => {
                let apk_path = script_dir.join(apk);
                debug!(apk_path = %apk_path.display(), "Line {line_num}: adb install: installing APK");
                progress.report("Installing APK", Some(0.0));
                let (tx, rx) = mpsc::unbounded_channel::<SideloadProgress>();
                drive_with_progress(
                    Box::pin(self.install_apk_with_progress(
                        &apk_path,
                        backups_location,
                        tx,
                        false,
                        auto_reinstall_on_conflict,
                    )),
                    rx,
                    |p| match p.progress {
                        Some(value) => progress.report_throttled(
                            &format!("Installing APK ({:.0}%)", value * 100.0),
                            Some(value),
                        ),
                        None => progress.report(&p.status, None),
                    },
                )
                .await
                .with_context(|| {
                    format!(
                        "Line {line_num}: adb install: failed to install APK '{}'",
                        apk_path.display()
                    )
                })?;
            }
            ScriptCommand::Uninstall { package } => {
                debug!(package, "Line {line_num}: uninstalling package");
                progress.report(&format!("Uninstalling {package}"), None);
                let package = match PackageName::parse(package) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(
                            error = e.as_ref() as &dyn Error,
                            "Line {line_num}: uninstall: invalid package '{package}'"
                        );
                        return Ok(());
                    }
                };
                if let Err(e) = self.uninstall_package(&package).await {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Line {line_num}: uninstall: failed to uninstall package '{package}'"
                    );
                }
            }
            ScriptCommand::Shell { command } => {
                debug!(command, "Line {line_num}: executing shell command");
                progress.report("Running shell command", None);
                let output = self.shell(command).await.with_context(|| {
                    format!("Line {line_num}: failed to execute command '{command}'")
                })?;
                debug!(output, "Line {line_num}: shell command output");
            }
            ScriptCommand::Push { source, dest } => {
                let source = script_dir.join(source);
                let dest = UnixPath::new(dest);
                debug!(source = %source.display(), dest = %dest.display(), "Line {line_num}: pushing");
                progress.report("Pushing files", Some(0.0));
                let push_result = if source.is_dir() {
                    let exact_obb_path =
                        dest.display().to_string().starts_with("/sdcard/Android/obb/")
                            && source.file_name().and_then(|name| name.to_str())
                                == dest.file_name().and_then(|name| name.to_str());
                    let (tx, rx) = mpsc::unbounded_channel::<DirectoryTransferProgress>();
                    let push = async {
                        if exact_obb_path {
                            self.push_dir_to_path_with_progress(&source, dest, true, tx).await
                        } else {
                            self.push_dir_with_progress(&source, dest, false, tx).await
                        }
                    };
                    let mut last_file_index = None;
                    drive_with_progress(push, rx, |p| {
                        let (push_progress, file_progress) = obb_progress(&p);
                        // Show currently transferred file, but don't overflow on final progress when transferred_files==total_files
                        let current_count = p.total_files.min(p.transferred_files + 1);
                        let status = format!(
                            "Pushing files {}/{} ({:.0}%)",
                            current_count,
                            p.total_files,
                            file_progress * 100.0
                        );
                        if last_file_index == Some(p.transferred_files) {
                            progress.report_throttled(&status, Some(push_progress));
                        } else {
                            last_file_index = Some(p.transferred_files);
                            progress.report(&status, Some(push_progress));
                        }
                    })
                    .await
                } else if source.is_file() {
                    let (tx, rx) = mpsc::unbounded_channel::<FileTransferProgress>();
                    drive_with_progress(self.push_with_progress(&source, dest, tx), rx, |p| {
                        let file_progress =
                            progress_fraction(p.transferred_bytes, p.total_bytes, 1.0);
                        progress.report_throttled(
                            &format!("Pushing file ({:.0}%)", file_progress * 100.0),
                            Some(file_progress),
                        );
                    })
                    .await
                } else {
                    self.push_any(&source, dest).await
                };

                if let Err(e) = push_result {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Line {line_num}: adb push: failed to push '{}' to '{}'",
                        source.display(),
                        dest.display()
                    )
                }
            }
            ScriptCommand::Pull { source, dest } => {
                let remote_source = UnixPath::new(source);
                let local_dest = script_dir.join(dest);
                debug!(source = %remote_source.display(), dest = %local_dest.display(), "Line {line_num}: pulling");
                progress.report("Pulling files", None);
                if let Err(e) = self.pull_any(remote_source, &local_dest).await {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Line {line_num}: adb pull: failed to pull '{source}' to '{dest}'"
                    )
                }
            }
        }
        Ok(())
    }

    /// Sideloads an app by installing its APK and pushing OBB data if present
    ///
    /// # Arguments
    /// * `app_dir` - Path to directory containing the app files
    /// * `progress_sender` - Sender for progress updates
    #[instrument(level = "debug", skip(self, progress_sender, token))]
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

        ensure!(app_dir.exists(), "App directory not found: {}", app_dir.display());
        ensure!(app_dir.is_dir(), "App path is not a directory: {}", app_dir.display());

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
            return self
                .execute_install_script(
                    &entry.path(),
                    backups_location,
                    &progress_sender,
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
            let remote_obb_parent = UnixPath::new("/sdcard/Android/obb");

            let (tx, mut rx) = mpsc::unbounded_channel::<DirectoryTransferProgress>();
            tokio::spawn(
                {
                    let progress_sender = progress_sender.clone();
                    async move {
                        let mut last_update = Instant::now();
                        let mut last_file_index: Option<u64> = None;
                        while let Some(progress) = rx.recv().await {
                            let now = Instant::now();
                            if now.duration_since(last_update) < PROGRESS_THROTTLE
                                && (last_file_index == Some(progress.transferred_files as u64))
                            {
                                continue;
                            }
                            last_update = now;
                            last_file_index = Some(progress.transferred_files as u64);

                            let (push_progress, file_progress) = obb_progress(&progress);
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

            let remote_obb_path = remote_obb_parent.join(package_name);
            self.push_dir_to_path_with_progress(&obb_dir, &remote_obb_path, true, tx).await?;
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn directory_progress(
        total_files: usize,
        transferred_files: usize,
        total_bytes: u64,
        transferred_bytes: u64,
        current_file: bool,
        current_file_total_bytes: u64,
        current_file_transferred_bytes: u64,
    ) -> DirectoryTransferProgress {
        DirectoryTransferProgress {
            directory_name: None,
            total_files,
            transferred_files,
            total_bytes,
            transferred_bytes,
            current_file: current_file.then(|| "main.obb".to_string()),
            current_file_progress: FileTransferProgress {
                total_bytes: current_file_total_bytes,
                transferred_bytes: current_file_transferred_bytes,
            },
        }
    }

    #[test]
    fn initial_obb_progress_is_zero() {
        let progress = directory_progress(2, 0, 100, 0, false, 0, 0);

        assert_eq!(obb_progress(&progress), (0.0, 0.0));
    }

    #[test]
    fn final_obb_progress_is_complete() {
        let progress = directory_progress(2, 2, 100, 100, false, 0, 0);

        assert_eq!(obb_progress(&progress), (1.0, 1.0));
    }

    #[test]
    fn zero_byte_obb_progress_uses_completed_files() {
        let initial = directory_progress(1, 0, 0, 0, true, 0, 0);
        let complete = directory_progress(1, 1, 0, 0, false, 0, 0);

        assert_eq!(obb_progress(&initial), (0.0, 0.0));
        assert_eq!(obb_progress(&complete), (1.0, 1.0));
    }

    #[test]
    fn parses_install_script_steps() {
        let script = "\
REM Install script
7z x data.7z
adb uninstall com.example.app > nul
adb install -r -g \"My App.apk\"
adb shell pm uninstall com.example.old
adb shell echo 'hello world'
# comment line
adb push com.example.app /sdcard/Android/obb/
adb pull /sdcard/save.dat save.dat
";

        let steps = parse_install_script(script).unwrap();
        let commands: Vec<_> = steps.iter().map(|s| (s.line_num, &s.command)).collect();

        assert_eq!(
            commands,
            vec![
                (3, &ScriptCommand::Uninstall { package: "com.example.app".into() }),
                (4, &ScriptCommand::Install { apk: "My App.apk".into() }),
                (5, &ScriptCommand::Uninstall { package: "com.example.old".into() }),
                (6, &ScriptCommand::Shell { command: "echo \"hello world\"".into() }),
                (
                    8,
                    &ScriptCommand::Push {
                        source: "com.example.app".into(),
                        dest: "/sdcard/Android/obb/".into(),
                    }
                ),
                (
                    9,
                    &ScriptCommand::Pull {
                        source: "/sdcard/save.dat".into(),
                        dest: "save.dat".into(),
                    }
                ),
            ]
        );
    }

    #[test]
    fn rejects_unsupported_install_script_commands() {
        assert!(parse_install_script("adb install app.apk\nrm -rf /").is_err());
        assert!(parse_install_script("adb reboot").is_err());
        assert!(parse_install_script("adb push only_source").is_err());
    }

    #[test]
    fn keeps_lone_quote_tokens() {
        let steps = parse_install_script("adb shell echo ' \" foo").unwrap();

        assert_eq!(
            steps,
            vec![ScriptStep {
                line_num: 1,
                command: ScriptCommand::Shell { command: "echo ' \" foo".into() },
            }]
        );
    }

    #[test]
    fn script_progress_spans_all_steps() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut progress = ScriptProgress::new(&tx, 4);

        progress.report("Extracting", Some(0.5));
        progress.next_step();
        progress.report("Installing APK", None);
        progress.next_step();
        progress.next_step();
        progress.report("Pushing files", Some(1.0));

        let updates: Vec<_> =
            std::iter::from_fn(|| rx.try_recv().ok()).map(|p| (p.status, p.progress)).collect();
        assert_eq!(
            updates,
            vec![
                ("Step 1/4: Extracting".to_string(), Some(0.125)),
                ("Step 2/4: Installing APK".to_string(), Some(0.25)),
                ("Step 4/4: Pushing files".to_string(), Some(1.0)),
            ]
        );
    }
}
