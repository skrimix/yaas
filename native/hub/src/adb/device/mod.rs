mod backup;
mod sideload;
mod transfer;

use std::{
    error::Error,
    fmt::Display,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
pub(crate) use backup::BackupOptions;
use bitflags::bitflags;
use const_format::concatcp;
use derive_more::Debug;
use forensic_adb::{Device, UnixPath};
use futures::FutureExt;
use lazy_regex::regex;
use sha2_const_stable::Sha256;
pub(crate) use sideload::SideloadProgress;
use tokio::{fs, time::sleep};
use tracing::{Span, debug, error, info, instrument, trace, warn};
pub(crate) mod battery_dump;

bitflags! {
    /// Device information fields that can be refreshed together.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct DeviceRefreshComponents: u8 {
        const PACKAGES = 1 << 0;
        const BATTERY_AND_CONTROLLERS = 1 << 1;
        const STORAGE = 1 << 2;
        const GUARDIAN = 1 << 3;
        const PROXIMITY = 1 << 4;
        const USB = 1 << 5;
    }
}

impl DeviceRefreshComponents {
    /// All declared refresh components.
    pub(super) const ALL: Self = Self::all();

    /// Package metadata and free space are refreshed together.
    pub(super) fn normalized(mut self) -> Self {
        if self.contains(Self::PACKAGES) {
            self |= Self::STORAGE;
        }
        self
    }
}

/// Successful values returned by a selective device query.
#[derive(Debug, Default)]
pub(super) struct DevicePatch {
    pub(super) battery_and_controllers: Option<(u8, Option<bool>, HeadsetControllersInfo)>,
    pub(super) is_charging: Option<Option<bool>>,
    pub(super) space_info: Option<SpaceInfo>,
    pub(super) installed_packages: Option<Vec<InstalledPackage>>,
    pub(super) guardian_paused: Option<Option<bool>>,
    pub(super) proximity_disabled: Option<Option<bool>>,
    pub(super) usb_state: Option<(Option<bool>, Option<String>)>,
    /// Applied after `usb_state`, so direct MTP updates take precedence.
    pub(super) storage_connected: Option<Option<bool>>,
}

impl DevicePatch {
    pub(super) fn storage_connected(connected: bool) -> Self {
        Self { storage_connected: Some(Some(connected)), ..Self::default() }
    }
}

/// Results from querying one or more device components.
#[derive(Debug, Default)]
pub(super) struct DeviceQueryOutcome {
    pub(super) patch: DevicePatch,
    pub(super) failures: Vec<(DeviceRefreshComponents, String)>,
}

use crate::{
    adb::PackageName,
    models::{
        InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, parse_list_apps_dex,
        signals::{adb::command::RebootMode, system::Toast},
        vendor::quest_controller::{
            CONTROLLER_INFO_COMMAND_DUMPSYS, CONTROLLER_INFO_COMMAND_JSON, HeadsetControllersInfo,
        },
    },
};

pub(super) fn charging_from_battery_status(status: u8) -> Option<bool> {
    match status {
        2 | 5 => Some(true),
        3 | 4 => Some(false),
        _ => None,
    }
}

fn parse_battery_state(dump: &str) -> Result<(u8, Option<bool>)> {
    let mut level = None;
    let mut is_charging = None;

    for line in dump.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        match key {
            "level" => level = value.trim().parse().ok(),
            "status" => {
                is_charging = value
                    .split_whitespace()
                    .next()
                    .and_then(|status| status.parse().ok())
                    .and_then(charging_from_battery_status)
            }
            _ => {}
        }
    }

    Ok((level.context("Failed to parse device battery level from dumpsys output")?, is_charging))
}

/// Java tool used for package listing
static LIST_APPS_DEX_BYTES: &[u8] = include_bytes!("../../../assets/list_apps.dex");
const LIST_APPS_DEX_SHA256: const_hex::Buffer<32> =
    const_hex::const_encode(&Sha256::new().update(LIST_APPS_DEX_BYTES).finalize());

/// Represents a connected Android device with ADB capabilities
#[derive(Debug, Clone)]
pub(crate) struct AdbDevice {
    #[debug(skip)]
    pub inner: Device,
    /// Human-readable device name
    pub name: Option<String>,
    /// Product identifier from device
    pub product: String,
    /// Unique device serial number (reported by ADB, e.g. `1WMHH000M12345` for USB devices, `192.168.1.100:5555` for wireless devices)
    pub serial: String,
    /// True device serial number (reported by the device, e.g. `1WMHH000M12345`)
    pub true_serial: String,
    /// ADB transport ID
    pub transport_id: String,
    /// True if connected over TCP/IP (adb over network)
    pub is_wireless: bool,
    /// Device battery level (0-100)
    pub battery_level: u8,
    /// Whether the device is charging or full while connected to power
    pub is_charging: Option<bool>,
    /// Information about connected controllers
    pub controllers: HeadsetControllersInfo,
    /// Device storage space information
    pub space_info: SpaceInfo,
    /// List of installed packages on the device
    #[debug("({} items)", installed_packages.len())]
    pub installed_packages: Vec<InstalledPackage>,
    /// Whether the Guardian system is currently paused on the device
    pub guardian_paused: Option<bool>,
    /// Whether the proximity sensor is currently disabled (faked/overridden) on the device
    pub proximity_disabled: Option<bool>,
    /// Whether MTP storage is currently enabled over USB
    pub storage_connected: Option<bool>,
    /// Current USB speed reported by Android
    pub usb_speed: Option<String>,
}

impl Display for AdbDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name.as_ref().unwrap_or(&"Unknown".to_string()), self.serial)
    }
}

impl AdbDevice {
    const WIRELESS_ADB_PORT: u16 = 5555;

    /// Creates a new AdbDevice instance and initializes its state
    ///
    /// # Arguments
    /// * `inner` - The underlying forensic_adb Device instance
    #[instrument(level = "debug", skip(inner), ret, err)]
    pub(super) async fn new(inner: Device) -> Result<Self> {
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
            is_charging: None,
            controllers: HeadsetControllersInfo::default(),
            space_info: SpaceInfo::default(),
            installed_packages: Vec::new(),
            guardian_paused: None,
            proximity_disabled: None,
            storage_connected: None,
            usb_speed: None,
        };

        // Read identity first to use manufacturer + model if available
        match Self::query_identity(&device.inner).await {
            Ok(identity) => device.name = Some(identity),
            Err(e) => warn!(
                error = e.as_ref() as &dyn Error,
                "Failed to refresh device identity, using fallback name"
            ),
        }
        let outcome = device.query_components(DeviceRefreshComponents::ALL).boxed().await;
        if !outcome.failures.is_empty() {
            let errors = outcome
                .failures
                .iter()
                .map(|(_, error)| error.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            warn!(errors, "Errors while refreshing device info");
        }
        device.apply_patch(outcome.patch);
        Ok(device)
    }

    /// Queries manufacturer (`ro.product.manufacturer`) and model (`ro.product.model`) from a `forensic_adb::Device` and returns a combined string.
    ///
    /// If manufacturer is empty, returns just model.
    #[instrument(level = "debug", skip(device), err)]
    pub(super) async fn query_identity(device: &Device) -> Result<String> {
        let manufacturer = tokio::time::timeout(
            Duration::from_millis(800),
            device.shell("getprop ro.product.manufacturer"),
        )
        .await
        .context("Timed out reading ro.product.manufacturer")?
        .context("Failed to read ro.product.manufacturer")?
        .trim()
        .to_string();
        let model = tokio::time::timeout(
            Duration::from_millis(800),
            device.shell("getprop ro.product.model"),
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
    #[instrument(level = "debug", skip(device), err)]
    pub(super) async fn query_true_serial(device: &Device) -> Result<String> {
        Ok(device
            .shell("getprop ro.serialno")
            .await
            .context("Failed to read ro.serialno")?
            .trim()
            .to_string())
    }

    /// Queries selected device information fields in parallel.
    #[instrument(level = "debug", skip(self))]
    pub(super) async fn query_components(
        &self,
        components: DeviceRefreshComponents,
    ) -> DeviceQueryOutcome {
        let components = components.normalized();
        let packages = async {
            if components.contains(DeviceRefreshComponents::PACKAGES) {
                Some(self.query_package_list().await)
            } else {
                None
            }
        };
        let battery_and_controllers = async {
            if components.contains(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS) {
                Some(self.query_battery_info().await)
            } else {
                None
            }
        };
        let storage = async {
            if components.contains(DeviceRefreshComponents::STORAGE) {
                Some(self.query_space_info().await)
            } else {
                None
            }
        };
        let guardian = async {
            if components.contains(DeviceRefreshComponents::GUARDIAN) {
                Some(self.query_guardian_state().await)
            } else {
                None
            }
        };
        let proximity = async {
            if components.contains(DeviceRefreshComponents::PROXIMITY) {
                Some(self.query_proximity_state().await)
            } else {
                None
            }
        };
        let usb = async {
            if components.contains(DeviceRefreshComponents::USB) {
                Some(self.query_usb_state().await)
            } else {
                None
            }
        };

        let (packages, battery_and_controllers, storage, guardian, proximity, usb) =
            tokio::join!(packages, battery_and_controllers, storage, guardian, proximity, usb,);

        let mut outcome = DeviceQueryOutcome::default();
        macro_rules! apply_result {
            ($result:expr, $field:ident, $component:expr, $name:literal) => {
                if let Some(result) = $result {
                    match result {
                        Ok(value) => outcome.patch.$field = Some(value),
                        Err(error) => {
                            outcome.failures.push(($component, format!("{}: {error:#}", $name)))
                        }
                    }
                }
            };
        }
        apply_result!(packages, installed_packages, DeviceRefreshComponents::PACKAGES, "packages");
        apply_result!(
            battery_and_controllers,
            battery_and_controllers,
            DeviceRefreshComponents::BATTERY_AND_CONTROLLERS,
            "battery/controllers"
        );
        apply_result!(storage, space_info, DeviceRefreshComponents::STORAGE, "storage");
        apply_result!(guardian, guardian_paused, DeviceRefreshComponents::GUARDIAN, "guardian");
        apply_result!(
            proximity,
            proximity_disabled,
            DeviceRefreshComponents::PROXIMITY,
            "proximity"
        );
        apply_result!(usb, usb_state, DeviceRefreshComponents::USB, "usb");
        outcome
    }

    /// Applies successful values and returns whether visible device state changed.
    pub(super) fn apply_patch(&mut self, patch: DevicePatch) -> bool {
        let mut changed = false;

        if let Some(packages) = patch.installed_packages
            && self.installed_packages != packages
        {
            self.installed_packages = packages;
            changed = true;
        }
        if let Some((battery_level, is_charging, controllers)) = patch.battery_and_controllers {
            if self.battery_level != battery_level {
                self.battery_level = battery_level;
                changed = true;
            }
            if self.is_charging != is_charging {
                self.is_charging = is_charging;
                changed = true;
            }
            if self.controllers != controllers {
                self.controllers = controllers;
                changed = true;
            }
        }
        if let Some(is_charging) = patch.is_charging
            && self.is_charging != is_charging
        {
            self.is_charging = is_charging;
            changed = true;
        }
        if let Some(space_info) = patch.space_info
            && self.space_info != space_info
        {
            self.space_info = space_info;
            changed = true;
        }
        if let Some(guardian_paused) = patch.guardian_paused
            && self.guardian_paused != guardian_paused
        {
            self.guardian_paused = guardian_paused;
            changed = true;
        }
        if let Some(proximity_disabled) = patch.proximity_disabled
            && self.proximity_disabled != proximity_disabled
        {
            self.proximity_disabled = proximity_disabled;
            changed = true;
        }
        if let Some((storage_connected, usb_speed)) = patch.usb_state {
            if self.storage_connected != storage_connected {
                self.storage_connected = storage_connected;
                changed = true;
            }
            if self.usb_speed != usb_speed {
                self.usb_speed = usb_speed;
                changed = true;
            }
        }
        if let Some(storage_connected) = patch.storage_connected
            && self.storage_connected != storage_connected
        {
            self.storage_connected = storage_connected;
            changed = true;
        }

        changed
    }

    /// Returns humanized `dumpsys battery` output from the device
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn battery_dump(&self) -> Result<String> {
        Ok(battery_dump::humanize_dump(
            &self
                .shell_checked("dumpsys battery")
                .await
                .context("'dumpsys battery' command failed")?,
        ))
    }

    /// Executes a shell command on the device
    #[instrument(level = "debug", skip(self), err, ret)]
    pub(super) async fn shell(&self, command: &str) -> Result<String> {
        self.inner
            .shell(command)
            .await
            .context("Failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "Shell command executed"))
    }

    /// Executes a shell command and fails if exit code is non-zero.
    #[instrument(level = "debug", skip(self), err, ret)]
    pub(super) async fn shell_checked(&self, command: &str) -> Result<String> {
        let shell_output = self
            .inner
            .shell_v2(command)
            .await
            .context(format!("Failed to execute checked shell command: {command}"))?;
        let stdout = std::str::from_utf8(&shell_output.stdout)
            .context("Shell command stdout is not valid UTF-8")?
            .replace("\r\n", "\n");
        let stderr = std::str::from_utf8(&shell_output.stderr)
            .context("Shell command stderr is not valid UTF-8")?
            .replace("\r\n", "\n");
        if shell_output.exit_code != 0 {
            error!(
                exit_code = shell_output.exit_code,
                stdout, stderr, "Shell command returned non-zero exit code"
            );
            bail!(
                "Command {command} failed with exit code {}. stdout: {stdout}; stderr: {stderr}",
                shell_output.exit_code
            );
        }
        if !stderr.is_empty() {
            trace!(stderr, "Shell command wrote to stderr");
        }
        Ok(stdout)
    }

    /// Reboots the device with the given mode
    ///
    /// # Arguments
    /// * `mode` - The mode to reboot the device in (normal, bootloader, recovery, fastboot, power off)
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn reboot_with_mode(&self, mode: RebootMode) -> Result<()> {
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
    /// * `enabled` - Whether to enable the real proximity sensor (true) or fake it as close (false)
    /// * `duration_ms` - Optional duration in milliseconds for disabling (only used when enabled=false)
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn set_proximity_sensor(
        &self,
        enabled: bool,
        duration_ms: Option<u64>,
    ) -> Result<()> {
        let cmd = if enabled {
            // Enable real sensor by disabling automation
            "am broadcast -a com.oculus.vrpowermanager.automation_disable".to_string()
        } else {
            // Disable sensor (fake as close)
            match duration_ms {
                Some(ms) => format!(
                    "am broadcast -a com.oculus.vrpowermanager.prox_close --ei duration {}",
                    ms
                ),
                None => "am broadcast -a com.oculus.vrpowermanager.prox_close".to_string(),
            }
        };
        self.shell_checked(&cmd).await.context(format!(
            "Failed to set proximity sensor: enabled={}, duration_ms={:?}",
            enabled, duration_ms
        ))?;
        Ok(())
    }

    /// Sets the guardian paused state
    ///
    /// # Arguments
    /// * `paused` - Whether to pause or resume the guardian
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn set_guardian_paused(&self, paused: bool) -> Result<()> {
        let value = if paused { 1 } else { 0 };
        self.shell_checked(&format!("setprop debug.oculus.guardian_pause {value}"))
            .await
            .context(format!("Failed to set guardian paused: {paused}"))?;
        Ok(())
    }

    /// Sets USB storage connection state.
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn set_storage_connection(&self, connected: bool) -> Result<()> {
        let command = if connected { "svc usb setFunctions mtp" } else { "svc usb setFunctions" };
        self.shell(command).await.context("Failed to set USB storage connection")?;
        Ok(())
    }

    /// Queries the guardian paused state from the device
    #[instrument(level = "debug", skip(self), err)]
    async fn query_guardian_state(&self) -> Result<Option<bool>> {
        let output = self.shell("getprop debug.oculus.guardian_pause").await?;
        let trimmed = output.trim();
        // Property value is "1" for paused, "0" or empty for not paused
        Ok(Some(trimmed == "1"))
    }

    /// Queries the proximity sensor disabled state from the device.
    /// Parses `dumpsys oculus.internal.power.IVrPowerManager/default` output.
    /// - `Virtual proximity state: CLOSE` => proximity disabled (faked)
    /// - `Virtual proximity state: DISABLED` => proximity enabled (real sensor)
    #[instrument(level = "debug", skip(self), err)]
    async fn query_proximity_state(&self) -> Result<Option<bool>> {
        let output = self.shell("dumpsys oculus.internal.power.IVrPowerManager/default").await?;

        if let Some(captures) = regex!(r"^Virtual proximity state: (\w+)").captures(&output)
            && let Some(state) = captures.get(1)
        {
            // CLOSE means proximity is faked (sensor disabled)
            // DISABLED means proximity override is off (sensor enabled/real)
            return Ok(match state.as_str() {
                "CLOSE" => Some(true),
                "DISABLED" => Some(false),
                _ => None,
            });
        }
        trace!(output, "No virtual proximity state found");

        Ok(None)
    }

    /// Queries current USB functions and speed.
    #[instrument(level = "debug", skip(self), err)]
    async fn query_usb_state(&self) -> Result<(Option<bool>, Option<String>)> {
        let functions = self
            .shell_checked("svc usb getFunctions")
            .await
            .context("Failed to query USB functions")?;
        let storage_connected = Some(functions.split(',').any(|function| function.trim() == "mtp"));
        let speed = if !self.is_wireless {
            let output = self
                .shell_checked("svc usb getUsbSpeed")
                .await
                .context("Failed to query USB speed")?;
            format_usb_speed(&output)
        } else {
            None
        };

        Ok((storage_connected, speed))
    }

    /// Queries the list of installed packages on the device
    #[instrument(level = "debug", skip(self), fields(count), err)]
    async fn query_package_list(&self) -> Result<Vec<InstalledPackage>> {
        const LIST_APPS_DEX_PATH: &str = "/data/local/tmp/list_apps.dex";
        if !self
            .shell_checked(concatcp!("sha256sum ", LIST_APPS_DEX_PATH))
            .await
            .map(|output| output.contains(LIST_APPS_DEX_SHA256.as_str()))
            .unwrap_or_default()
        {
            debug!("Pushing list_apps.dex");
            self.push_bytes(LIST_APPS_DEX_BYTES, UnixPath::new(LIST_APPS_DEX_PATH))
                .await
                .context("Failed to push list_apps.dex")?;
        }

        let list_output = self
            .shell_checked(concatcp!("CLASSPATH=", LIST_APPS_DEX_PATH, " app_process / Main"))
            .await
            .context("Failed to execute app_process for list_apps.dex")?;

        let packages =
            parse_list_apps_dex(&list_output).context("Failed to parse list_apps.dex output")?;

        Span::current().record("count", packages.len());
        Ok(packages)
    }

    /// Queries battery information for the device and controllers
    #[instrument(level = "debug", skip(self), err)]
    async fn query_battery_info(&self) -> Result<(u8, Option<bool>, HeadsetControllersInfo)> {
        let (battery_dump, controllers) =
            tokio::join!(self.battery_dump(), self.query_controllers());
        let battery_dump = battery_dump.context("Failed to get battery dump")?;

        let (device_level, is_charging) = parse_battery_state(&battery_dump)?;
        trace!(level = device_level, "Parsed device battery level");

        Ok((device_level, is_charging, controllers?))
    }

    /// Queries controller battery levels using rstest, with a dumpsys fallback.
    #[instrument(level = "debug", skip(self), err)]
    async fn query_controllers(&self) -> Result<HeadsetControllersInfo> {
        let controllers = match self.shell_checked(CONTROLLER_INFO_COMMAND_JSON).await {
            Ok(json) => match HeadsetControllersInfo::from_rstest_json(&json) {
                Ok(info) => info,
                Err(e) => {
                    warn!(
                        error = e.as_ref() as &dyn Error,
                        "Failed to parse rstest json, falling back to dumpsys"
                    );
                    let dump = self
                        .shell(CONTROLLER_INFO_COMMAND_DUMPSYS)
                        .await
                        .context("Failed to get controller info via dumpsys")?;
                    HeadsetControllersInfo::from_dumpsys(&dump)
                }
            },
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    "rstest command failed, falling back to dumpsys"
                );
                let dump = self
                    .shell(CONTROLLER_INFO_COMMAND_DUMPSYS)
                    .await
                    .context("Failed to get controller info via dumpsys")?;
                HeadsetControllersInfo::from_dumpsys(&dump)
            }
        };
        trace!(?controllers, "Parsed controller info");
        Ok(controllers)
    }

    /// Queries storage space information
    #[instrument(level = "debug", skip(self), err)]
    async fn query_space_info(&self) -> Result<SpaceInfo> {
        self.get_space_info().await
    }

    /// Gets storage space information from the device
    #[instrument(level = "debug", skip(self), err)]
    async fn get_space_info(&self) -> Result<SpaceInfo> {
        let output =
            self.shell_checked(SPACE_INFO_COMMAND).await.context("Space info command failed")?;
        SpaceInfo::from_stat_output(&output)
    }

    /// Launches an application on the device
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn launch(&self, package: &PackageName) -> Result<()> {
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
            warn!(output, %package, "Monkey command returned error");
            return Err(anyhow!("Failed to launch package '{package}'"));
        }

        info!("Launched with default category");
        Ok(())
    }

    /// Force stops an application on the device
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn force_stop(&self, package: &PackageName) -> Result<()> {
        self.inner
            .force_stop(package.as_str())
            .await
            .with_context(|| format!("Failed to force stop {package}"))
    }

    /// Uninstalls a package from the device
    #[instrument(level = "debug", skip(self))]
    pub(super) async fn uninstall_package(&self, package: &PackageName) -> Result<()> {
        match self.inner.uninstall_package(package.as_str()).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_str = e.to_string();
                debug!(error = %error_str, "Uninstall failed, checking error type");

                if error_str.contains("DELETE_FAILED_INTERNAL_ERROR") {
                    // Check if package exists
                    let escaped = package.as_str().replace('.', "\\.");
                    let output = self
                        .shell(&format!("pm list packages | grep -w ^package:{escaped}$"))
                        .await
                        .unwrap_or_default();

                    if output.trim().is_empty() {
                        Err(anyhow!("Package not installed: {package}"))
                    } else {
                        Err(e.into())
                    }
                } else if error_str.contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") {
                    info!(
                        "Package {} is protected by device policy, trying to force uninstall",
                        package.as_str()
                    );
                    self.shell(&format!("pm disable-user {package}")).await?;
                    self.inner
                        .uninstall_package(package.as_str())
                        .await
                        .map_err(Into::<anyhow::Error>::into)
                } else {
                    Err(e.into())
                }
            }
        }
        .context("Failed to uninstall package")
    }

    /// Gets APK path reported by `pm path <package>`
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn get_apk_path(&self, package: &PackageName) -> Result<String> {
        let output = self
            .shell_checked(&format!("pm path {package}"))
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
        bail!("Failed to parse APK path for package '{package}': {output}");
    }

    /// Pulls an application's APK and OBB (if present) into a local directory suitable for donation.
    ///
    /// Layout:
    /// - `<dest_root>/<package_name>/<package_name>.apk`
    /// - `<dest_root>/<package_name>/` + OBB contents (when present)
    #[instrument(level = "debug", skip(self, dest_root), err)]
    pub(super) async fn pull_app_for_donation(
        &self,
        package: &PackageName,
        dest_root: &Path,
    ) -> Result<PathBuf> {
        let package_str = package.as_str();

        if !dest_root.exists() {
            fs::create_dir_all(dest_root).await.with_context(|| {
                format!("Failed to create destination root {}", dest_root.display())
            })?;
        }
        anyhow::ensure!(
            dest_root.is_dir(),
            "Destination root is not a directory: {}",
            dest_root.display()
        );

        let app_dir = dest_root.join(package_str);
        if app_dir.exists() {
            debug!(path = %app_dir.display(), "Removing existing app donation directory");
            fs::remove_dir_all(&app_dir).await.with_context(|| {
                format!("Failed to remove existing directory {}", app_dir.display())
            })?;
        }
        fs::create_dir_all(&app_dir).await.with_context(|| {
            format!("Failed to create app donation directory {}", app_dir.display())
        })?;

        let apk_remote = self.get_apk_path(package).await?;
        let apk_remote_path = UnixPath::new(&apk_remote);
        let local_apk_path = self.pull(apk_remote_path, &app_dir).await?;

        let renamed_apk_path = app_dir.join(format!("{package_str}.apk"));
        if local_apk_path != renamed_apk_path {
            fs::rename(&local_apk_path, &renamed_apk_path).await.with_context(|| {
                format!(
                    "Failed to rename pulled APK from {} to {}",
                    local_apk_path.display(),
                    renamed_apk_path.display()
                )
            })?;
        }

        // Pull OBB directory if present
        let obb_remote_dir = UnixPath::new("/sdcard/Android/obb").join(package_str);
        if self.dir_exists(&obb_remote_dir).await? {
            debug!(package = package_str, "Pulling OBB directory for donation");
            self.pull_dir(&obb_remote_dir, &app_dir).await?;
        } else {
            debug!(package = package_str, "No OBB directory found for package, skipping");
        }

        Ok(app_dir)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn clean_temp_apks(&self) -> Result<()> {
        debug!("Cleaning up temporary APKs");
        self.shell("rm -rf /data/local/tmp/*.apk").await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    async fn ip_from_route(&self) -> Result<Option<Ipv4Addr>> {
        let output = self
            .shell_checked("ip route | grep wlan0")
            .await
            .context("'ip route' command failed")?;

        let caps = match regex!(r"src ((?:\d{1,3}\.){3}\d{1,3})").captures(&output) {
            Some(caps) => caps,
            None => return Ok(None),
        };

        let ip = caps[1].parse().context("Regex matched but IPv4 parsing failed")?;

        Ok(Some(ip))
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    async fn enable_tcpip(&self, ip: Ipv4Addr) -> Result<SocketAddrV4> {
        self.inner.tcpip(Self::WIRELESS_ADB_PORT).await.context("Failed to enable tcpip mode")?;

        Ok(SocketAddrV4::new(ip, Self::WIRELESS_ADB_PORT))
    }

    #[instrument(level = "debug", skip(self), ret, err)]
    pub(super) async fn enable_wireless_adb(&self) -> Result<SocketAddrV4> {
        if let Some(ip) = self.ip_from_route().await? {
            return self.enable_tcpip(ip).await;
        }

        Toast::send("Wireless ADB".to_string(), "Turning on Wi-Fi...".to_string(), false, None);

        self.shell_checked("svc wifi enable").await.context("'svc wifi enable' command failed")?;

        const TOTAL_WAIT: Duration = Duration::from_secs(20);
        const STEP: Duration = Duration::from_millis(500);
        let started = Instant::now();
        loop {
            if let Some(ip) = self.ip_from_route().await? {
                return self.enable_tcpip(ip).await;
            }

            if started.elapsed() >= TOTAL_WAIT {
                bail!(
                    "Failed to enable Wireless ADB: no IP address after enabling Wi‑Fi and \
                     polling for {}s",
                    TOTAL_WAIT.as_secs()
                );
            }

            sleep(STEP).await;
        }
    }
}

pub(crate) fn format_usb_speed(output: &str) -> Option<String> {
    let value = output
        .trim()
        .lines()
        // Some weird states give error lines like "libc: access denied finding property ..."
        .filter(|line| !line.starts_with("libc: "))
        .collect::<Vec<&str>>()
        .join("\n");
    if value.is_empty() {
        return None;
    }

    if let Ok(mbps) = value.parse::<u64>() {
        if mbps >= 1024 {
            let gbps = mbps as f64 / 1024.0;
            let formatted = if gbps.fract() == 0.0 {
                format!("{gbps:.0}")
            } else {
                format!("{gbps:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
            };
            return Some(format!("{formatted} Gbps"));
        }
        return Some(format!("{mbps} Mbps"));
    }
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use forensic_adb::{Device, Host};

    use super::{
        AdbDevice, DevicePatch, DeviceRefreshComponents, format_usb_speed, parse_battery_state,
    };
    use crate::models::{InstalledPackage, SpaceInfo};

    async fn test_device() -> AdbDevice {
        AdbDevice {
            inner: Device::new(Host::default(), "serial".to_string(), BTreeMap::new())
                .await
                .unwrap(),
            name: Some("Quest".to_string()),
            product: "hollywood".to_string(),
            serial: "serial".to_string(),
            true_serial: "serial".to_string(),
            transport_id: "1".to_string(),
            is_wireless: false,
            battery_level: 50,
            is_charging: Some(false),
            controllers: Default::default(),
            space_info: SpaceInfo { total: 100, available: 50 },
            installed_packages: Vec::new(),
            guardian_paused: Some(false),
            proximity_disabled: Some(false),
            storage_connected: Some(false),
            usb_speed: Some("5 Gbps".to_string()),
        }
    }

    #[test]
    fn package_refresh_also_refreshes_storage() {
        let components = DeviceRefreshComponents::PACKAGES.normalized();
        assert!(components.contains(DeviceRefreshComponents::PACKAGES));
        assert!(components.contains(DeviceRefreshComponents::STORAGE));
        assert_eq!(format!("{components:?}"), "DeviceRefreshComponents(PACKAGES | STORAGE)");
    }

    #[tokio::test]
    async fn applies_battery_and_controllers_together() {
        let mut device = test_device().await;
        let controllers =
            crate::models::vendor::quest_controller::HeadsetControllersInfo::default();
        let changed = device.apply_patch(DevicePatch {
            battery_and_controllers: Some((75, Some(true), controllers)),
            ..DevicePatch::default()
        });

        assert!(changed);
        assert_eq!(device.battery_level, 75);
        assert_eq!(device.is_charging, Some(true));
    }

    #[tokio::test]
    async fn applies_charging_only_when_it_changes() {
        let mut device = test_device().await;
        let original_level = device.battery_level;
        let original_controllers = device.controllers.clone();

        assert!(
            device.apply_patch(DevicePatch {
                is_charging: Some(Some(true)),
                ..DevicePatch::default()
            })
        );
        assert_eq!(device.battery_level, original_level);
        assert_eq!(device.controllers, original_controllers);
        assert!(
            !device.apply_patch(DevicePatch {
                is_charging: Some(Some(true)),
                ..DevicePatch::default()
            })
        );
    }

    #[test]
    fn parses_battery_level_and_charging_state() {
        assert_eq!(parse_battery_state("level: 85\nstatus: 2\n").unwrap(), (85, Some(true)));
        assert_eq!(parse_battery_state("status: 5\nlevel: 100\n").unwrap(), (100, Some(true)));
        assert_eq!(
            parse_battery_state("level: 85\nstatus: 2 (Charging)\n").unwrap(),
            (85, Some(true))
        );
        assert_eq!(parse_battery_state("level: 42\nstatus: 3\n").unwrap(), (42, Some(false)));
        assert_eq!(parse_battery_state("level: 42\n").unwrap(), (42, None));
        assert_eq!(parse_battery_state("level: 42\nstatus: 1\n").unwrap(), (42, None));
        assert!(parse_battery_state("status: 2\n").is_err());
    }

    #[tokio::test]
    async fn patch_changes_only_supplied_fields_and_detects_noop() {
        let mut device = test_device().await;
        let original_speed = device.usb_speed.clone();
        let changed = device.apply_patch(DevicePatch {
            installed_packages: Some(vec![InstalledPackage::default()]),
            storage_connected: Some(Some(true)),
            ..DevicePatch::default()
        });

        assert!(changed);
        assert_eq!(device.installed_packages.len(), 1);
        assert_eq!(device.storage_connected, Some(true));
        assert_eq!(device.usb_speed, original_speed);
        assert!(!device.apply_patch(DevicePatch::storage_connected(true)));
    }

    #[test]
    fn formats_numeric_usb_speed() {
        assert_eq!(format_usb_speed("480\n").as_deref(), Some("480 Mbps"));
    }

    #[test]
    fn formats_gigabit_usb_speed() {
        assert_eq!(format_usb_speed("5120\n").as_deref(), Some("5 Gbps"));
    }

    #[test]
    fn keeps_text_usb_speed() {
        assert_eq!(format_usb_speed("high speed\n").as_deref(), Some("high speed"));
    }

    #[test]
    fn ignores_empty_usb_values() {
        assert_eq!(format_usb_speed(" \n"), None);
    }
}
