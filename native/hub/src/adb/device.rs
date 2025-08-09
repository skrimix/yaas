use std::{fmt::Display, path::Path};

use anyhow::{Context, Result, anyhow, bail, ensure};
use derive_more::Debug;
use forensic_adb::{Device, DirectoryTransferProgress, UnixFileStatus, UnixPath, UnixPathBuf};
use lazy_regex::{Lazy, Regex, lazy_regex};
use tokio::{
    fs::File,
    io::BufReader,
    sync::mpsc::{self, UnboundedSender},
};
use tracing::{Instrument, Span, debug, error, info, instrument, trace, warn};

use crate::{
    adb::PACKAGE_NAME_REGEX,
    models::{
        DeviceType, InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, parse_list_apps_dex,
        vendor::quest::controller::{
            CONTROLLER_INFO_COMMAND, HeadsetControllersInfo, parse_dumpsys,
        },
    },
};

/// Java tool used for package listing
static LIST_APPS_DEX_BYTES: &[u8] = include_bytes!("../../assets/list_apps.dex");

// TODO: this or `r#"[\"]?.+?[\"]|[^ ]+"#`? Verify that it's correct
/// Regex to split command arguments
static COMMAND_ARGS_REGEX: Lazy<Regex> = lazy_regex!(r#"[\"]?.+?[\"]|[^ ]+"#);

/// Represents a connected Android device with ADB capabilities
#[derive(Debug, Clone)]
pub struct AdbDevice {
    #[debug(skip)]
    pub inner: Device,
    /// Human-readable device name
    pub name: String,
    /// Product identifier from device
    pub product: String,
    /// Type of device (e.g. Quest, Quest2, etc.)
    pub device_type: DeviceType,
    /// Unique device serial number
    pub serial: String,
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
        write!(f, "{} ({})", self.name, self.serial)
    }
}

impl AdbDevice {
    /// Creates a new AdbDevice instance and initializes its state
    ///
    /// # Arguments
    /// * `inner` - The underlying forensic_adb Device instance
    #[instrument(skip(inner), err)]
    pub async fn new(inner: Device) -> Result<Self> {
        let serial = inner.serial.clone();
        let product = inner
            .info
            .get("product")
            .ok_or_else(|| anyhow!("No product name found in device info"))?
            .to_string();
        let device_type = DeviceType::from_product_name(&product);
        let name = match device_type {
            DeviceType::Unknown => format!("Unknown ({product})"),
            _ => device_type.to_string(),
        };
        let mut device = Self {
            inner,
            name,
            product,
            device_type,
            serial,
            battery_level: 0,
            controllers: HeadsetControllersInfo::default(),
            space_info: SpaceInfo::default(),
            installed_packages: Vec::new(),
        };
        Box::pin(device.refresh()).await.context("Failed to refresh device info")?;
        Ok(device)
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

    /// Executes a shell command on the device
    // TODO: Add `check_exit_code` parameter
    #[instrument(skip(self), err)]
    async fn shell(&self, command: &str) -> Result<String> {
        self.inner
            .execute_host_shell_command(command)
            .await
            .context("Failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "Shell command executed"))
    }

    /// Refreshes the list of installed packages on the device
    #[instrument(skip(self), fields(count), err)]
    async fn refresh_package_list(&mut self) -> Result<()> {
        info!("Refreshing package list");
        self.push_bytes(LIST_APPS_DEX_BYTES, UnixPath::new("/data/local/tmp/list_apps.dex"))
            .await
            .context("Failed to push list_apps.dex")?;

        let shell_output = self
            .shell("CLASSPATH=/data/local/tmp/list_apps.dex app_process / Main ; echo -n $?")
            .await
            .context("Failed to execute app_process for list_apps.dex")?;

        let (list_output, exit_code) =
            shell_output.rsplit_once('\n').context("Failed to extract exit code")?;

        if exit_code != "0" {
            error!(
                exit_code,
                output = list_output,
                "app_process command returned non-zero exit code"
            );
            return Err(anyhow!("app_process command failed with exit code {}", exit_code));
        }

        let packages =
            parse_list_apps_dex(list_output).context("Failed to parse list_apps.dex output")?;

        Span::current().record("count", packages.len());
        self.installed_packages = packages;
        Ok(())
    }

    /// Refreshes battery information for the device and controllers
    #[instrument(skip(self), err)]
    async fn refresh_battery_info(&mut self) -> Result<()> {
        // Get device battery level
        let device_level: u8 = self
            .shell("dumpsys battery | grep '  level' | awk '{print $2}'")
            .await
            .context("Failed to get device battery level")?
            .trim()
            .parse()
            .context("Failed to parse device battery level")?;
        trace!(level = device_level, "Parsed device battery level");

        // Get controller battery levels
        let dump_result = self
            .shell(CONTROLLER_INFO_COMMAND)
            .await
            .context("Failed to get controller battery level")?;
        let controllers = parse_dumpsys(&dump_result);
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
        let output = self.shell(SPACE_INFO_COMMAND).await.context("Failed to get space info")?;
        SpaceInfo::from_stat_output(&output)
    }

    /// Launches an application on the device
    #[instrument(skip(self), err)]
    pub async fn launch(&self, package: &str) -> Result<()> {
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
        self.inner.force_stop(package).await.context("Failed to force stop package")
    }

    /// Resolves the destination path for a push operation
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

    /// Resolves the destination path for a pull operation
    async fn resolve_pull_dest_path(
        &self,
        source: &UnixPath,
        dest: &Path,
    ) -> Result<std::path::PathBuf> {
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
        self.inner.push(&mut file, &dest_path, 0o777).await.context("Failed to push file")
    }

    /// Pushes a directory to the device
    ///
    /// # Arguments
    /// * `source` - Local path of the directory to push
    /// * `dest` - Destination path on the device
    #[instrument(skip(self), err)]
    pub async fn push_dir(&self, source: &Path, dest: &UnixPath) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_push_dest_path(source, dest).await?;
        info!(source = %source.display(), dest = %dest_path.display(), "Pushing directory");
        self.inner.push_dir(source, &dest_path, 0o777).await.context("Failed to push directory")
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
            let output = self.shell(&format!("rm -rf {}", dest_path.display())).await?;
            debug!(output, "Cleaned up destination directory");
        }
        self.inner
            .push_dir_with_progress(source, &dest_path, 0o777, progress_sender)
            .await
            .context("Failed to push directory")
    }

    /// Pushes raw bytes to a file on the device
    #[instrument(skip(self, bytes), fields(len = bytes.len()), err)]
    async fn push_bytes(&self, mut bytes: &[u8], remote_path: &UnixPath) -> Result<()> {
        // debug!(len = bytes.len(), path = %remote_path.display(), "Pushing bytes");
        self.inner.push(&mut bytes, remote_path, 0o777).await.context("Failed to push bytes")
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
        self.inner.pull(source_file, &mut file).await?;
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
        // debug!(source = %source.display(), dest = %dest_path.display(), "Pulling directory");
        self.inner.pull_dir(source, &dest_path).await.context("Failed to pull directory")
    }

    /// Pulls an item from the device.
    #[instrument(skip(self, remote_path, local_path), err)]
    async fn pull_any(&self, remote_path: &UnixPath, local_path: &Path) -> Result<()> {
        ensure!(
            local_path.is_dir(),
            "Destination path does not exist or is not a directory: {}",
            local_path.display()
        );
        let stat = self.inner.stat(remote_path).await.context("Stat command failed")?;
        if stat.file_mode == UnixFileStatus::Directory {
            self.pull_dir(remote_path, local_path).await?;
        } else if stat.file_mode == UnixFileStatus::RegularFile {
            self.pull(remote_path, local_path).await?;
        } else {
            bail!("Unsupported file type: {:?}", stat.file_mode);
        }
        Ok(())
    }

    /// Pushes an item to the device
    #[instrument(skip(self, source, dest), err)]
    async fn push_any(&self, source: &Path, dest: &UnixPath) -> Result<()> {
        ensure!(source.exists(), "Source path does not exist: {}", source.display());
        if source.is_dir() {
            self.push_dir(source, dest).await?;
        } else if source.is_file() {
            self.push(source, dest).await?;
        } else {
            bail!("Unsupported source file type: {}", source.display());
        }
        Ok(())
    }

    /// Installs an APK on the device
    #[instrument(skip(self, apk_path), err)]
    pub async fn install_apk(&self, apk_path: &Path) -> Result<()> {
        info!(path = %apk_path.display(), "Installing APK");
        self.inner.install_package(apk_path, true, true).await.context("Failed to install APK")
    }

    /// Installs an APK on the device (with progress)
    #[instrument(skip(self, apk_path, progress_sender), err)]
    pub async fn install_apk_with_progress(
        &self,
        apk_path: &Path,
        progress_sender: UnboundedSender<f32>,
    ) -> Result<()> {
        info!(path = %apk_path.display(), "Installing APK with progress");
        self.inner
            .install_package_with_progress(apk_path, true, true, progress_sender)
            .await
            .context("Failed to install APK")
    }

    /// Uninstalls a package from the device
    #[instrument(skip(self), err)]
    pub async fn uninstall_package(&self, package_name: &str) -> Result<()> {
        match self.inner.uninstall_package(package_name).await {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("DELETE_FAILED_INTERNAL_ERROR") {
                    // Check if package exists
                    let escaped = package_name.replace('.', "\\.");
                    let output = self
                        .shell(&format!("pm list packages | grep -w ^package:{escaped}"))
                        .await
                        .unwrap_or_default();

                    if output.trim().is_empty() {
                        Err(anyhow!("Package not installed: {}", package_name))
                    } else {
                        Err(e.into())
                    }
                } else if e.to_string().contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") {
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
    async fn execute_install_script(&self, script_path: &Path) -> Result<()> {
        let script_content = tokio::fs::read_to_string(script_path)
            .await
            .context("Failed to read install script")?;
        let script_dir = script_path.parent().context("Failed to get script directory")?;

        // TODO: should this be moved elsewhere?
        // Unpack all 7z archives if present
        let mut dir = tokio::fs::read_dir(script_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            if entry.file_type().await?.is_file()
                && entry.path().extension().and_then(|e| e.to_str()) == Some("7z")
            {
                let path = entry.path();
                info!(path = %path.display(), "Decompressing 7z archive");
                let script_dir_clone = script_dir.to_path_buf();
                tokio::task::spawn_blocking(move || {
                    sevenz_rust2::decompress_file(&path, script_dir_clone)
                        .context("Error decompressing 7z archive")
                })
                .await??;
            }
        }

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

            let tokens: Vec<&str> = COMMAND_ARGS_REGEX
                .find_iter(command)
                .map(|m| m.as_str().trim_matches('"'))
                .filter(|token| !token.starts_with('-'))
                .collect();

            if tokens[0] == "7z" {
                debug!(line_num, command, "Skipping 7z command");
                continue;
            }
            ensure!(tokens[0] == "adb", "Line {line_num}: Unsupported command '{command}'");

            ensure!(tokens.len() >= 2, "Line {line_num}: ADB command missing operation");
            let adb_command = tokens[1];
            let adb_args = tokens[2..].to_vec();

            match adb_command {
                "install" => {
                    // We only care about the APK path
                    // TODO: see if we should care about other arguments
                    let apk_path = script_dir.join(
                        adb_args.iter().find(|arg| arg.ends_with(".apk")).with_context(|| {
                            format!("Line {line_num}: adb install: missing APK path")
                        })?,
                    );
                    self.install_apk(&apk_path).await.with_context(|| {
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
                    let package = adb_args[0];
                    self.uninstall_package(package).await.with_context(|| {
                        format!(
                            "Line {line_num}: adb uninstall: failed to uninstall package \
                             '{package}'"
                        )
                    })?;
                }
                "shell" => {
                    ensure!(!adb_args.is_empty(), "Line {line_num}: adb shell: missing command");
                    // Handle special case for 'pm uninstall'
                    if adb_args.len() == 3 && adb_args[0] == "pm" && adb_args[1] == "uninstall" {
                        let package = adb_args[2];
                        self.uninstall_package(package).await.with_context(|| {
                            format!(
                                "Line {line_num}: adb shell: failed to uninstall package \
                                 '{package}'"
                            )
                        })?;
                    } else {
                        let shell_cmd = adb_args.join(" ");
                        self.shell(&shell_cmd).await.with_context(|| {
                            format!(
                                "Line {line_num}: adb shell: failed to execute command \
                                 '{shell_cmd}'"
                            )
                        })?;
                    }
                }
                "push" => {
                    ensure!(
                        adb_args.len() == 2,
                        "Line {line_num}: adb push: wrong number of arguments: expected 2, got {}",
                        adb_args.len()
                    );
                    let source = script_dir.join(adb_args[0]);
                    let dest = UnixPath::new(adb_args[1]);
                    self.push_any(&source, dest).await.with_context(|| {
                        format!(
                            "Line {line_num}: adb push: failed to push '{}' to '{}'",
                            source.display(),
                            dest.display()
                        )
                    })?;
                }
                "pull" => {
                    ensure!(
                        adb_args.len() == 2,
                        "Line {line_num}: adb pull: wrong number of arguments: expected 2, got {}",
                        adb_args.len()
                    );
                    let source = UnixPath::new(adb_args[0]);
                    let dest = script_dir.join(adb_args[1]);
                    self.pull_any(source, &dest).await.with_context(|| {
                        format!(
                            "Line {line_num}: adb pull: failed to pull '{}' to '{}'",
                            adb_args[0], adb_args[1]
                        )
                    })?;
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
        progress_sender: UnboundedSender<SideloadProgress>,
    ) -> Result<()> {
        fn send_progress(
            progress_sender: &UnboundedSender<SideloadProgress>,
            status: &str,
            progress: f32,
        ) {
            let _ = progress_sender.send(SideloadProgress { status: status.to_string(), progress });
        }

        // TODO: support direct streaming of app files
        // TODO: add optional checksum verification
        // TODO: check free space before proceeding
        ensure!(app_dir.is_dir(), "App path must be a directory");

        send_progress(&progress_sender, "Enumerating files", 0.0);
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(app_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            entries.push(entry);
        }

        if let Some(entry) = entries
            .iter()
            .find(|e| e.file_name().to_str().is_some_and(|n| n.to_lowercase() == "install.txt"))
        {
            send_progress(&progress_sender, "Executing install script", 0.5);
            return self
                .execute_install_script(&entry.path())
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

        send_progress(&progress_sender, "Installing APK", 0.0);
        let install_progress_scale = if obb_dir.is_some() { 0.5 } else { 1.0 };

        let (tx, mut rx) = mpsc::unbounded_channel::<f32>();
        tokio::spawn(
            {
                let progress_sender = progress_sender.clone();
                async move {
                    while let Some(progress) = rx.recv().await {
                        send_progress(
                            &progress_sender,
                            &format!("Installing APK ({:.0}%)", progress * 100.0),
                            progress * install_progress_scale,
                        );
                    }
                }
            }
            .instrument(Span::current()),
        );
        self.install_apk_with_progress(apk_path, tx).await?;

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
                        while let Some(progress) = rx.recv().await {
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
                            send_progress(&progress_sender, &status, 0.5 + push_progress * 0.5);
                        }
                    }
                }
                .instrument(Span::current()),
            );

            self.push_dir_with_progress(&obb_dir, &remote_obb_path, true, tx).await?;
        }

        Ok(())
    }
}

// TODO: move somewhere else?
#[derive(Debug)]
pub struct SideloadProgress {
    pub status: String,
    pub progress: f32,
}
