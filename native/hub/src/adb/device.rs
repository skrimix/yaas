use std::{fmt::Display, path::Path, sync::LazyLock};

use anyhow::{Context, Result, anyhow, bail, ensure};
use bon::bon;
use derive_more::Debug;
use forensic_adb::{Device, DeviceError, UnixPath};
use tokio::{fs::File, io::BufReader};
use tracing::{Span, error, info, instrument, trace, warn};

use crate::{
    messages as proto,
    models::{
        DeviceType, InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, packages_from_device_output,
        vendor::quest::controller::{
            CONTROLLER_INFO_COMMAND, ControllerStatus, ControllersInfo, parse_dumpsys,
        },
    },
};

/// Path to the list_apps.dex file used for package listing
static LIST_APPS_DEX_BYTES: LazyLock<Vec<u8>> =
    LazyLock::new(|| include_bytes!("../assets/list_apps.dex").to_vec());

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
    pub controllers: ControllersInfo,
    /// Device storage space information
    pub space_info: SpaceInfo,
    /// List of installed packages on the device
    #[debug("({} items)", installed_packages.len())]
    pub installed_packages: Vec<InstalledPackage>,
}

impl Display for AdbDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.inner.serial)
    }
}

#[bon]
impl AdbDevice {
    /// Creates a new AdbDevice instance and initializes its state
    ///
    /// # Arguments
    /// * `inner` - The underlying forensic_adb Device instance
    ///
    /// # Returns
    /// Result containing the initialized AdbDevice or an error if initialization fails
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
            controllers: ControllersInfo::default(),
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
    ///
    /// # Returns
    /// Result indicating success or failure of the launch operation
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
    pub async fn force_stop(&self, package: &str) -> Result<(), DeviceError> {
        self.inner.force_stop(package).await
    }

    /// Pushes a file to the device
    ///
    /// # Arguments
    /// * `path` - Local path of the file to push
    /// * `remote_path` - Destination path on the device
    //#[instrument(err, level = "debug", fields(path = ?path.display(), remote_path = ?remote_path.display()))]
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

    /// Installs an APK on the device
    ///
    /// # Arguments
    /// * `apk_path` - Path to the APK file to install
    //#[instrument(err, fields(apk_path = ?apk_path.display()))]
    pub async fn install_apk(&self, apk_path: &Path) -> Result<(), DeviceError> {
        // TODO: Implement backup->reinstall->restore for incompatible updates
        self.inner.install_package(apk_path, true, true).await
    }

    /// Uninstalls a package from the device
    ///
    /// # Arguments
    /// * `package_name` - The package name to uninstall
    #[instrument(err)]
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
                        bail!("Package not installed: {}", package_name);
                    }
                    Err(e.into())
                } else if e.to_string().contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") {
                    // Try force uninstall for protected packages
                    info!(
                        "Package {} is protected by device policy, trying to force uninstall",
                        package_name
                    );
                    self.shell(&format!("pm disable-user {}", package_name)).await?;
                    self.inner.uninstall_package(package_name).await?;
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    /// Converts the AdbDevice instance into its protobuf representation
    pub fn into_proto(self) -> proto::AdbDevice {
        /// Helper function to convert controller info to protobuf
        fn controller_to_proto(
            controller: crate::models::vendor::quest::controller::ControllerInfo,
        ) -> proto::ControllerInfo {
            proto::ControllerInfo {
                battery_level: controller.battery_level.map(|l| l as u32),
                status: match controller.status {
                    ControllerStatus::Active => proto::ControllerStatus::Active,
                    ControllerStatus::Disabled => proto::ControllerStatus::Disabled,
                    ControllerStatus::Searching => proto::ControllerStatus::Searching,
                    ControllerStatus::Unknown => proto::ControllerStatus::Unknown,
                } as i32,
            }
        }

        /// Helper function to convert device type to protobuf
        fn device_type_to_proto(device_type: DeviceType) -> proto::DeviceType {
            match device_type {
                DeviceType::Quest => proto::DeviceType::Quest,
                DeviceType::Quest2 => proto::DeviceType::Quest2,
                DeviceType::Quest3 => proto::DeviceType::Quest3,
                DeviceType::Quest3S => proto::DeviceType::Quest3s,
                DeviceType::QuestPro => proto::DeviceType::QuestPro,
                DeviceType::Unknown => proto::DeviceType::Unknown,
            }
        }

        proto::AdbDevice {
            name: self.name,
            product: self.product,
            device_type: device_type_to_proto(self.device_type) as i32,
            serial: self.serial,
            battery_level: self.battery_level as u32,
            controllers: Some(proto::ControllersInfo {
                left: self.controllers.left.map(controller_to_proto),
                right: self.controllers.right.map(controller_to_proto),
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
}
