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
use const_format::concatcp;
use derive_more::Debug;
use forensic_adb::{Device, UnixPath};
use lazy_regex::{Lazy, Regex, lazy_regex};
use sha2_const_stable::Sha256;
pub(crate) use sideload::SideloadProgress;
use tokio::{fs, time::sleep};
use tracing::{Span, debug, error, info, instrument, trace, warn};

use crate::{
    adb::{PackageName, battery_dump},
    models::{
        InstalledPackage, SPACE_INFO_COMMAND, SpaceInfo, parse_list_apps_dex,
        signals::{adb::command::RebootMode, system::Toast},
        vendor::quest_controller::{
            CONTROLLER_INFO_COMMAND_DUMPSYS, CONTROLLER_INFO_COMMAND_JSON, HeadsetControllersInfo,
        },
    },
};

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
            controllers: HeadsetControllersInfo::default(),
            space_info: SpaceInfo::default(),
            installed_packages: Vec::new(),
            guardian_paused: None,
            proximity_disabled: None,
        };

        // Refresh identity first to use manufacturer + model if available
        if let Err(e) = device.refresh_identity().await {
            warn!(
                error = e.as_ref() as &dyn Error,
                "Failed to refresh device identity, using fallback name"
            );
        }
        device.refresh().await.context("Failed to refresh device info")?;
        Ok(device)
    }

    /// Refresh basic identity (name) using `ro.product.manufacturer` and `ro.product.model`
    #[instrument(level = "debug", skip(self), err)]
    async fn refresh_identity(&mut self) -> Result<()> {
        let identity = Self::query_identity(&self.inner).await?;
        self.name = Some(identity);
        Ok(())
    }

    /// Queries manufacturer + model from a `forensic_adb::Device` and returns a combined string.
    /// If manufacturer is empty, returns just model.
    #[instrument(level = "debug", skip(device), err)]
    pub(super) async fn query_identity(device: &Device) -> Result<String> {
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
    #[instrument(level = "debug", skip(device), err)]
    pub(super) async fn query_true_serial(device: &Device) -> Result<String> {
        Ok(device
            .execute_host_shell_command("getprop ro.serialno")
            .await
            .context("Failed to read ro.serialno")?
            .trim()
            .to_string())
    }

    /// Refreshes device information (packages, battery, space, guardian)
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn refresh(&mut self) -> Result<()> {
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
        if let Err(e) = self.refresh_guardian_state().await {
            errors.push(("guardian", e));
            self.guardian_paused = None;
        }
        if let Err(e) = self.refresh_proximity_state().await {
            errors.push(("proximity", e));
            self.proximity_disabled = None;
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
            .execute_host_shell_command(command)
            .await
            .context("Failed to execute shell command")
            .inspect(|v| trace!(output = ?v, "Shell command executed"))
    }

    /// Executes a shell command and fails if exit code is non-zero.
    /// Appends `; printf '\n%s' $?` and parses the final line as the exit status.
    #[instrument(level = "debug", skip(self), err, ret)]
    pub(super) async fn shell_checked(&self, command: &str) -> Result<String> {
        let shell_output = self
            .shell(&format!("{} ; printf '\\n%s' $?", command))
            .await
            .context(format!("Failed to execute checked shell command: {command}"))?;
        let (output, exit_code) = match shell_output.rsplit_once('\n') {
            Some(parts) => parts,
            None => {
                let trimmed = shell_output.trim();
                if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
                    ("", trimmed)
                } else {
                    return Err(anyhow!("Failed to extract exit code"));
                }
            }
        };
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

    /// Refreshes the guardian paused state from the device
    #[instrument(level = "debug", skip(self), err)]
    async fn refresh_guardian_state(&mut self) -> Result<()> {
        let output = self.shell("getprop debug.oculus.guardian_pause").await?;
        let trimmed = output.trim();
        // Property value is "1" for paused, "0" or empty for not paused
        self.guardian_paused = Some(trimmed == "1");
        Ok(())
    }

    /// Refreshes the proximity sensor disabled state from the device.
    /// Parses `dumpsys oculus.internal.power.IVrPowerManager/default` output.
    /// - `Virtual proximity state: CLOSE` => proximity disabled (faked)
    /// - `Virtual proximity state: DISABLED` => proximity enabled (real sensor)
    #[instrument(level = "debug", skip(self), err)]
    async fn refresh_proximity_state(&mut self) -> Result<()> {
        static VIRTUAL_PROXIMITY_STATE_REGEX: Lazy<Regex> =
            lazy_regex!(r"^Virtual proximity state: (\w+)");

        let output = self.shell("dumpsys oculus.internal.power.IVrPowerManager/default").await?;

        if let Some(captures) = VIRTUAL_PROXIMITY_STATE_REGEX.captures(&output)
            && let Some(state) = captures.get(1)
        {
            // CLOSE means proximity is faked (sensor disabled)
            // DISABLED means proximity override is off (sensor enabled/real)
            match state.as_str() {
                "CLOSE" => self.proximity_disabled = Some(true),
                "DISABLED" => self.proximity_disabled = Some(false),
                _ => self.proximity_disabled = None,
            }
            return Ok(());
        }

        self.proximity_disabled = None;
        Ok(())
    }

    /// Refreshes the list of installed packages on the device
    #[instrument(level = "debug", skip(self), fields(count), err)]
    async fn refresh_package_list(&mut self) -> Result<()> {
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
        self.installed_packages = packages;
        Ok(())
    }

    /// Refreshes battery information for the device and controllers
    #[instrument(level = "debug", skip(self), err)]
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

        self.battery_level = device_level;
        self.controllers = controllers;
        Ok(())
    }

    /// Refreshes storage space information
    #[instrument(level = "debug", skip(self), err)]
    async fn refresh_space_info(&mut self) -> Result<()> {
        let space_info = self.get_space_info().await?;
        self.space_info = space_info;
        Ok(())
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
        /// Regex to extract IP address from `ip route` output
        static IP_ROUTE_REGEX: Lazy<Regex> = lazy_regex!(r"src ((?:\d{1,3}\.){3}\d{1,3})");

        let output = self
            .shell_checked("ip route | grep wlan0")
            .await
            .context("'ip route' command failed")?;

        let caps = match IP_ROUTE_REGEX.captures(&output) {
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
