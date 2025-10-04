use std::{error::Error, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use derive_more::Debug;
use device::AdbDevice;
use forensic_adb::{DeviceBrief, DeviceState};
use lazy_regex::{Lazy, Regex, lazy_regex};
use rinf::{DartSignal, RustSignal};
use tokio::{
    process::Command,
    sync::{Mutex, RwLock, mpsc::UnboundedSender},
    time::{self, timeout},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, info_span, instrument, trace, warn};

use crate::{
    adb::device::{BackupOptions, SideloadProgress},
    models::{
        Settings,
        signals::{
            adb::{
                command::*, device::DeviceChangedEvent, dump::BatteryDumpResponse, state::AdbState,
            },
            system::Toast,
        },
    },
    utils::resolve_binary_path,
};

pub mod battery;
pub mod device;

pub static PACKAGE_NAME_REGEX: Lazy<Regex> = lazy_regex!(r"^(?:[A-Za-z]{1}[\w]*\.)+[A-Za-z][\w]*$");

/// Validates a package name and returns an error if invalid
pub fn ensure_valid_package(package_name: &str) -> Result<()> {
    ensure!(PACKAGE_NAME_REGEX.is_match(package_name), "Invalid package name: '{}'", package_name);
    Ok(())
}

/// Handles ADB device connections and commands
#[derive(Debug)]
pub struct AdbHandler {
    /// The ADB host instance for device communication
    adb_host: forensic_adb::Host,
    /// ADB server check/start mutex
    adb_server_mutex: Mutex<()>,
    /// ADB binary path
    adb_path: RwLock<Option<String>>,
    /// ADB handler state
    adb_state: RwLock<AdbState>,
    /// Currently connected device (if any)
    device: RwLock<Option<Arc<AdbDevice>>>,
    /// Cancellation token for running tasks
    cancel_token: RwLock<CancellationToken>,
}

impl AdbHandler {
    /// Creates a new AdbHandler instance and starts device monitoring.
    /// This is the main entry point for ADB functionality.
    ///
    /// # Returns
    /// Arc-wrapped AdbHandler that manages ADB device connections
    #[instrument(skip(settings_stream))]
    pub async fn new(mut settings_stream: WatchStream<Settings>) -> Arc<Self> {
        let adb_path =
            settings_stream.next().await.expect("Settings stream closed on adb init").adb_path;
        let adb_path = if adb_path.is_empty() { None } else { Some(adb_path) };
        let handle = Arc::new(Self {
            adb_host: if cfg!(target_os = "windows") {
                // This is some retarded stuff, but it fails to connect on a Windows host without this
                // However, passing this host to `adb start-server` fails too (so we're not using `adb_host.start_server()`)
                forensic_adb::Host { host: Some("127.0.0.1".to_string()), port: Some(5037) }
            } else {
                forensic_adb::Host::default()
            },
            adb_server_mutex: Mutex::new(()),
            adb_path: RwLock::new(adb_path),
            adb_state: RwLock::new(AdbState::default()),
            device: None.into(),
            cancel_token: RwLock::new(CancellationToken::new()),
        });
        tokio::spawn(
            {
                let handle = handle.clone();
                async move {
                    if let Err(e) = handle.ensure_server_running().await {
                        error!(
                            error = e.as_ref() as &dyn Error,
                            "Failed to start ADB server on init"
                        );
                        // State will be set inside ensure_server_running on failure
                    } else {
                        handle.refresh_adb_state().await;
                    }
                }
            }
            .instrument(info_span!("task_init_adb_server")),
        );
        tokio::spawn(handle.clone().start_tasks(settings_stream));
        handle
    }

    /// Starts all background tasks needed for ADB functionality.
    /// This includes device monitoring, command handling, and periodic refreshes.
    ///
    /// # Arguments
    /// * `settings_stream` - WatchStream for application settings updates
    #[instrument(skip(self, settings_stream))]
    async fn start_tasks(self: Arc<AdbHandler>, mut settings_stream: WatchStream<Settings>) {
        // Handle settings updates
        tokio::spawn(
            {
                let handle = self.clone();
                async move {
                    info!("Starting to listen for settings changes");
                    while let Some(settings) = settings_stream.next().await {
                        info!("AdbHandler received settings update");
                        debug!(?settings, "New settings");
                        let new_adb_path = settings.adb_path.clone();
                        let new_adb_path =
                            if new_adb_path.is_empty() { None } else { Some(new_adb_path) };
                        if new_adb_path != *handle.adb_path.read().await {
                            info!(?new_adb_path, "ADB path changed, restarting ADB");
                            *handle.adb_path.write().await = new_adb_path;
                            if let Err(e) = handle.clone().restart_adb().await {
                                error!(error = e.as_ref() as &dyn Error, "Failed to restart ADB");
                                // TODO: report this to the UI
                            }
                        }
                    }

                    panic!("Settings stream closed for AdbHandler");
                }
            }
            .instrument(info_span!("task_handle_settings_updates")),
        );

        self.start_adb_tasks().await;
    }

    /// Starts the ADB tasks
    #[instrument(skip(self))]
    async fn start_adb_tasks(self: Arc<AdbHandler>) {
        let cancel_token = self.cancel_token.read().await.clone();
        info!("Starting ADB tasks");

        // Listen for ADB device updates
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn({
            let cancel_token = cancel_token.clone();
            let handler = self.clone();
            async move {
                let result =
                    cancel_token.run_until_cancelled(handler.handle_device_updates(receiver)).await;
                debug!(result = ?result, "Device update handler task finished");
                result
            }
        });

        // Track ADB device changes
        tokio::spawn({
            let cancel_token = cancel_token.clone();
            let handler = self.clone();
            async move {
                let result =
                    cancel_token.run_until_cancelled(handler.run_device_tracker(sender)).await;
                debug!(result = ?result, "Device tracker task finished");
                result
            }
        });

        // Listen for commands
        tokio::spawn({
            let handle = self.clone();
            async move {
                let result = cancel_token.run_until_cancelled(handle.receive_commands()).await;
                debug!(result = ?result, "Command receiver task finished");
                result
            }
        });

        // Refresh device info periodically
        tokio::spawn({
            let handle = self.clone();
            let cancel_token = self.cancel_token.read().await.clone();
            async move {
                let result = cancel_token.run_until_cancelled(handle.run_periodic_refresh()).await;
                debug!(result = ?result, "Periodic refresh task finished");
                result
            }
        });
    }

    /// Restarts the ADB handling
    // TODO: make sure this cannot race with `ensure_server_running`
    #[instrument(skip(self), err)]
    async fn restart_adb(self: Arc<AdbHandler>) -> Result<()> {
        info!("Restarting ADB server and tasks");
        // Cancel all tasks
        self.cancel_token.read().await.cancel();
        // Disconnect from device
        let _ = self.disconnect_device().await;
        // Kill ADB server
        let _ = self.kill_adb_server().await;
        // Restart ADB server
        self.ensure_server_running().await?;
        // Restart tasks
        *self.cancel_token.write().await = CancellationToken::new();
        tokio::spawn(self.clone().start_adb_tasks().instrument(Span::current()));
        info!("ADB server and tasks restarted");
        Ok(())
    }

    /// Kills the ADB server
    #[instrument(skip(self), err)]
    async fn kill_adb_server(&self) -> Result<()> {
        info!("Killing ADB server");
        let adb_path = self.adb_path.read().await.clone();
        if let Err(e) = self.adb_host.kill_server(adb_path.as_deref()).await {
            warn!(error = &e as &dyn Error, "Failed to kill ADB server");
        }
        self.refresh_adb_state().await;
        Ok(())
    }

    /// Runs the device tracking loop that monitors for device connections and disconnections
    ///
    /// # Arguments
    /// * `sender` - Channel sender to communicate device updates
    #[instrument(skip(self, sender), err)]
    async fn run_device_tracker(
        self: Arc<AdbHandler>,
        sender: tokio::sync::mpsc::UnboundedSender<DeviceBrief>,
    ) -> Result<()> {
        loop {
            debug!("Starting track_devices loop");
            self.ensure_server_running().await?;
            let stream = self.adb_host.track_devices();
            tokio::pin!(stream);
            let mut got_update = false;

            while let Some(device_result) = stream.next().await {
                match device_result {
                    Ok(device) => {
                        got_update = true;
                        if sender.send(device).is_err() {
                            bail!("Device update receiver dropped");
                        }
                    }
                    Err(e) => {
                        if got_update {
                            // The stream worked, but encountered an error
                            warn!(
                                error = &e as &dyn Error,
                                "track_devices stream returned an unexpected error, restarting"
                            );
                            // Server might have died
                            self.refresh_adb_state().await;
                            // FIXME: device updates stop after this
                            break;
                        } else {
                            // The stream closed immediately (persistent error likely)
                            return Err(e).context("Failed to start track_devices stream");
                        }
                    }
                }
            }

            time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Handles device state updates received from the device tracker
    ///
    /// # Arguments
    #[instrument(skip(self, receiver), err)]
    async fn handle_device_updates(
        self: Arc<AdbHandler>,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<DeviceBrief>,
    ) -> Result<()> {
        while let Some(device_update) = receiver.recv().await {
            debug!(update = ?device_update, "Received device update");

            match (self.try_current_device().await, &device_update.state) {
                (Some(device), DeviceState::Offline) if device.serial == device_update.serial => {
                    info!(serial = %device.serial, "Current device went offline, disconnecting");
                    if let Err(e) = self.disconnect_device().await {
                        error!(error = e.as_ref() as &dyn Error, "Auto-disconnect failed");
                    }
                }
                (None, DeviceState::Device) => {
                    info!(serial = %device_update.serial, "New device available, auto-connecting");
                    if let Err(e) = self.connect_device().await {
                        error!(error = e.as_ref() as &dyn Error, "Auto-connect failed");
                        Toast::send(
                            "Failed to connect to device".to_string(),
                            format!("{e:#}"),
                            true,
                            None,
                        );
                    }
                }
                // TODO: handle other state combinations
                _ => {
                    trace!("Device update does not require action");
                }
            }

            self.refresh_adb_state().await;
        }

        bail!("Device update channel closed unexpectedly");
    }

    /// Listens for and processes ADB commands received from Dart
    #[instrument(skip(self))]
    async fn receive_commands(&self) {
        let receiver = AdbRequest::get_dart_signal_receiver();
        info!("Listening for ADB commands");
        while let Some(request) = receiver.recv().await {
            info!(command = ?request.message.command, key = %request.message.command_key, "Received ADB command");
            if let Err(e) =
                self.execute_command(request.message.command_key, request.message.command).await
            {
                error!(error = e.as_ref() as &dyn Error, "ADB command execution failed");
            }
        }
        error!("ADB command receiver channel closed");
    }

    /// Executes a received ADB command with the given parameters
    #[instrument(skip(self))]
    async fn execute_command(&self, key: String, command: AdbCommand) -> Result<()> {
        fn send_toast(title: String, description: String, error: bool, duration: Option<Duration>) {
            Toast::send(title, description, error, duration);
        }

        let device = self.current_device().await?;

        let result = match command.clone() {
            AdbCommand::LaunchApp(package_name) => {
                ensure_valid_package(&package_name)?;
                let result = device.launch(&package_name).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandType::LaunchApp,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Failed to launch {package_name}: {e:#}");
                        send_toast("Launch Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to launch {package_name}")))
                    }
                }
            }
            AdbCommand::StartCasting => {
                #[cfg(not(target_os = "windows"))]
                {
                    send_toast(
                        "Casting is Windows-only".to_string(),
                        "The Meta Quest Casting tool is available only on Windows.".to_string(),
                        true,
                        None,
                    );
                    AdbCommandCompletedEvent {
                        command_type: AdbCommandType::StartCasting,
                        command_key: key.clone(),
                        success: false,
                    }
                    .send_signal_to_dart();
                    Ok(())
                }
                #[cfg(target_os = "windows")]
                {
                    use std::path::PathBuf;

                    use tokio::process::Command as TokioCommand;
                    let device = self.current_device().await?;
                    if device.is_wireless {
                        // TODO: Support wireless devices for casting
                        send_toast(
                            "Casting not supported for wireless".to_string(),
                            "Please connect the headset via USB to start casting.".to_string(),
                            true,
                            None,
                        );
                        AdbCommandCompletedEvent {
                            command_type: AdbCommandType::StartCasting,
                            command_key: key.clone(),
                            success: false,
                        }
                        .send_signal_to_dart();
                        return Ok(());
                    }

                    // Resolve Casting.exe path (installed under app data directory)
                    let exe_path: PathBuf = std::env::current_dir()
                        .unwrap_or_default()
                        .join("Casting")
                        .join("Casting.exe");
                    if !exe_path.is_file() {
                        send_toast(
                            "Casting tool not installed".to_string(),
                            "Open Settings and download the Meta Quest Casting tool.".to_string(),
                            true,
                            None,
                        );
                        AdbCommandCompletedEvent {
                            command_type: AdbCommandType::StartCasting,
                            command_key: key.clone(),
                            success: false,
                        }
                        .send_signal_to_dart();
                        return Ok(());
                    }

                    // Ensure caches directory exists: %APPDATA%/odh/casting
                    let caches_dir = dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("odh")
                        .join("casting");
                    if let Err(e) = tokio::fs::create_dir_all(&caches_dir).await {
                        send_toast(
                            "Failed to prepare caches dir".to_string(),
                            format!("{}", e),
                            true,
                            None,
                        );
                        AdbCommandCompletedEvent {
                            command_type: AdbCommandType::StartCasting,
                            command_key: key.clone(),
                            success: false,
                        }
                        .send_signal_to_dart();
                        return Ok(());
                    }

                    // Resolve adb path
                    let adb_path_buf = match crate::utils::resolve_binary_path(
                        self.adb_path.read().await.as_deref(),
                        "adb",
                    ) {
                        Ok(p) => p,
                        Err(e) => {
                            let e = e.context("ADB binary not found");
                            send_toast(
                                "ADB binary not found".to_string(),
                                format!("{:#}", e),
                                true,
                                None,
                            );
                            AdbCommandCompletedEvent {
                                command_type: AdbCommandType::StartCasting,
                                command_key: key.clone(),
                                success: false,
                            }
                            .send_signal_to_dart();
                            return Ok(());
                        }
                    };

                    // Build command
                    let mut cmd = TokioCommand::new(&exe_path);
                    cmd.current_dir(exe_path.parent().unwrap_or_else(|| std::path::Path::new(".")));
                    cmd.arg("--adb").arg(adb_path_buf);
                    cmd.arg("--application-caches-dir").arg(&caches_dir);
                    cmd.arg("--exit-on-close");
                    cmd.arg("--launch-surface").arg("MQDH");
                    let target_json = format!("{{\"id\":\"{}\"}}", device.serial);
                    cmd.arg("--target-device").arg(target_json);
                    cmd.arg("--features").args([
                        "input_forwarding",
                        "input_forwarding_gaze_click",
                        "input_forwarding_text_input_forwarding",
                        "image_stabilization",
                        "update_device_fov_via_openxr_api",
                        "panel_streaming",
                    ]);

                    match cmd.spawn() {
                        Ok(_child) => {
                            AdbCommandCompletedEvent {
                                command_type: AdbCommandType::StartCasting,
                                command_key: key.clone(),
                                success: true,
                            }
                            .send_signal_to_dart();
                            Ok(())
                        }
                        Err(e) => {
                            send_toast(
                                "Failed to launch Casting".to_string(),
                                format!("{:#}", e),
                                true,
                                None,
                            );
                            AdbCommandCompletedEvent {
                                command_type: AdbCommandType::StartCasting,
                                command_key: key.clone(),
                                success: false,
                            }
                            .send_signal_to_dart();
                            Ok(())
                        }
                    }
                }
            }

            AdbCommand::ForceStopApp(package_name) => {
                ensure_valid_package(&package_name)?;
                let result = device.force_stop(&package_name).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandType::ForceStopApp,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Failed to force stop {package_name}: {e:#}");
                        send_toast("Stop Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to force stop {package_name}")))
                    }
                }
            }

            AdbCommand::UninstallPackage(package_name) => {
                ensure_valid_package(&package_name)?;
                let result = self.uninstall_package(&package_name).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandType::UninstallPackage,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Failed to uninstall {package_name}: {e:#}");
                        send_toast("Uninstall Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to uninstall {package_name}")))
                    }
                }
            }

            AdbCommand::RefreshDevice => match self.refresh_device().await {
                Ok(_) => Ok(()),
                Err(e) => {
                    let error_msg = format!("Failed to refresh device: {e:#}");
                    send_toast("Refresh Failed".to_string(), error_msg, true, None);
                    Err(e.context("Failed to refresh device"))
                }
            },

            // Power and device actions (parameterized)
            AdbCommand::Reboot(mode) => {
                let result = device.reboot_with_mode(mode).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandType::Reboot,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();
                result.map(|_| ()).context("Failed to reboot device")
            }

            AdbCommand::SetProximitySensor(enabled) => {
                let result = device.set_proximity_sensor(enabled).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandType::ProximitySensorSet,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();
                result.map(|_| ()).context("Failed to set proximity sensor")
            }

            AdbCommand::SetGuardianPaused(paused) => {
                let result = device.set_guardian_paused(paused).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandType::GuardianPausedSet,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();
                result.map(|_| ()).context("Failed to set guardian paused state")
            }

            AdbCommand::GetBatteryDump => match device.battery_dump().await {
                Ok(raw) => {
                    let human = battery::humanize_battery_dump(&raw);
                    BatteryDumpResponse { command_key: key.clone(), dump: human }
                        .send_signal_to_dart();
                    Ok(())
                }
                Err(e) => {
                    let error_msg = format!("Failed to get battery dump: {e:#}");
                    Toast::send("Battery Dump Failed".to_string(), error_msg, true, None);
                    Err(e.context("Failed to get battery dump"))
                }
            },
        };

        result.context("Command execution failed")
    }

    /// Updates the current device state and notifies Dart of the change
    /// # Arguments
    /// * `device` - Optional new device state
    /// * `update_current` - Whether to update the current device if it exists
    #[instrument(skip(self, device))]
    async fn set_device(&self, device: Option<AdbDevice>, update_current: bool) -> Result<()> {
        fn report_device_change(device: &Option<AdbDevice>) {
            let proto_device = device.clone().map(|d| d.into());
            DeviceChangedEvent { device: proto_device }.send_signal_to_dart();
        }

        let mut current_device = self.device.write().await;

        if update_current {
            if device.is_none() {
                error!("Attempted to pass None as a device update");
                return Err(anyhow!("Attempted to pass None as a device update"));
            }

            if let (Some(current), Some(new)) = (current_device.as_ref(), &device)
                && current.serial != new.serial
            {
                debug!(
                    current = %current.serial,
                    new = %new.serial,
                    "Ignoring device update for different device"
                );
                return Ok(());
            }
        }

        debug!(device = ?device.as_ref().map(|d| &d.serial), "Setting new device data");
        *current_device = device.clone().map(Arc::new);
        report_device_change(&device);
        Ok(())
    }

    /// Attempts to get the currently connected device    ///
    /// # Returns
    /// Option containing the current device if one is connected
    #[instrument(skip(self))]
    async fn try_current_device(&self) -> Option<Arc<AdbDevice>> {
        self.device.read().await.as_ref().map(Arc::clone)
    }

    /// Gets the currently connected device or returns an error if none is connected
    #[instrument(skip(self), level = "trace", err)]
    async fn current_device(&self) -> Result<Arc<AdbDevice>> {
        self.try_current_device().await.context("No device connected")
    }

    /// Connects to an ADB device
    #[instrument(skip(self), err, ret)]
    async fn connect_device(&self) -> Result<AdbDevice> {
        // TODO: wait for device to be ready (boot_completed)
        info!("Attempting to connect to any device");
        let adb_host = self.adb_host.clone();
        let devices = adb_host
            .devices::<Vec<_>>()
            .await?
            .into_iter()
            .filter(|d| d.state == DeviceState::Device)
            .collect::<Vec<_>>();

        // TODO: handle multiple devices
        let first_device = devices.first().context("No available device found")?;
        info!(serial = %first_device.serial, "Found device, connecting...");

        let inner_device = forensic_adb::Device::new(
            adb_host,
            first_device.serial.clone(),
            first_device.info.clone(),
        )
        .await
        .context("Failed to connect to device")?;

        let device = AdbDevice::new(inner_device).await?;
        info!(serial = %device.serial, "Device connected successfully");

        // Clean up old APKs (might be leftovers from interrupted installs)
        device.clean_temp_apks().await?;

        self.set_device(Some(device.clone()), false).await?;
        self.refresh_adb_state().await;
        Ok(device)
    }

    /// Disconnects the current ADB device
    #[instrument(skip(self), err)]
    async fn disconnect_device(&self) -> Result<()> {
        ensure!(
            self.device.read().await.is_some(),
            "Cannot disconnect from a device when none is connected"
        );
        info!("Disconnecting from device");
        self.set_device(None, false).await?;
        self.refresh_adb_state().await;
        Ok(())
    }

    /// Runs a periodic refresh of device information
    #[instrument(skip(self))]
    async fn run_periodic_refresh(&self) {
        let refresh_interval = Duration::from_secs(60);
        let mut interval = time::interval(refresh_interval);
        info!(interval = ?refresh_interval, "Starting periodic device refresh");

        loop {
            interval.tick().await;
            trace!("Device refresh tick");
            if let Some(device) = self.try_current_device().await {
                debug!(serial = %device.serial, "Performing periodic device refresh");
                if let Err(e) = self.refresh_device().await {
                    error!(error = e.as_ref() as &dyn Error, "Periodic device refresh failed");
                }
            }
        }
    }

    /// Refreshes the currently connected device
    #[instrument(skip(self), err)]
    pub async fn refresh_device(&self) -> Result<()> {
        let device = self.current_device().await?;
        // TODO: just add serial to instrument span
        debug!(serial = %device.serial, "Refreshing device data");
        let mut device_clone = (*device).clone();
        device_clone.refresh().await?;
        self.set_device(Some(device_clone), true).await?;
        debug!("Device data refreshed successfully");
        Ok(())
    }

    /// Installs an APK on the currently connected device
    #[instrument(skip(self, progress_sender))]
    pub async fn install_apk(
        &self,
        apk_path: &Path,
        backups_location: std::path::PathBuf,
        progress_sender: UnboundedSender<SideloadProgress>,
    ) -> Result<()> {
        let device = self.current_device().await?;
        let device_clone = (*device).clone();
        let result = device_clone
            .install_apk_with_progress(apk_path, &backups_location, progress_sender, false)
            .await;
        self.refresh_device().await?;
        result
    }

    /// Uninstalls a package from the currently connected device
    #[instrument(skip(self))]
    pub async fn uninstall_package(&self, package_name: &str) -> Result<()> {
        let device = self.current_device().await?;
        let device_clone = (*device).clone();
        let result = device_clone.uninstall_package(package_name).await;
        self.refresh_device().await?;
        result
    }

    /// Sideloads an app by installing its APK and pushing OBB data if present
    #[instrument(skip(self, progress_sender))]
    pub async fn sideload_app(
        &self,
        app_path: &Path,
        backups_location: std::path::PathBuf,
        progress_sender: UnboundedSender<SideloadProgress>,
    ) -> Result<()> {
        let device = self.current_device().await?;
        let device_clone = (*device).clone();
        let result = device_clone.sideload_app(app_path, &backups_location, progress_sender).await;
        self.refresh_device().await?;
        result
    }

    /// Creates a backup of an app on the currently connected device
    #[instrument(skip(self))]
    pub async fn backup_app(
        &self,
        package_name: &str,
        display_name: Option<&str>,
        backups_location: &Path,
        options: &BackupOptions,
    ) -> Result<Option<std::path::PathBuf>> {
        let device = self.current_device().await?;
        let device_clone = (*device).clone();
        device_clone.backup_app(package_name, display_name, backups_location, options).await
    }

    /// Restores a backup to the currently connected device
    #[instrument(skip(self))]
    pub async fn restore_backup(&self, backup_path: &Path) -> Result<()> {
        let device = self.current_device().await?;
        let device_clone = (*device).clone();
        let result = device_clone.restore_backup(backup_path).await;
        self.refresh_device().await?;
        result
    }

    /// Ensures the ADB server is running, starting it if necessary
    #[instrument(skip(self), /* fields(adb_host = ?self.adb_host) */, err)]
    async fn ensure_server_running(&self) -> Result<()> {
        let _guard = self.adb_server_mutex.lock().await;
        if !self.is_server_running().await {
            info!("ADB server not running, attempting to start it");
            self.set_adb_state(AdbState::ServerStarting).await;
            let adb_path_buf =
                match resolve_binary_path(self.adb_path.read().await.as_deref(), "adb") {
                    Ok(p) => p,
                    Err(e) => {
                        let e = e.context("ADB binary not found");
                        Toast::send(
                            "ADB binary not found".to_string(),
                            format!("{:#}", e),
                            true,
                            None,
                        );
                        self.set_adb_state(AdbState::ServerStartFailed).await;
                        return Err(e);
                    }
                };

            info!(path = %adb_path_buf.display(), "Found ADB binary, starting server");
            // self.adb_host
            //     .start_server(adb_path_buf.to_str())
            //     .await
            //     .context("Failed to start ADB server")
            //     .inspect_err(|e| {
            //         Toast::send(
            //             "Failed to start ADB server".to_string(),
            //             format!("{e:#}"),
            //             true,
            //             None,
            //         );
            //     })?;
            // run "adb start-server"
            let output = match timeout(Duration::from_millis(10000), {
                let mut command = Command::new(&adb_path_buf);
                command.arg("start-server");
                #[cfg(target_os = "windows")]
                // CREATE_NO_WINDOW
                command.creation_flags(0x08000000);
                command.output()
            })
            .await
            {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => {
                    Toast::send(
                        "Failed to start ADB server".to_string(),
                        format!("{e:#}"),
                        true,
                        None,
                    );
                    self.set_adb_state(AdbState::ServerStartFailed).await;
                    return Err(e).context("Failed to start ADB server");
                }
                Err(_) => {
                    let e = anyhow!("Timed out while starting ADB server");
                    Toast::send(
                        "Failed to start ADB server".to_string(),
                        e.to_string(),
                        true,
                        None,
                    );
                    self.set_adb_state(AdbState::ServerStartFailed).await;
                    return Err(e);
                }
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Toast::send(
                    "Failed to start ADB server".to_string(),
                    stderr.to_string(),
                    true,
                    None,
                );
                self.set_adb_state(AdbState::ServerStartFailed).await;
                bail!("Failed to start ADB server: {}", stderr);
            }
            self.refresh_adb_state().await;
            info!("ADB server started successfully");
        }
        Ok(())
    }

    /// Checks if the ADB server is running
    #[instrument(skip(self), level = "debug", ret)]
    async fn is_server_running(&self) -> bool {
        match timeout(Duration::from_millis(1000), self.adb_host.check_host_running()).await {
            Ok(Ok(_)) => true,
            Ok(Err(e)) => {
                error!(error = &e as &dyn Error, "Failed to check ADB server status");
                false
            }
            Err(_) => {
                debug!("Timed out while checking ADB server status (likely not running)");
                false
            }
        }
    }

    /// Gets the ADB devices
    #[instrument(skip(self), level = "debug", err, ret)]
    async fn get_adb_devices(&self) -> Result<Vec<DeviceBrief>> {
        let adb_host = self.adb_host.clone();
        let devices: Vec<DeviceBrief> =
            adb_host.devices::<Vec<_>>().await?.into_iter().map(|d| d.into()).collect();
        // debug!(count = devices.len(), "Got ADB devices");
        Ok(devices)
    }

    /// Sets the ADB state directly and notifies Dart
    #[instrument(skip(self))]
    async fn set_adb_state(&self, new_state: AdbState) {
        let mut adb_state = self.adb_state.write().await;
        if *adb_state != new_state {
            debug!(old_state = ?*adb_state, new_state = ?new_state, "ADB state changed");
            *adb_state = new_state.clone();
            new_state.send_signal_to_dart();
        }
    }

    /// Refreshes the ADB state based on the current device and server status
    #[instrument(skip(self))]
    async fn refresh_adb_state(&self) {
        let mut adb_state_lock = self.adb_state.write().await;
        let current_state = adb_state_lock.clone();
        let new_state = if !self.is_server_running().await {
            match current_state {
                AdbState::ServerStartFailed => AdbState::ServerStartFailed,
                AdbState::ServerStarting => AdbState::ServerStarting,
                _ => AdbState::ServerNotRunning,
            }
        } else if self.try_current_device().await.is_some() {
            // Get full list for count
            match self.get_adb_devices().await {
                Ok(devices) => {
                    let count = devices.len() as u32;
                    AdbState::DeviceConnected { count }
                }
                Err(_) => AdbState::DeviceConnected { count: 1 },
            }
        } else {
            match self.get_adb_devices().await {
                Ok(devices) => {
                    if devices.is_empty() {
                        AdbState::NoDevices
                    } else if devices.iter().all(|d| d.state == DeviceState::Unauthorized) {
                        AdbState::DeviceUnauthorized { count: devices.len() as u32 }
                    } else {
                        let device_serials = devices.iter().map(|d| d.serial.clone()).collect();
                        AdbState::DevicesAvailable(device_serials)
                    }
                }
                Err(e) => {
                    error!(
                        error = e.as_ref() as &dyn Error,
                        "Failed to get ADB devices for state refresh"
                    );
                    // Preserve failure/start states if they were set
                    match current_state {
                        AdbState::ServerStartFailed => AdbState::ServerStartFailed,
                        AdbState::ServerStarting => AdbState::ServerStarting,
                        _ => AdbState::ServerNotRunning,
                    }
                }
            }
        };

        if *adb_state_lock != new_state {
            debug!(old_state = ?*adb_state_lock, new_state = ?new_state, "ADB state changed");
            *adb_state_lock = new_state.clone();
            new_state.send_signal_to_dart();
        } else {
            trace!(state = ?new_state, "ADB state unchanged");
        }
    }
}
