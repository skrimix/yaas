use std::{fmt::Display, path::Path, sync::LazyLock};

use anyhow::{Context, Result, anyhow, bail, ensure};
use bon::bon;
use derive_more::Debug;
use forensic_adb::{Device, UnixFileStatus, UnixPath};
use lazy_regex::{Lazy, Regex, lazy_regex};
use tokio::{fs::File, io::BufReader};
use tracing::{Span, debug, error, info, instrument, trace, warn};

use crate::{
    adb::PACKAGE_NAME_REGEX,
    messages as proto,
    models::{
        DeviceType, InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, packages_from_device_output,
        vendor::quest::controller::{
            CONTROLLER_INFO_COMMAND, HeadsetControllersInfo, parse_dumpsys,
        },
    },
};

/// Path to the list_apps.dex file used for package listing
static LIST_APPS_DEX_BYTES: LazyLock<Vec<u8>> =
    LazyLock::new(|| include_bytes!("../assets/list_apps.dex").to_vec());

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

#[bon]
impl AdbDevice {
    /// Creates a new AdbDevice instance and initializes its state
    ///
    /// # Arguments
    /// * `inner` - The underlying forensic_adb Device instance
    #[instrument(level = "trace")]
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
        device.refresh_all().await.context("Failed to refresh device info")?;
        Ok(device)
    }

    /// Refreshes all device information (packages, battery, space)
    #[instrument(level = "debug")]
    pub async fn refresh_all(&mut self) -> Result<()> {
        self.refresh().battery(true).space(true).packages(true).call().await
    }

    /// Refreshes specific device information based on provided flags
    ///
    /// # Arguments
    /// * `packages` - Whether to refresh package list
    /// * `battery` - Whether to refresh battery info
    /// * `space` - Whether to refresh space info
    #[builder]
    pub async fn refresh(
        &mut self,
        packages: Option<bool>,
        battery: Option<bool>,
        space: Option<bool>,
    ) -> Result<()> {
        let packages = packages.unwrap_or(false);
        let battery = battery.unwrap_or(false);
        let space = space.unwrap_or(false);
        ensure!((packages || battery || space), "Device info refresh called without any options");

        let mut errors = Vec::new();

        if packages {
            if let Err(e) = self.refresh_package_list().await {
                errors.push(("packages", e));
            }
        }
        if battery {
            if let Err(e) = self.refresh_battery_info().await {
                errors.push(("battery", e));
            }
        }
        if space {
            if let Err(e) = self.refresh_space_info().await {
                errors.push(("space", e));
            }
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
    #[instrument(err, level = "debug")]
    // TODO: Add `check_exit_code` parameter
    async fn shell(&self, command: &str) -> Result<String> {
        self.inner
            .execute_host_shell_command(command)
            .await
            .context("Failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "Shell command executed"))
    }

    /// Refreshes the list of installed packages on the device
    #[instrument(err, level = "debug")]
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
    #[instrument(err, level = "debug")]
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
    #[instrument(err, level = "debug")]
    async fn refresh_space_info(&mut self) -> Result<()> {
        let space_info = self.get_space_info().await?;
        self.space_info = space_info;
        Ok(())
    }

    /// Gets storage space information from the device
    #[instrument(err, level = "debug")]
    async fn get_space_info(&self) -> Result<SpaceInfo> {
        let output = self.shell(SPACE_INFO_COMMAND).await.context("Failed to get space info")?;
        SpaceInfo::from_stat_output(&output)
    }

    /// Launches an application on the device
    ///
    /// # Arguments
    /// * `package` - The package name to launch
    #[instrument(err)]
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
    #[instrument(err)]
    pub async fn force_stop(&self, package: &str) -> Result<()> {
        self.inner.force_stop(package).await.context("Failed to force stop package")
    }

    /// Pushes a file to the device
    ///
    /// # Arguments
    /// * `path` - Local path of the file to push
    /// * `remote_path` - Destination path on the device
    // #[instrument(err, level = "debug", fields(path = ?path.display(), remote_path = ?remote_path.display()))]
    async fn push(&self, path: &Path, remote_path: &UnixPath) -> Result<()> {
        ensure!(path.is_file(), "Path does not exist or is not a file: {}", path.display());
        let mut file = BufReader::new(File::open(path).await?);
        self.inner.push(&mut file, remote_path, 0o777).await.context("Failed to push file")
    }

    /// Pushes a directory to the device
    ///
    /// # Arguments
    /// * `source` - Local directory path to push
    /// * `dest_dir` - Destination directory path on device
    //#[instrument(err, level = "debug", fields(source = ?source.display(), dest_dir = ?dest_dir.display()))]
    pub async fn push_dir(&self, source: &Path, dest_dir: &UnixPath) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );
        self.inner.push_dir(source, dest_dir, 0o777).await.context("Failed to push directory")
    }

    /// Pushes raw bytes to a file on the device
    ///
    /// # Arguments
    /// * `bytes` - The bytes to push
    /// * `remote_path` - Destination path on the device
    // #[instrument(err, level = "debug", skip(bytes, remote_path))] // BUG: segfaults
    async fn push_bytes(&self, mut bytes: &[u8], remote_path: &UnixPath) -> Result<()> {
        self.inner.push(&mut bytes, remote_path, 0o777).await.context("Failed to push bytes")
    }

    /// Pulls a file from the device
    ///
    /// # Arguments
    /// * `remote_file_path` - Remote path to the file on the device
    /// * `local_file_path` - Destination local file path
    // #[instrument(err, level = "debug", fields(remote_path = ?remote_path.display(), local_path = ?local_path.display()))]
    async fn pull(&self, remote_file_path: &UnixPath, local_file_path: &Path) -> Result<()> {
        if local_file_path.exists() {
            if local_file_path.is_file() {
                debug!(file = remote_file_path.display().to_string(), "Overwriting existing file");
            } else {
                bail!("Destination path is not a file: {}", local_file_path.display());
            }
        }
        let mut file = File::create(local_file_path).await?;
        self.inner.pull(remote_file_path, &mut file).await?;
        Ok(())
    }

    /// Pulls a directory from the device
    ///
    /// # Arguments
    /// * `remote_dir_path` - Remote path to the directory on the device
    /// * `local_dir_path` - Destination local directory path
    async fn pull_dir(&self, remote_dir_path: &UnixPath, local_dir_path: &Path) -> Result<()> {
        ensure!(
            local_dir_path.is_dir(),
            "Destination path does not exist or is not a directory: {}",
            local_dir_path.display()
        );
        self.inner
            .pull_dir(remote_dir_path, local_dir_path)
            .await
            .context("Failed to pull directory")
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

    /// Installs an APK on the device
    ///
    /// # Arguments
    /// * `apk_path` - Path to the APK file to install
    //#[instrument(err, fields(apk_path = ?apk_path.display()))]
    pub async fn install_apk(&mut self, apk_path: &Path) -> Result<()> {
        // TODO: Implement backup->reinstall->restore for incompatible updates
        let install_result = self.inner.install_package(apk_path, true, true).await;
        let _ = self.on_package_list_change().await;
        install_result.context("Failed to install APK")
    }

    /// Uninstalls a package from the device
    ///
    /// # Arguments
    /// * `package_name` - The package name to uninstall
    #[instrument(err)]
    pub async fn uninstall_package(&mut self, package_name: &str) -> Result<()> {
        let uninstall_result = match self.inner.uninstall_package(package_name).await {
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
        };
        let _ = self.on_package_list_change().await;
        uninstall_result
    }

    /// Converts the AdbDevice instance into its protobuf representation
    pub fn into_proto(self) -> proto::AdbDevice {
        proto::AdbDevice {
            name: self.name,
            product: self.product,
            device_type: self.device_type.into_proto() as i32,
            serial: self.serial,
            battery_level: self.battery_level as u32,
            controllers: Some(proto::ControllersInfo {
                left: self.controllers.left.map(|c| c.into_proto()),
                right: self.controllers.right.map(|c| c.into_proto()),
            }),
            space_info: Some(proto::SpaceInfo {
                total: self.space_info.total.into(),
                available: self.space_info.available.into(),
            }),
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
    #[instrument(err, level = "debug")]
    async fn execute_install_script(&mut self, script_path: &Path) -> Result<()> {
        let script_content = tokio::fs::read_to_string(script_path)
            .await
            .context("Failed to read install script")?;
        let script_dir = script_path.parent().context("Failed to get script directory")?;

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

                    ensure!(
                        source.exists(),
                        "Line {}: adb push: source path '{}' does not exist",
                        line_index + 1,
                        adb_args[0]
                    );

                    if source.is_dir() {
                        self.push_dir(&source, dest).await.with_context(|| {
                            format!(
                                "Line {}: adb push: failed to push directory '{}' to '{}'",
                                line_index + 1,
                                adb_args[0],
                                adb_args[1]
                            )
                        })?;
                    } else if source.is_file() {
                        self.push(&source, dest).await.with_context(|| {
                            format!(
                                "Line {}: adb push: failed to push file '{}' to '{}'",
                                line_index + 1,
                                adb_args[0],
                                adb_args[1]
                            )
                        })?;
                    } else {
                        bail!(
                            "Line {}: adb push: source path '{}' exists but is not a file or \
                             directory",
                            line_index + 1,
                            adb_args[0]
                        );
                    }
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
    #[instrument(err)]
    pub async fn sideload_app(&mut self, app_dir: &Path) -> Result<()> {
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

    async fn on_package_list_change(&mut self) -> Result<()> {
        self.refresh().packages(true).space(true).call().await
    }
}
