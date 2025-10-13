use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use derive_more::Debug;
use forensic_adb::{Device, DirectoryTransferProgress, UnixFileStatus, UnixPath, UnixPathBuf};
use lazy_regex::{Lazy, Regex, lazy_regex};
use time::{OffsetDateTime, macros::format_description};
use tokio::{
    fs::{self, File},
    io::BufReader,
    sync::mpsc::{self, UnboundedSender},
};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, instrument, trace, warn};

use crate::{
    adb::{PACKAGE_NAME_REGEX, ensure_valid_package},
    apk::get_apk_info,
    models::{
        InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, parse_list_apps_dex,
        signals::adb::command::RebootMode,
        vendor::quest_controller::{
            self, CONTROLLER_INFO_COMMAND_DUMPSYS, CONTROLLER_INFO_COMMAND_JSON,
            HeadsetControllersInfo,
        },
    },
    utils::{dir_has_any_files, first_subdirectory, remove_child_dir_if_exists},
};

/// Java tool used for package listing
static LIST_APPS_DEX_BYTES: &[u8] = include_bytes!("../../assets/list_apps.dex");

/// Regex to split command arguments - handles quoted arguments with spaces
/// Note: This is a simplified parser for install scripts and may not handle all edge cases
static COMMAND_ARGS_REGEX: Lazy<Regex> = lazy_regex!(r#""[^"]*"|'[^']*'|[^\s]+"#);

/// Represents a connected Android device with ADB capabilities
#[derive(Debug, Clone)]
pub struct AdbDevice {
    #[debug(skip)]
    pub inner: Device,
    /// Human-readable device name
    pub name: Option<String>,
    /// Product identifier from device
    pub product: String,
    /// Unique device serial number
    pub serial: String,
    /// True device serial number
    pub true_serial: String,
    /// ADB transport ID
    pub transport_id: String,
    /// True if connected over TCP/IP (adb over network)
    pub is_wireless: bool,
    /// Device battery level (0-100)
    pub battery_level: u8,
    /// Information about connected controllers
    pub controllers: HeadsetControllersInfo,
    /// Device storage space information
    pub space_info: SpaceInfo,
    /// List of installed packages on the device
    #[debug("({} items)", installed_packages.len())]
    pub installed_packages: Vec<InstalledPackage>,
}

impl Display for AdbDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name.as_ref().unwrap_or(&"Unknown".to_string()), self.serial)
    }
}

impl AdbDevice {
    /// Creates a new AdbDevice instance and initializes its state
    ///
    /// # Arguments
    /// * `inner` - The underlying forensic_adb Device instance
    #[instrument(skip(inner), ret, err)]
    pub async fn new(inner: Device) -> Result<Self> {
        let serial = inner.serial.clone();
        // Heuristic: wireless adb usually uses host:port as serial
        let is_wireless = serial.contains(':');
        let product = inner
            .info
            .get("product")
            .ok_or_else(|| anyhow!("No product name found in device info"))?
            .to_string();
        let true_serial = Self::query_true_serial(&inner).await?;
        let transport_id = inner
            .info
            .get("transport_id")
            .ok_or_else(|| anyhow!("No transport_id found in device info"))?
            .to_string();
        let mut device = Self {
            inner,
            name: None,
            product,
            serial,
            true_serial,
            transport_id,
            is_wireless,
            battery_level: 0,
            controllers: HeadsetControllersInfo::default(),
            space_info: SpaceInfo::default(),
            installed_packages: Vec::new(),
        };

        // Refresh identity first to use manufacturer + model if available
        if let Err(e) = device.refresh_identity().await {
            warn!(error = %e, "Failed to refresh device identity, using fallback name");
        }
        device.refresh().await.context("Failed to refresh device info")?;
        Ok(device)
    }

    /// Refresh basic identity (name) using `ro.product.manufacturer` and `ro.product.model`
    #[instrument(skip(self), err)]
    async fn refresh_identity(&mut self) -> Result<()> {
        let identity = Self::query_identity(&self.inner).await?;
        self.name = Some(identity);
        Ok(())
    }

    /// Queries manufacturer + model from a `forensic_adb::Device` and returns a combined string.
    /// If manufacturer is empty, returns just model.
    #[instrument(skip(device), err)]
    pub async fn query_identity(device: &Device) -> Result<String> {
        let manufacturer = tokio::time::timeout(
            Duration::from_millis(800),
            device.execute_host_shell_command("getprop ro.product.manufacturer"),
        )
        .await
        .context("Timed out reading ro.product.manufacturer")?
        .context("Failed to read ro.product.manufacturer")?
        .trim()
        .to_string();
        let model = tokio::time::timeout(
            Duration::from_millis(800),
            device.execute_host_shell_command("getprop ro.product.model"),
        )
        .await
        .context("Timed out reading ro.product.model")?
        .context("Failed to read ro.product.model")?
        .trim()
        .to_string();
        if !manufacturer.is_empty() && !model.is_empty() {
            Ok(format!("{} {}", manufacturer, model))
        } else if !model.is_empty() {
            Ok(model)
        } else {
            bail!("empty identity");
        }
    }

    /// Queries the true serial number from a `forensic_adb::Device`
    #[instrument(skip(device), err)]
    pub async fn query_true_serial(device: &Device) -> Result<String> {
        Ok(device
            .execute_host_shell_command("getprop ro.serialno")
            .await
            .context("Failed to read ro.serialno")?
            .trim()
            .to_string())
    }

    /// Refreshes device information (packages, battery, space)
    #[instrument(skip(self), err)]
    pub async fn refresh(&mut self) -> Result<()> {
        let mut errors = Vec::new();

        if let Err(e) = self.refresh_package_list().await {
            errors.push(("packages", e));
            self.installed_packages = Vec::new();
        }
        if let Err(e) = self.refresh_battery_info().await {
            errors.push(("battery", e));
            self.battery_level = 0;
            self.controllers = HeadsetControllersInfo::default();
        }
        if let Err(e) = self.refresh_space_info().await {
            errors.push(("space", e));
            self.space_info = SpaceInfo::default();
        }

        if !errors.is_empty() {
            let error_msg = errors
                .into_iter()
                .map(|(component, error)| format!("{component}: {error:#}"))
                .collect::<Vec<_>>()
                .join(", ");
            warn!(errors = error_msg, "Errors while refreshing device info");
        }

        Ok(())
    }

    /// Returns raw `dumpsys battery` output from the device
    #[instrument(skip(self), err)]
    pub async fn battery_dump(&self) -> Result<String> {
        self.shell_checked("dumpsys battery").await.context("'dumpsys battery' command failed")
    }

    /// Executes a shell command on the device
    #[instrument(skip(self), err)]
    async fn shell(&self, command: &str) -> Result<String> {
        self.inner
            .execute_host_shell_command(command)
            .await
            .context("Failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "Shell command executed"))
    }

    /// Executes a shell command and fails if exit code is non-zero.
    /// Appends `; printf %s $?` and parses the final line as the exit status.
    #[instrument(skip(self), err)]
    async fn shell_checked(&self, command: &str) -> Result<String> {
        let shell_output = self
            .shell(&format!("{} ; printf %s $?", command))
            .await
            .context(format!("Failed to execute checked shell command: {command}"))?;
        let (output, exit_code) =
            shell_output.rsplit_once('\n').context("Failed to extract exit code")?;
        if exit_code != "0" {
            error!(exit_code, output, "Shell command returned non-zero exit code");
            bail!("Command {command} failed with exit code {exit_code}. Output: {output}");
        }
        Ok(output.to_string())
    }

    /// Reboots the device with the given mode
    ///
    /// # Arguments
    /// * `mode` - The mode to reboot the device in (normal, bootloader, recovery, fastboot, power off)
    #[instrument(skip(self), err)]
    pub async fn reboot_with_mode(&self, mode: RebootMode) -> Result<()> {
        let cmd = match mode {
            RebootMode::Normal => "reboot",
            RebootMode::Bootloader => "reboot bootloader",
            RebootMode::Recovery => "reboot recovery",
            RebootMode::Fastboot => "reboot fastboot",
            RebootMode::PowerOff => "reboot -p",
        };
        self.shell_checked(cmd).await.context(format!("Failed to reboot with mode: {mode:?}"))?;
        Ok(())
    }

    /// Sets the proximity sensor state
    ///
    /// # Arguments
    /// * `enabled` - Whether to enable or disable the proximity sensor
    #[instrument(skip(self), err)]
    pub async fn set_proximity_sensor(&self, enabled: bool) -> Result<()> {
        // enable => automation_disable, disable => prox_close
        let cmd = if enabled {
            "am broadcast -a com.oculus.vrpowermanager.automation_disable"
        } else {
            "am broadcast -a com.oculus.vrpowermanager.prox_close"
        };
        self.shell_checked(cmd)
            .await
            .context(format!("Failed to set proximity sensor: {enabled}"))?;
        Ok(())
    }

    /// Sets the guardian paused state
    ///
    /// # Arguments
    /// * `paused` - Whether to pause or resume the guardian
    #[instrument(skip(self), err)]
    pub async fn set_guardian_paused(&self, paused: bool) -> Result<()> {
        let value = if paused { 1 } else { 0 };
        self.shell_checked(&format!("setprop debug.oculus.guardian_pause {value}"))
            .await
            .context(format!("Failed to set guardian paused: {paused}"))?;
        Ok(())
    }

    /// Refreshes the list of installed packages on the device
    #[instrument(skip(self), fields(count), err)]
    async fn refresh_package_list(&mut self) -> Result<()> {
        info!("Refreshing package list");
        self.push_bytes(LIST_APPS_DEX_BYTES, UnixPath::new("/data/local/tmp/list_apps.dex"))
            .await
            .context("Failed to push list_apps.dex")?;

        let list_output = self
            .shell_checked("CLASSPATH=/data/local/tmp/list_apps.dex app_process / Main")
            .await
            .context("Failed to execute app_process for list_apps.dex")?;

        let packages =
            parse_list_apps_dex(&list_output).context("Failed to parse list_apps.dex output")?;

        Span::current().record("count", packages.len());
        self.installed_packages = packages;
        Ok(())
    }

    /// Refreshes battery information for the device and controllers
    #[instrument(skip(self), err)]
    async fn refresh_battery_info(&mut self) -> Result<()> {
        // Get device battery level
        let battery_dump = self.battery_dump().await.context("Failed to get battery dump")?;

        let device_level: u8 = battery_dump
            .lines()
            .find_map(|line| {
                // Look for lines like "  level: 85"
                if line.trim().starts_with("level:") {
                    line.split(':').nth(1)?.trim().parse().ok()
                } else {
                    None
                }
            })
            .context("Failed to parse device battery level from dumpsys output")?;
        trace!(level = device_level, "Parsed device battery level");

        // Get controller battery levels using rstest first, then fall back to dumpsys
        let controllers = match self.shell_checked(CONTROLLER_INFO_COMMAND_JSON).await {
            Ok(json) => match quest_controller::parse_rstest_json(&json) {
                Ok(info) => info,
                Err(e) => {
                    warn!(error = %e, "Failed to parse rstest json, falling back to dumpsys");
                    let dump = self
                        .shell(CONTROLLER_INFO_COMMAND_DUMPSYS)
                        .await
                        .context("Failed to get controller info via dumpsys")?;
                    quest_controller::parse_dumpsys(&dump)
                }
            },
            Err(e) => {
                warn!(error = %e, "rstest command failed, falling back to dumpsys");
                let dump = self
                    .shell(CONTROLLER_INFO_COMMAND_DUMPSYS)
                    .await
                    .context("Failed to get controller info via dumpsys")?;
                quest_controller::parse_dumpsys(&dump)
            }
        };
        trace!(?controllers, "Parsed controller info");

        self.battery_level = device_level;
        self.controllers = controllers;
        Ok(())
    }

    /// Refreshes storage space information
    #[instrument(skip(self), err)]
    async fn refresh_space_info(&mut self) -> Result<()> {
        let space_info = self.get_space_info().await?;
        self.space_info = space_info;
        Ok(())
    }

    /// Gets storage space information from the device
    #[instrument(skip(self), err)]
    async fn get_space_info(&self) -> Result<SpaceInfo> {
        let output =
            self.shell_checked(SPACE_INFO_COMMAND).await.context("Space info command failed")?;
        SpaceInfo::from_stat_output(&output)
    }

    /// Launches an application on the device
    #[instrument(skip(self), err)]
    pub async fn launch(&self, package: &str) -> Result<()> {
        ensure_valid_package(package)?;
        // First try launching with VR category
        let output = self
            .shell(&format!("monkey -p {package} -c com.oculus.intent.category.VR 1"))
            .await
            .context("Failed to execute monkey command")?;

        if !output.contains("monkey aborted") {
            info!("Launched with VR category");
            return Ok(());
        }
        info!(output, "Monkey command with VR category failed");

        debug!("Retrying with default launch category");
        let output = self
            .shell(&format!("monkey -p {package} 1"))
            .await
            .context("Failed to execute monkey command")?;

        if output.contains("monkey aborted") {
            warn!(output, package, "Monkey command returned error");
            return Err(anyhow!("Failed to launch package '{}'", package));
        }

        info!("Launched with default category");
        Ok(())
    }

    /// Force stops an application on the device
    #[instrument(skip(self), err)]
    pub async fn force_stop(&self, package: &str) -> Result<()> {
        ensure_valid_package(package)?;
        self.inner.force_stop(package).await.context("Failed to force stop package")
    }

    /// Resolves the effective remote destination path for a push operation.
    ///
    /// Behavior:
    /// - If `dest` exists on the device and is a directory, the source file or directory name
    ///   is appended (push into that directory).
    /// - If `dest` exists and is a regular file, then:
    ///   - pushing a local file overwrites that remote file path;
    ///   - pushing a local directory is rejected (cannot push a dir to an existing file).
    /// - If `dest` does not exist but its parent exists, use `dest` as-is (the caller will create
    ///   directories/files as needed during the transfer).
    /// - If neither `dest` nor its parent exists, returns an error.
    ///
    /// This mirrors common `adb push` conventions and ensures we never silently place content
    /// at an unexpected path.
    #[instrument(level = "debug", ret, err)]
    async fn resolve_push_dest_path(&self, source: &Path, dest: &UnixPath) -> Result<UnixPathBuf> {
        let source_name = source
            .file_name()
            .context("Source path has no file name")?
            .to_str()
            .context("Source file name is not valid UTF-8")?;

        // Check if destination exists
        let dest_stat = self.inner.stat(dest).await;

        if let Ok(stat) = dest_stat {
            if stat.file_mode == UnixFileStatus::Directory {
                // If destination is a directory, append source file name
                Ok(UnixPathBuf::from(dest).join(source_name))
            } else if source.is_dir() {
                // Can't push directory to existing file
                bail!(
                    "Cannot push directory '{}' to existing file '{}'",
                    source.display(),
                    dest.display()
                )
            } else {
                // Use destination path as is
                Ok(UnixPathBuf::from(dest))
            }
        } else if let Some(parent) = dest.parent() {
            if self.inner.stat(parent).await.is_ok() {
                Ok(UnixPathBuf::from(dest))
            } else {
                bail!("Parent directory '{}' does not exist", parent.display())
            }
        } else {
            bail!("Invalid destination path: no parent directory")
        }
    }

    /// Resolves the effective local destination path for a pull operation.
    ///
    /// Behavior:
    /// - If `dest` exists locally and is a directory, append the source file/dir name and pull
    ///   into that directory.
    /// - If `dest` exists and is a regular file, then pulling a remote directory is rejected,
    ///   otherwise the remote file is saved to that file path.
    /// - If `dest` does not exist but its parent exists locally, use `dest` as-is (callers may
    ///   create intermediate directories as needed).
    /// - If `dest` has no existing parent directory, returns an error to avoid surprising
    ///   filesystem writes.
    ///
    /// This keeps pull semantics predictable and prevents accidental directory creation outside
    /// intended locations.
    #[instrument(level = "debug", ret, err)]
    async fn resolve_pull_dest_path(&self, source: &UnixPath, dest: &Path) -> Result<PathBuf> {
        let source_name = source
            .file_name()
            .context("Source path has no file name")?
            .to_str()
            .context("Source file name is not valid UTF-8")?;

        if dest.exists() {
            if dest.is_dir() {
                // If destination is a directory, append source file name
                Ok(dest.join(source_name))
            } else {
                // Can't pull to existing file if source is directory
                let source_is_dir = matches!(self.inner.stat(source).await, Ok(stat) if stat.file_mode == UnixFileStatus::Directory);
                if source_is_dir {
                    bail!(
                        "Cannot pull directory '{}' to existing file '{}'",
                        source.display(),
                        dest.display()
                    )
                } else {
                    Ok(dest.to_path_buf())
                }
            }
        } else if let Some(parent) = dest.parent() {
            if parent.exists() {
                // Parent exists, use destination path as is
                Ok(dest.to_path_buf())
            } else {
                bail!("Parent directory '{}' does not exist", parent.display())
            }
        } else {
            bail!("Invalid destination path: no parent directory")
        }
    }

    /// Pushes a file to the device
    ///
    /// # Arguments
    /// * `source_file` - Local path of the file to push
    /// * `dest_file` - Destination path on the device
    #[instrument(skip(self), err)]
    async fn push(&self, source_file: &Path, dest_file: &UnixPath) -> Result<()> {
        ensure!(
            source_file.is_file(),
            "Path does not exist or is not a file: {}",
            source_file.display()
        );

        let dest_path = self.resolve_push_dest_path(source_file, dest_file).await?;
        debug!(source = %source_file.display(), dest = %dest_path.display(), "Pushing file");
        let mut file = BufReader::new(File::open(source_file).await?);
        Box::pin(self.inner.push(&mut file, &dest_path, 0o777)).await.context("Failed to push file")
    }

    /// Pushes a directory to the device
    ///
    /// # Arguments
    /// * `source` - Local path of the directory to push
    /// * `dest` - Destination path on the device
    /// * `overwrite` - Whether to remove existing destination before pushing
    #[instrument(skip(self), err)]
    pub async fn push_dir(&self, source: &Path, dest: &UnixPath, overwrite: bool) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_push_dest_path(source, dest).await?;
        if overwrite {
            debug!(path = %dest_path.display(), "Cleaning up destination directory");
            self.shell(&format!("rm -rf '{}'", dest_path.display())).await?;
        }
        info!(source = %source.display(), dest = %dest_path.display(), "Pushing directory");
        Box::pin(self.inner.push_dir(source, &dest_path, 0o777))
            .await
            .context("Failed to push directory")
    }

    /// Pushes a directory to the device (with progress)
    ///
    /// # Arguments
    /// * `source` - Local directory path to push
    /// * `dest_dir` - Destination directory path on device
    /// * `overwrite` - Whether to clean up destination directory before pushing
    /// * `progress_sender` - Sender for progress updates
    #[instrument(skip(self, progress_sender), err)]
    async fn push_dir_with_progress(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
        progress_sender: UnboundedSender<DirectoryTransferProgress>,
    ) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_push_dest_path(source, dest).await?;
        // debug!(source = %source.display(), dest = %dest_path.display(), overwrite, "Pushing directory with progress");
        if overwrite {
            debug!(path = %dest_path.display(), "Cleaning up destination directory");
            self.shell(&format!("rm -rf '{}'", dest_path.display())).await?;
        }
        Box::pin(self.inner.push_dir_with_progress(source, &dest_path, 0o777, progress_sender))
            .await
            .context("Failed to push directory")
    }

    /// Pushes raw bytes to a file on the device
    #[instrument(skip(self, bytes), fields(len = bytes.len()), err)]
    async fn push_bytes(&self, mut bytes: &[u8], remote_path: &UnixPath) -> Result<()> {
        // debug!(len = bytes.len(), path = %remote_path.display(), "Pushing bytes");
        Box::pin(self.inner.push(&mut bytes, remote_path, 0o777))
            .await
            .context("Failed to push bytes")
    }

    /// Pulls a file from the device
    ///
    /// # Arguments
    /// * `source_file` - Source path on the device
    /// * `dest_file` - Local path to save the file
    #[instrument(skip(self), err)]
    async fn pull(&self, source_file: &UnixPath, dest_file: &Path) -> Result<()> {
        let source_stat =
            self.inner.stat(source_file).await.context("Failed to stat source file")?;
        ensure!(
            source_stat.file_mode == UnixFileStatus::RegularFile,
            "Source path is not a regular file: {}",
            source_file.display()
        );

        let dest_path = self.resolve_pull_dest_path(source_file, dest_file).await?;
        // debug!(source = %source_file.display(), dest = %dest_path.display(), "Pulling file");
        let mut file = File::create(&dest_path).await?;
        Box::pin(self.inner.pull(source_file, &mut file)).await?;
        Ok(())
    }

    /// Pulls a directory from the device
    ///
    /// # Arguments
    /// * `source` - Source path on the device
    /// * `dest` - Local path to save the directory
    #[instrument(skip(self), err)]
    async fn pull_dir(&self, source: &UnixPath, dest: &Path) -> Result<()> {
        let source_stat =
            self.inner.stat(source).await.context("Failed to stat source directory")?;
        ensure!(
            source_stat.file_mode == UnixFileStatus::Directory,
            "Source path is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_pull_dest_path(source, dest).await?;
        // Ensure the destination directory exists before pulling
        // For directory pulls, it's convenient to create the destination path automatically.
        // This mirrors typical `adb pull` behavior when targeting a new directory path.
        fs::create_dir_all(&dest_path).await.with_context(|| {
            format!("Failed to create destination directory: {}", dest_path.display())
        })?;
        // debug!(source = %source.display(), dest = %dest_path.display(), "Pulling directory");
        Box::pin(self.inner.pull_dir(source, &dest_path)).await.context("Failed to pull directory")
    }

    /// Pulls an item from the device.
    #[instrument(skip(self, remote_path, local_path))]
    async fn pull_any(&self, remote_path: &UnixPath, local_path: &Path) -> Result<()> {
        let stat = self.inner.stat(remote_path).await.context("Stat command failed")?;

        match stat.file_mode {
            UnixFileStatus::Directory => {
                // If destination exists and is a regular file, this is an error.
                if local_path.exists() && local_path.is_file() {
                    bail!(
                        "Cannot pull directory '{}' to existing file '{}'",
                        remote_path.display(),
                        local_path.display()
                    );
                }
                // `pull_dir` will ensure the destination directory exists (create_dir_all).
                self.pull_dir(remote_path, local_path).await?
            }
            UnixFileStatus::RegularFile => {
                // For files, allow non-existent destination paths as long as the parent exists.
                if let Some(parent) = local_path.parent() {
                    ensure!(
                        parent.exists(),
                        "Parent directory '{}' does not exist",
                        parent.display()
                    );
                }
                // If destination is a directory, `pull` will place the file inside it via
                // `resolve_pull_dest_path`. Otherwise it writes to the given file path.
                self.pull(remote_path, local_path).await?
            }
            other => bail!("Unsupported file type: {:?}", other),
        }
        Ok(())
    }

    /// Pushes an item to the device
    #[instrument(skip(self, source, dest), err)]
    async fn push_any(&self, source: &Path, dest: &UnixPath) -> Result<()> {
        ensure!(source.exists(), "Source path does not exist: {}", source.display());
        if source.is_dir() {
            self.push_dir(source, dest, false).await?;
        } else if source.is_file() {
            self.push(source, dest).await?;
        } else {
            bail!("Unsupported source file type: {}", source.display());
        }
        Ok(())
    }

    /// Installs an APK on the device
    #[instrument(skip(self, apk_path, backups_location), err)]
    pub async fn install_apk(&self, apk_path: &Path, backups_location: &Path) -> Result<()> {
        info!(path = %apk_path.display(), "Installing APK");
        let (tx, mut _rx) = mpsc::unbounded_channel::<SideloadProgress>();
        // Drain in background to avoid unbounded buffer growth
        tokio::spawn(async move { while _rx.recv().await.is_some() {} });
        self.install_apk_with_progress(apk_path, backups_location, tx, false).await
    }

    /// Installs an APK on the device (with progress)
    #[instrument(skip(self, apk_path, progress_sender), err)]
    pub async fn install_apk_with_progress(
        &self,
        apk_path: &Path,
        backups_location: &Path,
        progress_sender: UnboundedSender<SideloadProgress>,
        did_reinstall: bool,
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

        match Box::pin(self.inner.install_package_with_progress(apk_path, true, true, true, tx))
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_str = e.to_string();
                debug!(error = %error_str, "Install failed, checking error type");

                // TODO: add a settings flag to disable this behavior?
                if (error_str.contains("INSTALL_FAILED_VERSION_DOWNGRADE")
                    || error_str.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE"))
                    && !did_reinstall
                {
                    info!("Incompatible update, reinstalling. Reason: {}", error_str);
                    let _ = progress_sender.send(SideloadProgress {
                        status: "Incompatible update, reinstalling".to_string(),
                        progress: None,
                    });
                    let apk_info =
                        get_apk_info(apk_path).context("Failed to get APK info for backup")?;
                    let package_name = apk_info.package_name;
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
                    Err(e.into())
                }
            }
        }
    }

    /// Uninstalls a package from the device
    #[instrument(skip(self))]
    pub async fn uninstall_package(&self, package_name: &str) -> Result<()> {
        ensure_valid_package(package_name)?;
        match self.inner.uninstall_package(package_name).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_str = e.to_string();
                debug!(error = %error_str, "Uninstall failed, checking error type");

                if error_str.contains("DELETE_FAILED_INTERNAL_ERROR") {
                    // Check if package exists
                    let escaped = package_name.replace('.', "\\.");
                    let output = self
                        .shell(&format!("pm list packages | grep -w ^package:{escaped}$"))
                        .await
                        .unwrap_or_default();

                    if output.trim().is_empty() {
                        Err(anyhow!("Package not installed: {}", package_name))
                    } else {
                        Err(e.into())
                    }
                } else if error_str.contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") {
                    info!(
                        "Package {} is protected by device policy, trying to force uninstall",
                        package_name
                    );
                    self.shell(&format!("pm disable-user {package_name}")).await?;
                    self.inner
                        .uninstall_package(package_name)
                        .await
                        .map_err(Into::<anyhow::Error>::into)
                } else {
                    Err(e.into())
                }
            }
        }
        .context("Failed to uninstall package")
    }

    /// Executes an install script from the given path
    #[instrument(skip(self), err)]
    async fn execute_install_script(
        &self,
        script_path: &Path,
        backups_location: &Path,
        token: CancellationToken,
    ) -> Result<()> {
        let script_content = tokio::fs::read_to_string(script_path)
            .await
            .context("Failed to read install script")?;
        let script_dir = script_path.parent().context("Failed to get script directory")?;

        // Unpack all 7z archives if present
        crate::utils::decompress_all_7z_in_dir_cancellable(script_dir, token.clone())
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
                    // TODO: see if we should care about other arguments
                    let apk_path = script_dir.join(
                        adb_args.iter().find(|arg| arg.ends_with(".apk")).with_context(|| {
                            format!("Line {line_num}: adb install: missing APK path")
                        })?,
                    );
                    debug!(apk_path = %apk_path.display(), "Line {line_num}: adb install: installing APK");
                    self.install_apk(&apk_path, backups_location).await.with_context(|| {
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
                    if let Err(e) = self.uninstall_package(package).await {
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
                        if let Err(e) = self.uninstall_package(package).await {
                            warn!(
                                error = e.as_ref() as &dyn Error,
                                "Line {line_num}: failed to uninstall package '{package}'"
                            );
                        }
                    } else {
                        let shell_cmd = adb_args.join(" ");
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
    #[instrument(skip(self, progress_sender), err)]
    pub async fn sideload_app(
        &self,
        app_dir: &Path,
        backups_location: &Path,
        progress_sender: UnboundedSender<SideloadProgress>,
        token: CancellationToken,
    ) -> Result<()> {
        // TODO: add a test for this (smallest APK with generated OBB)
        fn send_progress(
            progress_sender: &UnboundedSender<SideloadProgress>,
            status: &str,
            progress: Option<f32>,
        ) {
            let _ = progress_sender.send(SideloadProgress { status: status.to_string(), progress });
        }

        // TODO: support direct streaming of app files
        // TODO: add optional checksum verification
        // TODO: check free space before proceeding
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
                .execute_install_script(&entry.path(), backups_location, token.clone())
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

        // TODO: read package name from APK file and use it to find OBB directory
        let obb_dir = entries.iter().find_map(|e| {
            if e.path().is_dir() {
                e.file_name().to_str().and_then(|n| {
                    if PACKAGE_NAME_REGEX.is_match(n) { Some(e.path()) } else { None }
                })
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
        self.install_apk_with_progress(apk_path, backups_location, tx, false).await?;

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
                            let status = format!(
                                "Pushing OBB {}/{} ({:.0}%)",
                                progress.transferred_files + 1,
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

    /// Returns true if a directory exists on the device
    #[instrument(skip(self), err)]
    async fn dir_exists(&self, path: &UnixPath) -> Result<bool> {
        match self.inner.stat(path).await {
            Ok(stat) => Ok(stat.file_mode == UnixFileStatus::Directory),
            Err(e) => {
                trace!(error = %e, path = %path.display(), "stat failed");
                Ok(false)
            }
        }
    }

    /// Gets APK path reported by `pm path <package>`
    #[instrument(skip(self), err)]
    async fn get_apk_path(&self, package_name: &str) -> Result<String> {
        ensure_valid_package(package_name)?;
        let output = self
            .shell_checked(&format!("pm path {package_name}"))
            .await
            .context("Failed to run 'pm path'")?;
        for line in output.lines() {
            if let Some(rest) = line.strip_prefix("package:") {
                let p = rest.trim();
                if !p.is_empty() {
                    return Ok(p.to_string());
                }
            }
        }
        bail!("Failed to parse APK path for package '{}': {}", package_name, output);
    }

    /// Creates a backup of the given package.
    /// Returns `Ok(Some(path))` if backup was created, `Ok(None)` if nothing to back up.
    #[instrument(skip(self), err)]
    pub async fn backup_app(
        &self,
        package_name: &str,
        display_name: Option<&str>,
        backups_location: &Path,
        options: &BackupOptions,
    ) -> Result<Option<PathBuf>> {
        // TODO: add a test for this

        // TODO: Restrict recursive deletions to the backup directory
        // Take backup_path as an argument to the macro
        // macro_rules! delete_dir {

        ensure_valid_package(package_name)?;
        ensure!(backups_location.is_dir(), "Backups location must be a directory");
        ensure!(
            !options.require_private_data || options.backup_data,
            "require_private_data requires backup_data"
        );

        info!(package = package_name, "Creating app backup");
        let fmt = format_description!("[year]-[month]-[day]_[hour]-[minute]-[second]");
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let timestamp = now.format(&fmt).unwrap_or_else(|_| "0000-00-00_00-00-00".into());
        // Build directory name: timestamp + sanitized display name (fallback to package name)
        let display = display_name
            .map(sanitize_filename::sanitize)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| package_name.to_string());
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

        let shared_data_path = UnixPath::new("/sdcard/Android/data").join(package_name);
        let private_data_path = UnixPath::new("/data/data").join(package_name);
        let obb_path = UnixPath::new("/sdcard/Android/obb").join(package_name);
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
            let tmp_pkg = tmp_root.join(package_name);
            let cmd = format!(
                "mkdir -p '{tmp}'; run-as {pkg} tar -cf - -C '{priv_path}' . | tar -xvf - -C \
                 '{tmp}'",
                tmp = tmp_pkg.display(),
                pkg = package_name,
                priv_path = private_data_path.display(),
            );
            let cmd_output = self.shell(&cmd).await?;
            if !cmd_output.is_empty() {
                debug!("Command output: {}", cmd_output);
            }
            if options.require_private_data && cmd_output.contains("run-as:") {
                bail!("Private data backup failed: run-as failed: {}", cmd_output);
            }
            self.pull_dir(&tmp_pkg, &private_data_backup_path).await?;
            let _ = self.shell("rm -rf /sdcard/backup_tmp/").await;

            let private_pkg_dir = private_data_backup_path.join(package_name);
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
                self.pull_dir(&shared_data_path, &shared_data_backup_path).await?;

                let shared_pkg_dir = shared_data_backup_path.join(package_name);
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
            let apk_remote = self.get_apk_path(package_name).await?;
            self.pull(UnixPath::new(&apk_remote), &backup_path).await?;
            backup_empty = false;
        }

        // Backup OBB
        if options.backup_obb {
            if self.dir_exists(&obb_path).await? {
                debug!("Backing up OBB");
                fs::create_dir_all(&obb_backup_path).await?;
                self.pull_dir(&obb_path, &obb_backup_path).await?;

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
            info!("Nothing backed up; cleaning up empty directory");
            let _ = fs::remove_dir_all(&backup_path).await;
            return Ok(None);
        }

        // Marker file
        let _ = File::create(backup_path.join(".backup")).await?;
        info!(path = %backup_path.display(), "Backup created successfully");
        Ok(Some(backup_path))
    }

    /// Restores a backup from the given path
    #[instrument(skip(self), err)]
    pub async fn restore_backup(&self, backup_path: &Path) -> Result<()> {
        // TODO: add a test for this
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
            // TODO: here and below, ensure there's a single valid directory
            && let Some(pkg_dir) = first_subdirectory(&obb_backup_path).await?
        {
            debug!("Restoring OBB");
            let remote_parent = UnixPath::new("/sdcard/Android/obb");
            self.push_dir(&pkg_dir, remote_parent, true).await?;
        }

        // Restore shared data
        if shared_data_backup_path.is_dir()
            && let Some(pkg_dir) = first_subdirectory(&shared_data_backup_path).await?
        {
            debug!("Restoring shared data");
            let remote_parent = UnixPath::new("/sdcard/Android/data");
            self.push_dir(&pkg_dir, remote_parent, true).await?;
        }

        // Restore private data
        if private_data_backup_path.is_dir()
            && let Some(pkg_dir) = first_subdirectory(&private_data_backup_path).await?
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

    #[instrument(skip(self), err)]
    pub async fn clean_temp_apks(&self) -> Result<()> {
        debug!("Cleaning up temporary APKs");
        self.shell("rm -rf /data/local/tmp/*.apk").await?;
        Ok(())
    }
}

// TODO: move somewhere else?
#[derive(Debug)]
pub struct SideloadProgress {
    pub status: String,
    pub progress: Option<f32>,
}

/// Options to control backup behavior
#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
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
