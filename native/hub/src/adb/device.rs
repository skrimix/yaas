use std::{fmt::Display, path::Path};

use anyhow::{Context, Result, anyhow, bail, ensure};
use derive_more::Debug;
use forensic_adb::{Device, UnixFileStatus, UnixPath, UnixPathBuf};
use lazy_regex::{Lazy, Regex, lazy_regex};
use tokio::{fs::File, io::BufReader};
use tracing::{Span, debug, error, info, trace, warn};

use crate::{
    adb::PACKAGE_NAME_REGEX,
    models::{
        DeviceType, InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, packages_from_device_output,
        vendor::quest::controller::{
            CONTROLLER_INFO_COMMAND, HeadsetControllersInfo, parse_dumpsys,
        },
    },
    signals::adb::device as device_signals,
};

/// Java tool used for package listing
static LIST_APPS_DEX_BYTES: &[u8] = include_bytes!("../assets/list_apps.dex");

/// Regex to split command arguments
static COMMAND_ARGS_REGEX: Lazy<Regex> = lazy_regex!(r#"[\"].+?[\"]|[^ ]+"#);

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
    //  #[instrument(level = "trace")]
    pub async fn new(inner: Device) -> Result<Self> {
        let serial = inner.serial.clone();
        let product = inner
            .info
            .get("product")
            .ok_or_else(|| anyhow!("No product name found in device info"))?
            .to_string();
        let device_type = DeviceType::from_product_name(&product);
        let name = match device_type {
            DeviceType::Unknown => format!("Unknown ({})", product),
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
        device.refresh().await.context("Failed to refresh device info")?;
        Ok(device)
    }

    /// Refreshes device information (packages, battery, space)
    pub async fn refresh(&mut self) -> Result<()> {
        let mut errors = Vec::new();

        if let Err(e) = self.refresh_package_list().await {
            errors.push(("packages", e));
        }
        if let Err(e) = self.refresh_battery_info().await {
            errors.push(("battery", e));
        }
        if let Err(e) = self.refresh_space_info().await {
            errors.push(("space", e));
        }

        if !errors.is_empty() {
            let error_msg = errors
                .into_iter()
                .map(|(component, error)| format!("{}: {}", component, error))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("Failed to refresh device info: {}", error_msg);
        }

        Ok(())
    }

    /// Executes a shell command on the device
    ///
    /// # Arguments
    /// * `command` - The shell command to execute
    ///
    /// # Returns
    /// Result containing the command output as a string
    //  #[instrument(err, level = "debug")]
    // TODO: Add `check_exit_code` parameter
    async fn shell(&self, command: &str) -> Result<String> {
        self.inner
            .execute_host_shell_command(command)
            .await
            .context("Failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "Shell command executed"))
    }

    /// Refreshes the list of installed packages on the device
    //  #[instrument(err, level = "debug")]
    async fn refresh_package_list(&mut self) -> Result<()> {
        // Push the list_apps.dex tool to device
        self.push_bytes(&LIST_APPS_DEX_BYTES, UnixPath::new("/data/local/tmp/list_apps.dex"))
            .await
            .context("Failed to push list_apps.dex")?;

        // Execute the "magic" tool and get package list
        let shell_output = self
            .shell("CLASSPATH=/data/local/tmp/list_apps.dex app_process / Main ; echo -n $?")
            .await
            .context("Failed to execute app_process for list_apps.dex")?;

        let (list_output, exit_code) =
            shell_output.rsplit_once('\n').context("Failed to extract exit code")?;

        if exit_code != "0" {
            error!(
                exit_code = exit_code,
                output = list_output,
                "app_process command returned non-zero exit code"
            );
            return Err(anyhow!("app_process command failed with exit code {}", exit_code));
        }

        // TODO: See if getting this through the list_apps.dex tool is better
        let dumpsys_output = self.shell("dumpsys diskstats").await?;

        let packages = packages_from_device_output(list_output, &dumpsys_output)
            .context("Failed to parse device output")?;

        Span::current().record("result", format!("found {} packages", packages.len()));
        Span::current().record("count", packages.len());

        self.installed_packages = packages;
        Ok(())
    }

    /// Refreshes battery information for the device and controllers
    //  #[instrument(err, level = "debug")]
    async fn refresh_battery_info(&mut self) -> Result<()> {
        // Get device battery level
        let device_level: u8 = self
            .shell("dumpsys battery | grep level | awk '{print $2}'")
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

        self.battery_level = device_level;
        self.controllers = controllers;
        Ok(())
    }

    /// Refreshes storage space information
    //  #[instrument(err, level = "debug")]
    async fn refresh_space_info(&mut self) -> Result<()> {
        let space_info = self.get_space_info().await?;
        self.space_info = space_info;
        Ok(())
    }

    /// Gets storage space information from the device
    //  #[instrument(err, level = "debug")]
    async fn get_space_info(&self) -> Result<SpaceInfo> {
        let output = self.shell(SPACE_INFO_COMMAND).await.context("Failed to get space info")?;
        SpaceInfo::from_stat_output(&output)
    }

    /// Launches an application on the device
    ///
    /// # Arguments
    /// * `package` - The package name to launch
    //  #[instrument(err)]
    pub async fn launch(&self, package: &str) -> Result<()> {
        // First try launching with VR category
        let output = self
            .shell(&format!("monkey -p {} -c com.oculus.intent.category.VR 1", package))
            .await
            .context("Failed to execute monkey command")?;

        if !output.contains("monkey aborted") {
            return Ok(());
        }

        // If VR launch fails, try default launch
        info!("Monkey command failed with VR category, retrying with default");
        let output = self
            .shell(&format!("monkey -p {} 1", package))
            .await
            .context("Failed to execute monkey command")?;

        if output.contains("monkey aborted") {
            warn!(output = output, package = package, "Monkey command returned error");
            return Err(anyhow!("Failed to launch package '{}'", package));
        }

        Ok(())
    }

    /// Force stops an application on the device
    ///
    /// # Arguments
    /// * `package` - The package name to force stop
    //  #[instrument(err)]
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
        } else {
            // Check if parent exists
            if let Some(parent) = dest.parent() {
                let parent_stat = self.inner.stat(parent).await;
                if parent_stat.is_ok() {
                    Ok(UnixPathBuf::from(dest))
                } else {
                    bail!("Parent directory '{}' does not exist", parent.display())
                }
            } else {
                bail!("Invalid destination path: no parent directory")
            }
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

        // Check if destination exists
        if dest.exists() {
            if dest.is_dir() {
                // If destination is a directory, append source file name
                Ok(dest.join(source_name))
            } else {
                // Can't pull to existing file if source is directory
                let source_is_dir = match self.inner.stat(source).await {
                    Ok(stat) => stat.file_mode == UnixFileStatus::Directory,
                    Err(_) => false,
                };
                if source_is_dir {
                    bail!(
                        "Cannot pull directory '{}' to existing file '{}'",
                        source.display(),
                        dest.display()
                    )
                } else {
                    // Use destination path as is for file
                    Ok(dest.to_path_buf())
                }
            }
        } else {
            // Check if parent exists
            if let Some(parent) = dest.parent() {
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
    }

    /// Pushes a file to the device
    ///
    /// # Arguments
    /// * `source_file` - Local path of the file to push
    /// * `dest_file` - Destination path on the device
    async fn push(&self, source_file: &Path, dest_file: &UnixPath) -> Result<()> {
        ensure!(
            source_file.is_file(),
            "Path does not exist or is not a file: {}",
            source_file.display()
        );

        let dest_path = self.resolve_push_dest_path(source_file, dest_file).await?;
        debug!(
            source_file = source_file.display().to_string(),
            dest_path = dest_path.display().to_string(),
            "Pushing file"
        );
        let mut file = BufReader::new(File::open(source_file).await?);
        self.inner.push(&mut file, &dest_path, 0o777).await.context("Failed to push file")
    }

    /// Pushes a directory to the device
    ///
    /// # Arguments
    /// * `source` - Local directory path to push
    /// * `dest_dir` - Destination directory path on device
    pub async fn push_dir(&self, source: &Path, dest: &UnixPath) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_push_dest_path(source, dest).await?;
        debug!(
            source = source.display().to_string(),
            dest_path = dest_path.display().to_string(),
            "Pushing directory"
        );
        self.inner.push_dir(source, &dest_path, 0o777).await.context("Failed to push directory")
    }

    /// Pushes raw bytes to a file on the device
    ///
    /// # Arguments
    /// * `bytes` - The bytes to push
    /// * `remote_path` - Destination path on the device
    // #[instrument(err, level = "debug", skip(bytes, remote_path))] // BUG: segfaults
    async fn push_bytes(&self, mut bytes: &[u8], remote_path: &UnixPath) -> Result<()> {
        debug!(
            bytes_len = bytes.len(),
            remote_path = remote_path.display().to_string(),
            "Pushing bytes"
        );
        self.inner.push(&mut bytes, remote_path, 0o777).await.context("Failed to push bytes")
    }

    /// Pulls a file from the device
    ///
    /// # Arguments
    /// * `source_file` - Remote path to the file on the device
    /// * `dest_file` - Destination local file path
    async fn pull(&self, source_file: &UnixPath, dest_file: &Path) -> Result<()> {
        // Verify source exists and is a file
        let source_stat =
            self.inner.stat(source_file).await.context("Failed to stat source file")?;
        ensure!(
            source_stat.file_mode == UnixFileStatus::RegularFile,
            "Source path is not a regular file: {}",
            source_file.display().to_string()
        );

        let dest_path = self.resolve_pull_dest_path(source_file, dest_file).await?;
        debug!(
            source_file = source_file.display().to_string(),
            dest_path = dest_path.display().to_string(),
            "Pulling file"
        );
        let mut file = File::create(&dest_path).await?;
        self.inner.pull(source_file, &mut file).await?;
        Ok(())
    }

    /// Pulls a directory from the device
    ///
    /// # Arguments
    /// * `source` - Remote path to the directory on the device
    /// * `dest` - Destination local directory path
    async fn pull_dir(&self, source: &UnixPath, dest: &Path) -> Result<()> {
        // Verify source exists and is a directory
        let source_stat =
            self.inner.stat(source).await.context("Failed to stat source directory")?;
        ensure!(
            source_stat.file_mode == UnixFileStatus::Directory,
            "Source path is not a directory: {}",
            source.display().to_string()
        );

        let dest_path = self.resolve_pull_dest_path(source, dest).await?;
        debug!(
            source = source.display().to_string(),
            dest_path = dest_path.display().to_string(),
            "Pulling directory"
        );
        self.inner.pull_dir(source, &dest_path).await.context("Failed to pull directory")
    }

    /// Pulls an item from the device. A stat command is used to determine if the remote path is a file or directory
    ///
    /// # Arguments
    /// * `remote_path` - Remote path to the file/directory on the device
    /// * `local_path` - Local path to save the file/directory on the local machine
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
    ///
    /// # Arguments
    /// * `source` - Local path to the file/directory to push
    /// * `dest` - Destination path on the device
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
    ///
    /// # Arguments
    /// * `apk_path` - Path to the APK file to install
    //#[instrument(err, fields(apk_path = ?apk_path.display()))]
    pub async fn install_apk(&self, apk_path: &Path) -> Result<()> {
        // TODO: Implement backup->reinstall->restore for incompatible updates
        self.inner.install_package(apk_path, true, true).await.context("Failed to install APK")
    }

    /// Uninstalls a package from the device
    ///
    /// # Arguments
    /// * `package_name` - The package name to uninstall
    //  #[instrument(err)]
    pub async fn uninstall_package(&self, package_name: &str) -> Result<()> {
        match self.inner.uninstall_package(package_name).await {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("DELETE_FAILED_INTERNAL_ERROR") {
                    // Check if package exists
                    let escaped = package_name.replace(".", "\\.");
                    let output = self
                        .shell(&format!("pm list packages | grep -w ^package:{}", escaped))
                        .await
                        .unwrap_or_default();

                    if output.trim().is_empty() {
                        Err(anyhow!("Package not installed: {}", package_name))
                    } else {
                        Err(e.into())
                    }
                } else if e.to_string().contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") {
                    // Try force uninstall for protected packages
                    info!(
                        "Package {} is protected by device policy, trying to force uninstall",
                        package_name
                    );
                    self.shell(&format!("pm disable-user {}", package_name)).await?;
                    self.inner.uninstall_package(package_name).await.map_err(Into::into)
                } else {
                    Err(e.into())
                }
            }
        }
        .context("Failed to uninstall package")
    }

    /// Converts the AdbDevice instance into its protobuf representation
    pub fn into_proto(self) -> device_signals::AdbDevice {
        device_signals::AdbDevice {
            name: self.name,
            product: self.product,
            device_type: self.device_type.into_proto(),
            serial: self.serial,
            battery_level: self.battery_level,
            controllers: device_signals::ControllersInfo {
                left: self.controllers.left.map(|c| c.into_proto()),
                right: self.controllers.right.map(|c| c.into_proto()),
            },
            space_info: device_signals::SpaceInfo {
                total: self.space_info.total,
                available: self.space_info.available,
            },
            installed_packages: self
                .installed_packages
                .into_iter()
                .map(InstalledPackage::into_proto)
                .collect(),
        }
    }

    /// Executes an install script from the given path
    ///
    /// # Arguments
    /// * `script_path` - Path to the install script file
    //  #[instrument(err, level = "debug")]
    async fn execute_install_script(&self, script_path: &Path) -> Result<()> {
        let script_content = tokio::fs::read_to_string(script_path)
            .await
            .context("Failed to read install script")?;
        let script_dir = script_path.parent().context("Failed to get script directory")?;

        // TODO: should this be moved elsewhere?
        // Unpack all 7z archives if present
        let mut dir = tokio::fs::read_dir(script_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            if entry.file_type().await.context("Failed to get directory entry file type")?.is_file()
                && entry.path().extension().and_then(|e| e.to_str()) == Some("7z")
            {
                tokio::task::spawn_blocking({
                    let path = entry.path();
                    debug!(path = path.display().to_string(), "Decompressing 7z archive");
                    let script_dir = script_dir.to_path_buf();
                    move || {
                        sevenz_rust::decompress_file(&path, script_dir)
                            .context("Error decompressing 7z archive")
                    }
                })
                .await??;
            }
        }

        for (line_index, line) in script_content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with("REM") {
                trace!(line = line, "Skipping empty or comment line");
                continue;
            }

            // Remove redirections
            let command = line.split('>').next().unwrap_or("").trim();
            ensure!(
                !command.is_empty(),
                "Line {}: Line is empty after removing redirections",
                line_index + 1
            );
            debug!(command = command, "Parsed command");

            let tokens: Vec<&str> = COMMAND_ARGS_REGEX
                .find_iter(command)
                .map(|m| m.as_str().trim_matches('"'))
                .filter(|token| !token.starts_with("-"))
                .collect();

            if tokens[0] == "7z" {
                debug!(line = line, "Skipping 7z command");
                continue;
            }
            ensure!(
                tokens[0] == "adb",
                "Line {}: Unsupported command '{}'",
                line_index + 1,
                command
            );

            ensure!(tokens.len() >= 2, "Line {}: Missing ADB subcommand", line_index + 1);
            let adb_command = tokens[1];
            let adb_args = tokens[2..].to_vec();

            match adb_command {
                "install" => {
                    // Find argument ending with .apk
                    let apk_path = script_dir.join(
                        adb_args.iter().find(|arg| arg.ends_with(".apk")).with_context(|| {
                            format!("Line {}: adb install: missing APK path", line_index + 1)
                        })?,
                    );
                    self.install_apk(&apk_path).await.with_context(|| {
                        format!(
                            "Line {}: adb install: failed to install APK '{}'",
                            line_index + 1,
                            apk_path.display()
                        )
                    })?;
                }
                "uninstall" => {
                    ensure!(
                        adb_args.len() == 1,
                        "Line {}: adb uninstall: wrong number of arguments: expected 1, got {}",
                        line_index + 1,
                        adb_args.len()
                    );
                    let package = adb_args[0];
                    self.uninstall_package(package).await.with_context(|| {
                        format!(
                            "Line {}: adb uninstall: failed to uninstall package '{}'",
                            line_index + 1,
                            package
                        )
                    })?;
                }
                "shell" => {
                    ensure!(
                        !adb_args.is_empty(),
                        "Line {}: adb shell: missing command",
                        line_index + 1
                    );

                    // Handle special case for 'pm uninstall'
                    if adb_args.len() == 3 && adb_args[0] == "pm" && adb_args[1] == "uninstall" {
                        let package = adb_args[2];
                        self.uninstall_package(package).await.with_context(|| {
                            format!(
                                "Line {}: adb shell: failed to uninstall package '{}'",
                                line_index + 1,
                                package
                            )
                        })?;
                    } else {
                        let shell_cmd = adb_args.join(" ");
                        self.shell(&shell_cmd).await.with_context(|| {
                            format!(
                                "Line {}: adb shell: failed to execute command '{}'",
                                line_index + 1,
                                shell_cmd
                            )
                        })?;
                    }
                }
                "push" => {
                    ensure!(
                        adb_args.len() == 2,
                        "Line {}: adb push: wrong number of arguments: expected 2, got {}",
                        line_index + 1,
                        adb_args.len()
                    );
                    let source = script_dir.join(adb_args[0]);
                    let dest = UnixPath::new(adb_args[1]);

                    self.push_any(&source, dest).await.with_context(|| {
                        format!(
                            "Line {}: adb push: failed to push file/directory '{}' to '{}'",
                            line_index + 1,
                            source.display(),
                            dest.display()
                        )
                    })?;
                }
                "pull" => {
                    ensure!(
                        adb_args.len() == 2,
                        "Line {}: adb pull: wrong number of arguments: expected 2, got {}",
                        line_index + 1,
                        adb_args.len()
                    );
                    let source = UnixPath::new(adb_args[0]);
                    let dest = script_dir.join(adb_args[1]);
                    self.pull_any(source, &dest).await.with_context(|| {
                        format!(
                            "Line {}: adb pull: failed to pull file/directory '{}' to '{}'",
                            line_index + 1,
                            adb_args[0],
                            adb_args[1]
                        )
                    })?;
                }
                _ => bail!("Line {}: Unsupported ADB command '{}'", line_index + 1, command),
            }
        }

        Ok(())
    }

    /// Sideloads an app by installing its APK and pushing OBB data if present
    ///
    /// # Arguments
    /// * `app_dir` - Path to directory containing the app files
    //  #[instrument(err)]
    pub async fn sideload_app(&self, app_dir: &Path) -> Result<()> {
        // TODO: support direct streaming of app files
        // TODO: add optional checksum verification
        // TODO: check free space before proceeding
        ensure!(app_dir.is_dir(), "App path must be a directory");

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(app_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            entries.push(entry);
        }

        // TODO: optimize multiple iterations
        // Execute install script if present
        for entry in &entries {
            if let Some(name) = entry.file_name().to_str() {
                if name.to_lowercase() == "install.txt" {
                    return self
                        .execute_install_script(&entry.path())
                        .await
                        .context("Failed to execute install script");
                }
            }
        }

        // Find APK file
        let mut apk_path = None;
        for entry in &entries {
            if entry.file_type().await.context("Failed to get directory entry file type")?.is_file()
                && entry.path().extension().and_then(|e| e.to_str()) == Some("apk")
            {
                if apk_path.is_some() {
                    bail!("Multiple APK files found in app directory");
                }
                apk_path = Some(entry.path());
            }
        }

        let apk_path = apk_path.context("No APK file found in app directory")?;

        // Look for OBB directory
        let mut obb_dir = None;
        for entry in &entries {
            if entry.file_type().await.context("Failed to get directory entry file type")?.is_dir()
            {
                let path = entry.path();
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    if PACKAGE_NAME_REGEX.is_match(dir_name) {
                        if obb_dir.is_some() {
                            bail!("Multiple possible OBB directories found");
                        }
                        obb_dir = Some(path);
                    }
                }
            }
        }

        // Install APK
        self.install_apk(&apk_path).await?;

        // Push OBB directory
        if let Some(obb_dir) = obb_dir {
            let package_name = obb_dir
                .file_name()
                .and_then(|n| n.to_str())
                .context("Failed to get package name from OBB path")?;
            let remote_obb_path = UnixPath::new("/sdcard/Android/obb").join(package_name);
            self.push_dir(&obb_dir, &remote_obb_path).await?;
        }

        Ok(())
    }
}
