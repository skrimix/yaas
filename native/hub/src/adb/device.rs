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

static LIST_APPS_DEX_BYTES: LazyLock<Vec<u8>> =
    LazyLock::new(|| include_bytes!("../assets/list_apps.dex").to_vec());

#[derive(Debug, Clone)]
pub struct AdbDevice {
    #[debug(skip)]
    pub inner: Device,
    pub name: String,
    pub product: String,
    pub device_type: DeviceType,
    pub serial: String,
    pub battery_level: u8,
    pub controllers: ControllersInfo,
    pub space_info: SpaceInfo,
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
    #[instrument(level = "trace")]
    pub async fn new(inner: Device) -> Result<Self> {
        let serial = inner.serial.clone();
        let product = inner.info.get("product").expect("no product name found").to_string();
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
        device.refresh_all().await.context("failed to refresh device info")?;
        Ok(device)
    }

    #[instrument(level = "debug")]
    pub async fn refresh_all(&mut self) -> Result<()> {
        self.refresh().battery(true).space(true).packages(true).call().await
    }

    // TODO: periodic auto-refresh
    // #[instrument(err, level = "debug")] // BUG: segfault
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
        ensure!((packages || battery || space), "device info refresh called without any options");
        if packages {
            self.refresh_package_list().await.context("failed to refresh packages")?;
        }
        if battery {
            self.refresh_battery_info().await.context("failed to refresh battery info")?;
        }
        if space {
            self.refresh_space_info().await.context("failed to refresh space info")?;
        }
        Ok(())
    }

    #[instrument(err, level = "debug")]
    async fn shell(&self, command: &str) -> Result<String> {
        self.inner
            .execute_host_shell_command(command)
            .await
            .context("failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "shell command executed"))
    }

    // TODO: refresh after package operations
    // #[instrument(err, fields(result, count), level = "debug")] // BUG: segfault
    async fn refresh_package_list(&mut self) -> Result<()> {
        // pushing every time, but it's only 4.8kB, should be fine?
        self.push_bytes(&LIST_APPS_DEX_BYTES, UnixPath::new("/data/local/tmp/list_apps.dex"))
            .await
            .context("failed to push list_apps.dex")?;

        // use the "magic" tool to get detailed list of installed packages
        let shell_output = self
            .shell("CLASSPATH=/data/local/tmp/list_apps.dex app_process / Main ; echo -n $?")
            .await
            .context("failed to execute app_process for list_apps.dex")?;
        let (list_output, exit_code) =
            shell_output.rsplit_once('\n').context("failed to extract exit code")?;
        if exit_code != "0" {
            error!(
                exit_code = exit_code,
                output = list_output,
                "app_process command returned non-zero exit code"
            );
            return Err(anyhow!("app_process command returned non-zero exit code"));
        }

        let dumpsys_output = self.shell("dumpsys diskstats").await?;

        let packages = packages_from_device_output(list_output, &dumpsys_output)
            .context("failed to parse device output")?;

        Span::current().record("result", format!("found {} packages", packages.len()));
        Span::current().record("count", packages.len());

        self.installed_packages = packages;
        Ok(())
    }

    #[instrument(err, level = "debug")]
    async fn refresh_battery_info(&mut self) -> Result<()> {
        let device_level: u8 = self
            .shell("dumpsys battery | grep level | awk '{print $2}'")
            .await
            .context("failed to get device battery level")?
            .trim()
            .parse()
            .context("failed to parse device battery level")?;
        trace!(level = device_level, "parsed device battery level");

        let dump_result = self
            .shell(CONTROLLER_INFO_COMMAND)
            .await
            .context("failed to get controller battery level")?;
        let controllers = parse_dumpsys(&dump_result);

        self.battery_level = device_level;
        self.controllers = controllers;
        Ok(())
    }

    #[instrument(err, level = "debug")]
    async fn refresh_space_info(&mut self) -> Result<()> {
        let space_info = self.get_space_info().await?;
        self.space_info = space_info;
        Ok(())
    }

    #[instrument(err, level = "debug")]
    async fn get_space_info(&self) -> Result<SpaceInfo> {
        let output = self.shell(SPACE_INFO_COMMAND).await.context("failed to get space info")?;
        SpaceInfo::from_adb_output(&output)
    }

    #[instrument(err)]
    pub async fn launch(&self, package: &str) -> Result<()> {
        // TODO: try to use exit code to run both variants and determine result?
        let output = self
            .shell(format!("monkey -p {} -c com.oculus.intent.category.VR 1", package).as_str())
            .await
            .context("failed to execute monkey command")?;
        if !output.contains("monkey aborted") {
            return Ok(());
        }

        info!("monkey command failed with VR category, retrying with default");
        let output = self
            .shell(format!("monkey -p {} 1", package).as_str())
            .await
            .context("failed to execute monkey command")?;
        if output.contains("monkey aborted") {
            warn!(output = output, package = package, "monkey command returned error");
            // TODO: return output
            return Err(anyhow!("both monkey commands returned error"));
        }
        Ok(())
    }

    #[instrument(err)]
    pub async fn force_stop(&self, package: &str) -> Result<(), DeviceError> {
        self.inner.force_stop(package).await
    }

    #[instrument(err, level = "debug", fields(path = ?path.display(), remote_path = ?remote_path.display()))]
    async fn push(&self, path: &Path, remote_path: &UnixPath) -> Result<()> {
        if !path.is_file() {
            return Err(anyhow!("path does not exist or is not a file"));
        }
        let mut file = BufReader::new(File::open(path).await?);
        self.inner.push(&mut file, remote_path, 0o777).await.context("failed to push file")
    }

    #[instrument(err, level = "debug", fields(source = ?source.display(), dest_dir = ?dest_dir.display()))]
    pub async fn push_dir(&self, source: &Path, dest_dir: &UnixPath) -> Result<()> {
        if !source.is_dir() {
            return Err(anyhow!("source path does not exist or is not a directory"));
        }
        self.inner.push_dir(source, dest_dir, 0o777).await.context("failed to push directory")
    }

    #[instrument(err, level = "debug", skip(bytes, remote_path), fields(remote_path = ?remote_path.display()))]
    async fn push_bytes(&self, mut bytes: &[u8], remote_path: &UnixPath) -> Result<()> {
        self.inner.push(&mut bytes, remote_path, 0o777).await.context("failed to push bytes")
    }

    #[instrument(err, fields(apk_path = ?apk_path.display()))]
    pub async fn install_apk(&self, apk_path: &Path) -> Result<(), DeviceError> {
        // TODO: backup->reinstall->restore for incompatible updates
        self.inner.install_package(apk_path, true, true).await
    }

    #[instrument(err)]
    pub async fn uninstall_package(&self, package_name: &str) -> Result<()> {
        if let Err(e) = self.inner.uninstall_package(package_name).await {
            if e.to_string().contains("DELETE_FAILED_INTERNAL_ERROR") {
                let escaped = package_name.replace(".", "\\.");
                let output = self
                    .shell(&format!("pm list packages | grep -w ^package:{}", escaped))
                    .await
                    .unwrap_or_default();
                if output.trim().is_empty() {
                    bail!("package not installed");
                }
            } else if e.to_string().contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") {
                info!(
                    "package {} is protected by device policy, trying to force uninstall",
                    package_name
                );
                self.shell(&format!("pm disable-user {}", package_name)).await?;
                self.inner.uninstall_package(package_name).await?;
            }
            return Err(e.into());
        }
        Ok(())
    }

    pub fn into_proto(self) -> proto::AdbDevice {
        fn controller_to_proto(
            c: crate::models::vendor::quest::controller::ControllerInfo,
        ) -> proto::ControllerInfo {
            proto::ControllerInfo {
                battery_level: c.battery_level.map(|l| l as u32),
                status: match c.status {
                    ControllerStatus::Active => proto::ControllerStatus::Active,
                    ControllerStatus::Disabled => proto::ControllerStatus::Disabled,
                    ControllerStatus::Searching => proto::ControllerStatus::Searching,
                    ControllerStatus::Unknown => proto::ControllerStatus::Unknown,
                } as i32,
            }
        }

        let device_type = match self.device_type {
            DeviceType::Quest => proto::DeviceType::Quest,
            DeviceType::Quest2 => proto::DeviceType::Quest2,
            DeviceType::Quest3 => proto::DeviceType::Quest3,
            DeviceType::Quest3S => proto::DeviceType::Quest3s,
            DeviceType::QuestPro => proto::DeviceType::QuestPro,
            DeviceType::Unknown => proto::DeviceType::Unknown,
        };

        proto::AdbDevice {
            name: self.name,
            product: self.product,
            device_type: device_type as i32,
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
