use std::{
    collections::HashMap,
    error::Error,
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use derive_more::Debug;
use forensic_adb::{DeviceBrief, DeviceInfo, DeviceState};
use lazy_regex::{Lazy, Regex, lazy_regex};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use rinf::{DartSignal, RustSignal};
use tokio::{
    process::Command,
    sync::{Mutex, RwLock, mpsc::UnboundedSender},
    time::{self, timeout},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, info_span, instrument, trace, warn};

use super::device::AdbDevice;
use crate::{
    adb::device::{BackupOptions, SideloadProgress},
    models::{
        ConnectionKind, Settings,
        signals::{
            adb::{
                command::*,
                device::DeviceChangedEvent,
                devices_list::{AdbDeviceBrief, AdbDevicesList},
                dump::BatteryDumpResponse,
                state::AdbState,
            },
            system::Toast,
        },
    },
    utils::resolve_binary_path,
};

pub(crate) static PACKAGE_NAME_REGEX: Lazy<Regex> =
    lazy_regex!(r"^(?:[A-Za-z]{1}[\w]*\.)+[A-Za-z][\w]*$");

/// Validated Android package name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageName(String);

impl PackageName {
    /// Validates and constructs a `PackageName` from the provided string-like value.
    pub(crate) fn parse(value: impl AsRef<str>) -> Result<Self> {
        let value_ref = value.as_ref();
        ensure!(PACKAGE_NAME_REGEX.is_match(value_ref), "Invalid package name: '{}'", value_ref);
        Ok(Self(value_ref.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Default)]
struct CachedDeviceData {
    pub name: String,
    pub true_serial: String,
}

/// Handles ADB device connections and commands
#[derive(Debug)]
pub(crate) struct AdbHandler {
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
    /// Serializes connect/disconnect operations to avoid races
    device_op_mutex: Mutex<()>,
    /// Cancellation token for running tasks
    cancel_token: RwLock<CancellationToken>,
    /// Cache of adb transport_id -> device data
    device_data_cache: RwLock<HashMap<String, CachedDeviceData>>,
    /// Whether mDNS auto-connect is enabled
    mdns_auto_connect: bool,
    /// Preferred connection type (USB or Wireless) for auto-connect
    preferred_connection_type: RwLock<ConnectionKind>,
}

impl AdbHandler {
    /// Creates a new AdbHandler instance and starts device monitoring.
    /// This is the main entry point for ADB functionality.
    ///
    /// # Returns
    /// Arc-wrapped AdbHandler that manages ADB device connections
    #[instrument(level = "debug", skip(settings_stream))]
    pub(crate) async fn new(mut settings_stream: WatchStream<Settings>) -> Arc<Self> {
        let first_settings =
            settings_stream.next().await.expect("Settings stream closed on adb init");
        let adb_path = first_settings.adb_path;
        let adb_path = if adb_path.is_empty() { None } else { Some(adb_path) };
        let handle = Arc::new(Self {
            adb_host: if cfg!(target_os = "windows") {
                // No idea why, but it fails to connect on a Windows host without this
                // However, passing this host to `adb start-server` fails too (so we can't use `adb_host.start_server()`)
                forensic_adb::Host { host: Some("127.0.0.1".to_string()), port: Some(5037) }
            } else {
                forensic_adb::Host::default()
            },
            adb_server_mutex: Mutex::new(()),
            adb_path: RwLock::new(adb_path),
            adb_state: RwLock::new(AdbState::default()),
            device: None.into(),
            device_op_mutex: Mutex::new(()),
            cancel_token: RwLock::new(CancellationToken::new()),
            device_data_cache: RwLock::new(HashMap::new()),
            mdns_auto_connect: first_settings.mdns_auto_connect,
            preferred_connection_type: RwLock::new(first_settings.preferred_connection_type),
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
    #[instrument(level = "debug", skip(self, settings_stream))]
    async fn start_tasks(self: Arc<AdbHandler>, mut settings_stream: WatchStream<Settings>) {
        // Handle settings updates
        tokio::spawn(
            {
                let handle = self.clone();
                async move {
                    debug!("AdbHandler starting to listen for settings changes");
                    while let Some(settings) = settings_stream.next().await {
                        debug!("AdbHandler received settings update");
                        debug!(?settings, "New settings");
                        let new_adb_path = settings.adb_path.clone();
                        let new_adb_path =
                            if new_adb_path.is_empty() { None } else { Some(new_adb_path) };
                        if new_adb_path != *handle.adb_path.read().await {
                            info!(?new_adb_path, "ADB path changed, restarting ADB");
                            *handle.adb_path.write().await = new_adb_path;
                            if let Err(e) = handle.clone().restart_adb().await {
                                error!(error = e.as_ref() as &dyn Error, "Failed to restart ADB");
                                Toast::send(
                                    "Failed to restart ADB".to_string(),
                                    format!("{e}"),
                                    true,
                                    Some(Duration::from_secs(5)),
                                );
                            }
                        }

                        let new_connection_type = settings.preferred_connection_type;
                        if new_connection_type != *handle.preferred_connection_type.read().await {
                            info!(?new_connection_type, "Preferred connection type changed");
                            *handle.preferred_connection_type.write().await = new_connection_type;
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
    #[instrument(level = "debug", skip(self))]
    async fn start_adb_tasks(self: Arc<AdbHandler>) {
        let cancel_token = self.cancel_token.read().await.clone();
        debug!("Starting ADB tasks");

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

        // mDNS auto-connect for ADB-over-Wi‑Fi targets (applies on startup)
        if self.mdns_auto_connect {
            tokio::spawn({
                let handle = self.clone();
                let cancel_token = self.cancel_token.read().await.clone();
                async move {
                    let result =
                        cancel_token.run_until_cancelled(handle.run_mdns_auto_connect()).await;
                    debug!(result = ?result, "mDNS auto-connect task finished");
                    result
                }
            });
        }
    }

    /// Restarts the ADB handling
    // TODO: make sure this cannot race with `ensure_server_running`
    #[instrument(skip(self), err)]
    async fn restart_adb(self: Arc<AdbHandler>) -> Result<()> {
        info!("Restarting ADB server and tasks");
        // Cancel all tasks
        self.cancel_token.read().await.cancel();
        // Disconnect from device
        let _ = self.disconnect_device(None).await;
        // Drop cache
        self.device_data_cache.write().await.clear();
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
    #[instrument(level = "debug", skip(self), err)]
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
    #[instrument(level = "debug", skip(self, sender), err)]
    async fn run_device_tracker(
        self: Arc<AdbHandler>,
        sender: tokio::sync::mpsc::UnboundedSender<Vec<DeviceBrief>>,
    ) -> Result<()> {
        loop {
            debug!("Starting track_devices loop");
            self.ensure_server_running().await?;
            let stream = self.adb_host.track_devices();
            tokio::pin!(stream);
            let mut got_update = false;

            while let Some(device_result) = stream.next().await {
                match device_result {
                    Ok(device_list) => {
                        got_update = true;
                        if sender.send(device_list).is_err() {
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
    #[instrument(level = "debug", skip(self, receiver), err)]
    async fn handle_device_updates(
        self: Arc<AdbHandler>,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<DeviceBrief>>,
    ) -> Result<()> {
        while let Some(devices) = receiver.recv().await {
            debug!(update = ?devices, "Received device list update");

            if let Some(current) = self.try_current_device().await {
                let still_present = devices
                    .iter()
                    .any(|d| d.serial == current.serial && d.state == DeviceState::Device);
                if !still_present {
                    info!(
                        serial = %current.serial,
                        "Current device missing from device list or is not in \"device\" state, disconnecting"
                    );
                    if let Err(e) = self.disconnect_device(Some(&current.serial)).await {
                        error!(error = e.as_ref() as &dyn Error, "Auto-disconnect failed");
                    }
                }
            }

            if self.try_current_device().await.is_none()
                && devices.iter().any(|d| d.state == DeviceState::Device)
            {
                info!("Found available device, auto-connecting");
                let preferred = *self.preferred_connection_type.read().await;
                if let Err(e) = self.connect_device(None, preferred).await {
                    error!(error = e.as_ref() as &dyn Error, "Auto-connect failed");
                }
            }

            self.refresh_adb_state().await;
        }

        bail!("Device update channel closed unexpectedly");
    }

    /// Listens for and processes ADB commands received from Dart
    #[instrument(level = "debug", skip(self))]
    async fn receive_commands(&self) {
        let receiver = AdbRequest::get_dart_signal_receiver();
        info!("Listening for ADB commands");
        while let Some(request) = receiver.recv().await {
            debug!(command = ?request.message.command, key = %request.message.command_key, "Received ADB command");
            if let Err(e) =
                self.execute_command(request.message.command_key, request.message.command).await
            {
                error!(error = e.as_ref() as &dyn Error, "ADB command execution failed");
            }
        }
        panic!("AdbRequest receiver closed");
    }

    /// Executes a received ADB command with the given parameters
    #[instrument(level = "debug", skip(self))]
    async fn execute_command(&self, key: String, command: AdbCommand) -> Result<()> {
        fn send_toast(title: String, description: String, error: bool, duration: Option<Duration>) {
            Toast::send(title, description, error, duration);
        }

        let result = match command.clone() {
            AdbCommand::LaunchApp(package_name) => {
                let device = self.current_device().await?;
                let package = PackageName::parse(&package_name)?;
                let result = device.launch(&package).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::LaunchApp,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Failed to launch {package}: {e:#}");
                        send_toast("Launch Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to launch {package}")))
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
                        command_type: AdbCommandKind::StartCasting,
                        command_key: key.clone(),
                        success: false,
                    }
                    .send_signal_to_dart();
                    Ok(())
                }
                #[cfg(target_os = "windows")]
                {
                    use crate::casting::CastingManager;

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
                                command_type: AdbCommandKind::StartCasting,
                                command_key: key.clone(),
                                success: false,
                            }
                            .send_signal_to_dart();
                            return Ok(());
                        }
                    };

                    let device = self.current_device().await?;
                    let wireless = device.is_wireless;
                    let device_serial = &device.true_serial;

                    match CastingManager::start_casting(&adb_path_buf, device_serial, wireless)
                        .await
                    {
                        Ok(_) => {
                            AdbCommandCompletedEvent {
                                command_type: AdbCommandKind::StartCasting,
                                command_key: key.clone(),
                                success: true,
                            }
                            .send_signal_to_dart();
                            Ok(())
                        }
                        Err(_) => {
                            AdbCommandCompletedEvent {
                                command_type: AdbCommandKind::StartCasting,
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
                let device = self.current_device().await?;
                let package = PackageName::parse(&package_name)?;
                let result = device.force_stop(&package).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::ForceStopApp,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Failed to force stop {package}: {e:#}");
                        send_toast("Stop Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to force stop {package}")))
                    }
                }
            }

            AdbCommand::UninstallPackage(package_name) => {
                let device = self.current_device().await?;
                let package = PackageName::parse(&package_name)?;
                let result = self.uninstall_package(&device, &package).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::UninstallPackage,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Failed to uninstall {package}: {e:#}");
                        send_toast("Uninstall Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to uninstall {package}")))
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
                let device = self.current_device().await?;
                let result = device.reboot_with_mode(mode).await;
                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::Reboot,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();
                result.map(|_| ()).context("Failed to reboot device")
            }

            AdbCommand::SetProximitySensor { enabled, duration_ms } => {
                let device = self.current_device().await?;
                let result = device.set_proximity_sensor(enabled, duration_ms).await;
                let success = result.is_ok();
                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::ProximitySensorSet,
                    command_key: key.clone(),
                    success,
                }
                .send_signal_to_dart();
                // Refresh device state to update proximity_disabled field
                if success {
                    let _ = self.refresh_device().await;
                }
                result.map(|_| ()).context("Failed to set proximity sensor")
            }

            AdbCommand::SetGuardianPaused(paused) => {
                let device = self.current_device().await?;
                let result = device.set_guardian_paused(paused).await;
                let success = result.is_ok();
                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::GuardianPausedSet,
                    command_key: key.clone(),
                    success,
                }
                .send_signal_to_dart();
                // Refresh guardian state
                if success {
                    let _ = self.refresh_device().await;
                }
                result.map(|_| ()).context("Failed to set guardian paused state")
            }

            AdbCommand::GetBatteryDump => {
                let device = self.current_device().await?;
                match device.battery_dump().await {
                    Ok(dump) => {
                        BatteryDumpResponse { command_key: key.clone(), dump }
                            .send_signal_to_dart();
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to get battery dump: {e:#}");
                        Toast::send("Battery Dump Failed".to_string(), error_msg, true, None);
                        Err(e.context("Failed to get battery dump"))
                    }
                }
            }

            AdbCommand::ConnectTo(serial) => {
                // Skip if already connected to the requested device
                if let Some(current) = self.try_current_device().await
                    && current.serial == serial
                {
                    AdbCommandCompletedEvent {
                        command_type: AdbCommandKind::ConnectTo,
                        command_key: key.clone(),
                        success: true,
                    }
                    .send_signal_to_dart();
                    return Ok(());
                }

                let preferred = *self.preferred_connection_type.read().await;
                let result = self.connect_device(Some(&serial), preferred).await;

                AdbCommandCompletedEvent {
                    command_type: AdbCommandKind::ConnectTo,
                    command_key: key.clone(),
                    success: result.is_ok(),
                }
                .send_signal_to_dart();

                match result {
                    Ok(_) => {
                        self.refresh_adb_state().await;
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("{serial}: {e:#}");
                        Toast::send("Device Connect Failed".to_string(), error_msg, true, None);
                        Err(e.context("Failed to connect to selected device"))
                    }
                }
            }

            AdbCommand::EnableWirelessAdb => {
                let device = self.current_device().await?;

                if device.is_wireless {
                    AdbCommandCompletedEvent {
                        command_type: AdbCommandKind::WirelessAdbEnable,
                        command_key: key.clone(),
                        success: false,
                    }
                    .send_signal_to_dart();
                    bail!("Current device is already wireless")
                }

                // Step 1: enable Wireless ADB (tcpip mode) and compute target address
                match device.enable_wireless_adb().await {
                    Ok(addr) => {
                        // Report success, things can get kinda random from here
                        AdbCommandCompletedEvent {
                            command_type: AdbCommandKind::WirelessAdbEnable,
                            command_key: key.clone(),
                            success: true,
                        }
                        .send_signal_to_dart();

                        Toast::send(
                            "Wireless ADB enabled".to_string(),
                            format!("Trying to connect to {}…", addr),
                            false,
                            Some(Duration::from_secs(3)),
                        );

                        // Step 2: attempt to connect and then switch current device
                        if let Err(e) = self.try_connect_wireless_adb(addr.into()).await {
                            warn!(error = e.as_ref() as &dyn Error, target = %display_target(addr.into()), "Wireless ADB connect failed");
                            Toast::send(
                                "ADB connect failed".to_string(),
                                format!("{}", e),
                                true,
                                None,
                            );
                            return Ok(());
                        }

                        let serial = addr.to_string();
                        const MAX_SWITCH_ATTEMPTS: usize = 3;

                        tokio::time::sleep(Duration::from_millis(300)).await;

                        let preferred = *self.preferred_connection_type.read().await;
                        let mut last_err: Option<anyhow::Error> = None;
                        for attempt in 1..=MAX_SWITCH_ATTEMPTS {
                            match self.connect_device(Some(&serial), preferred).await {
                                Ok(_) => {
                                    last_err = None;
                                    break;
                                }
                                Err(e) => {
                                    let e_str = format!("{:#}", e);
                                    let retryable = e_str.contains("not available");

                                    if attempt < MAX_SWITCH_ATTEMPTS && retryable {
                                        debug!(
                                            attempt,
                                            serial = %serial,
                                            "Wireless device not yet available, retrying"
                                        );
                                        last_err = Some(e);
                                        tokio::time::sleep(Duration::from_millis(600)).await;
                                        continue;
                                    }

                                    last_err = Some(e);
                                    break;
                                }
                            }
                        }

                        if let Some(e) = last_err {
                            warn!(
                                error = e.as_ref() as &dyn Error,
                                serial = %serial,
                                "Switch to wireless connection failed"
                            );
                            Toast::send(
                                "Switch to Wireless failed".to_string(),
                                format!("{}", e),
                                true,
                                None,
                            );
                        }
                    }
                    Err(e) => {
                        Toast::send(
                            "Enable Wireless ADB failed".to_string(),
                            format!("{:#}", e),
                            true,
                            None,
                        );

                        AdbCommandCompletedEvent {
                            command_type: AdbCommandKind::WirelessAdbEnable,
                            command_key: key.clone(),
                            success: false,
                        }
                        .send_signal_to_dart();
                    }
                }

                Ok(())
            }
        };

        result.context("Command execution failed")
    }

    /// Atomically set the current device if the expected serial matches.
    ///
    /// - If `expect_serial` is `Some(s)`, the set happens only when the current device's serial is `s`.
    /// - If `expect_serial` is `None`, the set happens only when there is no current device.
    ///
    /// Returns `true` if the device was set, `false` if the expectation failed.
    #[instrument(level = "debug", skip(self, device))]
    async fn set_device(
        &self,
        device: Option<AdbDevice>,
        expect_serial: Option<&str>,
    ) -> Result<bool> {
        let device_clone = device.clone();

        let mut current_device = self.device.write().await;
        let current_serial = current_device.as_ref().map(|d| d.serial.as_str());

        if current_serial != expect_serial {
            trace!(current = ?current_serial, expect = ?expect_serial, "Compare-and-set failed for set_device");
            return Ok(false);
        }

        debug!(device = ?device.as_ref().map(|d| &d.serial), "Setting new device data");
        *current_device = device.map(Arc::new);

        DeviceChangedEvent { device: device_clone.map(|d| d.into()) }.send_signal_to_dart();
        Ok(true)
    }

    /// Attempts to get the currently connected device    ///
    /// # Returns
    /// Option containing the current device if one is connected
    #[instrument(level = "debug", skip(self))]
    async fn try_current_device(&self) -> Option<Arc<AdbDevice>> {
        self.device.read().await.as_ref().map(Arc::clone)
    }

    /// Gets the currently connected device or returns an error if none is connected
    #[instrument(skip(self), level = "debug", err)]
    pub(crate) async fn current_device(&self) -> Result<Arc<AdbDevice>> {
        self.try_current_device().await.context("No device connected")
    }

    /// Connects to an ADB device
    ///
    /// # Arguments
    /// * `serial` - Optional serial number to target. If None, connects to the first available device.
    /// * `preferred_connection` - Preferred connection type. Ignored if `serial` is provided.
    #[instrument(skip(self), err, ret)]
    async fn connect_device(
        &self,
        serial: Option<&str>,
        preferred_connection: ConnectionKind,
    ) -> Result<AdbDevice> {
        let prefer_usb = preferred_connection == ConnectionKind::Usb;
        let adb_host = self.adb_host.clone();
        let mut devices = adb_host
            .devices::<Vec<_>>()
            .await?
            .into_iter()
            .filter(|d| d.state == DeviceState::Device)
            .collect::<Vec<_>>();

        // Select target device based on serial parameter
        let target_device = if let Some(target_serial) = serial {
            if let Some(current) = self.try_current_device().await
                && current.serial == target_serial
            {
                info!(serial = %target_serial, "Device already connected, skipping");
                return Ok((*current).clone());
            }

            info!(%target_serial, "Attempting to connect to specific device");
            devices
                .into_iter()
                .find(|d| d.serial == target_serial)
                .with_context(|| format!("Requested device {target_serial} not available"))?
        } else {
            info!(prefer_usb, "Attempting to connect to first available device");

            if devices.is_empty() {
                bail!("No devices available");
            }

            // Sort devices by USB/wireless preference
            devices.sort_by_key(|d| {
                let is_usb = !d.serial.contains(':');
                if prefer_usb { !is_usb } else { is_usb }
            });

            devices.first().cloned().context("No devices available")?
        };

        // Serialize connect/disconnect operations to avoid races
        let _op_guard = self.device_op_mutex.lock().await;

        if let Some(target) = serial
            && let Some(current) = self.try_current_device().await
            && current.serial == target
        {
            info!(serial = %target, "Device already connected, skipping");
            return Ok((*current).clone());
        }

        info!(serial = %target_device.serial, "Found device, connecting...");

        let inner_device = forensic_adb::Device::new(
            adb_host,
            target_device.serial.clone(),
            target_device.info.clone(),
        )
        .await
        .context("Failed to connect to device")?;

        let device = AdbDevice::new(inner_device).await?;
        let prev = self.try_current_device().await;

        // Clean up old APKs (might be leftovers from interrupted installs)
        device.clean_temp_apks().await?;

        let set_ok = if let Some(prev_dev) = &prev {
            debug!(from = %prev_dev.serial, to = %device.serial, "Switching connected device");
            self.set_device(Some(device.clone()), Some(&prev_dev.serial)).await?
        } else {
            debug!(to = %device.serial, "Setting first connected device");
            self.set_device(Some(device.clone()), None).await?
        };

        if !set_ok {
            bail!("Failed to switch device: current changed concurrently");
        }

        match prev {
            Some(prev_dev) if prev_dev.serial != device.serial => {
                let new_name = device.name.as_deref().unwrap_or("Unknown");
                Toast::send(
                    "Switched device".to_string(),
                    format!("{} ({})", new_name, device.serial),
                    false,
                    Some(Duration::from_secs(3)),
                );
            }
            None => {
                Toast::send(
                    "Connected to device".to_string(),
                    format!(
                        "{} ({})",
                        device.name.as_ref().unwrap_or(&"Unknown".to_string()),
                        device.serial
                    ),
                    false,
                    Some(Duration::from_secs(3)),
                );
            }
            _ => {}
        }

        self.refresh_adb_state().await;
        Ok(device)
    }

    /// Disconnects the current ADB device
    ///
    /// # Arguments
    /// * `serial` - Optional serial number to target. If None, disconnects current device.
    ///              If Some, only disconnects if the current device matches this serial.
    #[instrument(skip(self), err)]
    async fn disconnect_device(&self, serial: Option<&str>) -> Result<()> {
        let _op_guard = self.device_op_mutex.lock().await;

        let current = self.try_current_device().await;
        let Some(current) = current else {
            bail!("Cannot disconnect from a device when none is connected");
        };

        if let Some(target_serial) = serial
            && current.serial != target_serial
        {
            debug!(
                current = %current.serial,
                target = %target_serial,
                "Ignoring disconnect request for different device"
            );
            return Ok(());
        }

        info!(serial = %current.serial, "Disconnecting from device");
        let name = current.name.clone();
        let serial_owned = current.serial.clone();

        let cleared = self.set_device(None, Some(&serial_owned)).await?;

        if cleared {
            Toast::send(
                "Disconnected from device".to_string(),
                format!("{} ({})", name.unwrap_or_else(|| "Unknown".to_string()), serial_owned),
                true,
                Some(Duration::from_secs(3)),
            );
            self.refresh_adb_state().await;
        }

        Ok(())
    }

    /// Runs a periodic refresh of device information
    #[instrument(level = "debug", skip(self))]
    async fn run_periodic_refresh(&self) {
        let refresh_interval = Duration::from_secs(60);
        let mut interval = time::interval(refresh_interval);
        debug!(interval = ?refresh_interval, "Starting periodic device refresh");

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

    /// Browses for ADB-over-Wi‑Fi services via mDNS and attempts ADB `connect`.
    #[instrument(level = "debug", skip(self), err)]
    async fn run_mdns_auto_connect(self: Arc<AdbHandler>) -> Result<()> {
        if let Err(e) = self.ensure_server_running().await {
            warn!(error = e.as_ref() as &dyn Error, "ADB server not running prior to mDNS start");
        }

        const MDNS_SERVICE_TYPES: &[&str] =
            &["_adb-tls-connect._tcp.local.", "_adb_secure_connect._tcp.local."];

        let mdns = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                warn!(error = &e as &dyn Error, "Failed to start mDNS daemon");
                return Err(e.into());
            }
        };

        let mut workers = Vec::new();

        for ty in MDNS_SERVICE_TYPES {
            let rx = match mdns.browse(ty) {
                Ok(rx) => rx,
                Err(e) => {
                    warn!(error = &e as &dyn Error, service = %ty, "Failed to start mDNS browse");
                    continue;
                }
            };
            let this = self.clone();

            let handle = tokio::spawn(async move {
                debug!("mDNS: browsing `{}`", ty);
                loop {
                    match rx.recv_async().await {
                        Ok(ServiceEvent::ServiceResolved(resolved)) => {
                            let port = resolved.get_port();
                            for ip in resolved
                                .get_addresses()
                                .iter()
                                .filter(|a| !a.is_loopback())
                                .map(|a| a.to_ip_addr())
                            {
                                let addr = SocketAddr::new(ip, port);
                                debug!(
                                    target = %display_target(addr),
                                    fullname = %resolved.get_fullname(),
                                    "Found Wireless ADB service, attempting connect"
                                );

                                // Fire-and-forget
                                let this = this.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = this.try_connect_wireless_adb(addr).await {
                                        warn!(error = e.as_ref() as &dyn Error, target = %display_target(addr), "mDNS auto-connect failed");
                                    }
                                });
                            }
                        }
                        Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                            debug!("mDNS: service removed: {}", fullname);
                        }
                        Ok(ServiceEvent::ServiceFound(_, fullname)) => {
                            trace!("mDNS: service found: {}", fullname);
                        }
                        Ok(ServiceEvent::SearchStarted(s)) => trace!("mDNS: search started: {}", s),
                        Ok(ServiceEvent::SearchStopped(s)) => trace!("mDNS: search stopped: {}", s),
                        Ok(_) => {}
                        Err(e) => {
                            warn!(error = &e as &dyn Error, service = %ty, "mDNS browse channel closed");
                            break;
                        }
                    }
                }
            });

            workers.push(handle);
        }

        for w in workers {
            let _ = w.await;
        }

        Ok(())
    }

    /// Attempts to connect to a Wireless ADB target discovered via mDNS.
    #[instrument(skip(self), fields(target = %display_target(addr)), err)]
    async fn try_connect_wireless_adb(&self, addr: SocketAddr) -> Result<()> {
        self.ensure_server_running().await.ok();

        let target = match addr {
            SocketAddr::V4(_) => format!("{}:{}", addr.ip(), addr.port()),
            SocketAddr::V6(_) => format!("[{}]:{}", addr.ip(), addr.port()),
        };

        // If already connected, exit early
        if let Ok(devs) = self.adb_host.devices::<Vec<_>>().await {
            let already = devs.iter().any(|d| d.serial.contains(&target));
            if already {
                debug!("Wireless ADB target already connected, skipping");
                return Ok(());
            }
        }

        // Retry connect for up to 10s with short timeouts.
        const TOTAL_WAIT: Duration = Duration::from_secs(10);
        const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);
        const SLEEP_BETWEEN: Duration = Duration::from_millis(400);

        let started = tokio::time::Instant::now();
        loop {
            // Check if it got connected via other means meanwhile
            if let Ok(devs) = self.adb_host.devices::<Vec<_>>().await
                && devs.iter().any(|d| d.serial.contains(&target))
            {
                info!(%target, "Wireless ADB target became connected");
                self.refresh_adb_state().await;
                return Ok(());
            }

            info!(%target, "ADB connect attempt");
            match tokio::time::timeout(ATTEMPT_TIMEOUT, self.adb_host.connect_device(&target)).await
            {
                Ok(Ok(msg)) => {
                    info!(response = %msg, "ADB connect ok");
                    self.refresh_adb_state().await;
                    return Ok(());
                }
                Ok(Err(e)) => {
                    debug!(error = &e as &dyn Error, %target, "ADB connect attempt failed");
                }
                Err(_) => {
                    debug!(%target, "ADB connect attempt timed out");
                }
            }

            if started.elapsed() >= TOTAL_WAIT {
                bail!("Timed out connecting to {}", target);
            }

            tokio::time::sleep(SLEEP_BETWEEN).await;
        }
    }

    /// Refreshes the currently connected device
    #[instrument(level = "debug", skip(self), err)]
    pub(crate) async fn refresh_device(&self) -> Result<()> {
        let device = self.current_device().await?;
        // TODO: just add serial to instrument span
        debug!(serial = %device.serial, "Refreshing device data");
        let mut device_clone = (*device).clone();
        device_clone.refresh().await?;

        let _ = self.set_device(Some(device_clone), Some(&device.serial)).await?;
        debug!("Device data refreshed successfully");
        Ok(())
    }

    /// Installs an APK on the currently connected device
    #[instrument(level = "debug", skip(self, progress_sender))]
    pub(crate) async fn install_apk(
        &self,
        device: &AdbDevice,
        apk_path: &Path,
        backups_location: std::path::PathBuf,
        progress_sender: UnboundedSender<SideloadProgress>,
    ) -> Result<()> {
        let result = device
            .install_apk_with_progress(apk_path, &backups_location, progress_sender, false)
            .await;
        self.refresh_device().await?;
        result
    }

    /// Uninstalls a package from the currently connected device
    #[instrument(level = "debug", skip(self))]
    pub(crate) async fn uninstall_package(
        &self,
        device: &AdbDevice,
        package: &PackageName,
    ) -> Result<()> {
        let result = device.uninstall_package(package).await;
        self.refresh_device().await?;
        result
    }

    /// Sideloads an app by installing its APK and pushing OBB data if present
    #[instrument(level = "debug", skip(self, progress_sender))]
    pub(crate) async fn sideload_app(
        &self,
        device: &AdbDevice,
        app_path: &Path,
        backups_location: std::path::PathBuf,
        progress_sender: UnboundedSender<SideloadProgress>,
        token: CancellationToken,
    ) -> Result<()> {
        let result = device.sideload_app(app_path, &backups_location, progress_sender, token).await;
        self.refresh_device().await?;
        result
    }

    /// Creates a backup of an app on the currently connected device
    #[instrument(level = "debug", skip(self))]
    pub(crate) async fn backup_app(
        &self,
        device: &AdbDevice,
        package: &PackageName,
        display_name: Option<&str>,
        backups_location: &Path,
        options: &BackupOptions,
        token: CancellationToken,
    ) -> Result<Option<std::path::PathBuf>> {
        device.backup_app(package, display_name, backups_location, options, token).await
    }

    /// Restores a backup to the currently connected device
    #[instrument(level = "debug", skip(self))]
    pub(crate) async fn restore_backup(
        &self,
        device: &AdbDevice,
        backup_path: &Path,
    ) -> Result<()> {
        let result = device.restore_backup(backup_path).await;
        self.refresh_device().await?;
        result
    }

    /// Pulls an application's APK and OBB (if present) into a local directory suitable for donation.
    ///
    /// Layout:
    /// - `<dest_root>/<package_name>/<package_name>.apk`
    /// - `<dest_root>/<package_name>/` + OBB contents (when present)
    #[instrument(level = "debug", skip(self, dest_root), err)]
    pub(crate) async fn pull_app_for_donation(
        &self,
        device: &AdbDevice,
        package: &PackageName,
        dest_root: &Path,
    ) -> Result<PathBuf> {
        device.pull_app_for_donation(package, dest_root).await
    }

    /// Ensures the ADB server is running, starting it if necessary
    #[instrument(level = "debug", skip(self), /* fields(adb_host = ?self.adb_host) */, err)]
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
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
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
    async fn get_adb_devices(&self) -> Result<Vec<DeviceInfo>> {
        let adb_host = self.adb_host.clone();
        let devices: Vec<DeviceInfo> = adb_host.devices::<Vec<_>>().await?.into_iter().collect();
        // debug!(count = devices.len(), "Got ADB devices");
        Ok(devices)
    }

    /// Sets the ADB state directly and notifies Dart
    #[instrument(level = "debug", skip(self))]
    async fn set_adb_state(&self, new_state: AdbState) {
        let mut adb_state = self.adb_state.write().await;
        if *adb_state != new_state {
            debug!(old_state = ?*adb_state, new_state = ?new_state, "ADB state changed");
            *adb_state = new_state.clone();
            new_state.send_signal_to_dart();
        }
    }

    /// Refreshes the ADB state based on the current device and server status
    #[instrument(level = "debug", skip(self))]
    async fn refresh_adb_state(&self) {
        let mut devices_list: Vec<DeviceInfo> = vec![];
        let current_state = self.adb_state.read().await.clone();
        let new_state = if !self.is_server_running().await {
            self.emit_devices_list(&[] as &[DeviceInfo]).await;
            match current_state {
                AdbState::ServerStartFailed => AdbState::ServerStartFailed,
                AdbState::ServerStarting => AdbState::ServerStarting,
                _ => AdbState::ServerNotRunning,
            }
        } else {
            match self.get_adb_devices().await {
                Ok(devices) => {
                    self.emit_devices_list(&devices).await;

                    devices_list = devices.clone();

                    let online_devices = devices
                        .iter()
                        .filter(|d| d.state == DeviceState::Device)
                        .collect::<Vec<_>>();
                    let online_serials =
                        online_devices.iter().map(|d| d.serial.clone()).collect::<Vec<_>>();

                    // Choose state based on presence

                    if self.try_current_device().await.is_some() {
                        AdbState::DeviceConnected
                    } else if devices.is_empty() {
                        AdbState::NoDevices
                    } else if devices.iter().all(|d| d.state == DeviceState::Unauthorized) {
                        AdbState::DeviceUnauthorized
                    } else if !online_devices.is_empty() {
                        AdbState::DevicesAvailable(online_serials)
                    } else {
                        AdbState::NoDevices
                    }
                }
                Err(e) => {
                    error!(
                        error = e.as_ref() as &dyn Error,
                        "Failed to get ADB devices for state refresh"
                    );
                    self.emit_devices_list(&[] as &[DeviceInfo]).await;
                    // Preserve failure/start states if they were set
                    match current_state {
                        AdbState::ServerStartFailed => AdbState::ServerStartFailed,
                        AdbState::ServerStarting => AdbState::ServerStarting,
                        _ => AdbState::ServerNotRunning,
                    }
                }
            }
        };

        let mut adb_state_lock = self.adb_state.write().await;
        if *adb_state_lock != new_state {
            debug!(old_state = ?*adb_state_lock, new_state = ?new_state, "ADB state changed");
            *adb_state_lock = new_state.clone();
            new_state.send_signal_to_dart();
        } else {
            trace!(state = ?new_state, "ADB state unchanged");
        }

        if let Err(e) = self.resolve_device_data(&devices_list).await {
            warn!(error = e.as_ref() as &dyn Error, "Resolving device names failed");
        }
    }

    /// Emits the AdbDevicesList signal using the provided devices and cached data
    async fn emit_devices_list(&self, devices: &[DeviceInfo]) {
        let current = self.try_current_device().await;
        if let Some(dev) = &current
            && dev.name.is_some()
        {
            self.device_data_cache.write().await.insert(
                dev.transport_id.clone(),
                CachedDeviceData {
                    name: dev.name.clone().unwrap(),
                    true_serial: dev.true_serial.clone(),
                },
            );
        }

        let cache = self.device_data_cache.read().await;
        let list = devices
            .iter()
            .map(|d| {
                let cached = d.info.get("transport_id").and_then(|s| cache.get(s));
                AdbDeviceBrief {
                    serial: d.serial.clone(),
                    is_wireless: d.serial.contains(':'),
                    state: d.state.clone().into(),
                    name: cached.map(|d| d.name.clone()),
                    true_serial: cached.map(|d| d.true_serial.clone()),
                }
            })
            .collect();
        AdbDevicesList { value: list }.send_signal_to_dart();
    }

    /// Resolves and caches device data for ready devices missing entries, then re-emits list
    #[instrument(level = "debug", skip(self), err)]
    async fn resolve_device_data(&self, devices: &[DeviceInfo]) -> Result<()> {
        let cache = self.device_data_cache.read().await;
        let current = self.try_current_device().await;
        let current_serial = current.as_ref().map(|d| d.serial.clone());
        let to_resolve = devices
            .iter()
            .filter(|d| d.state == DeviceState::Device)
            .filter(|d| Some(d.serial.as_str()) != current_serial.as_deref())
            .filter(|d| !cache.contains_key(&d.serial))
            .cloned()
            .collect::<Vec<_>>();
        drop(cache);

        if to_resolve.is_empty() {
            return Ok(());
        }

        let adb_host = self.adb_host.clone();
        let all = adb_host.devices::<Vec<_>>().await?;
        let mut resolved: HashMap<String, CachedDeviceData> = HashMap::new();
        for d in to_resolve {
            if let Some(entry) = all.iter().find(|e| e.serial == d.serial) {
                let device = forensic_adb::Device::new(
                    adb_host.clone(),
                    entry.serial.clone(),
                    entry.info.clone(),
                )
                .await?;
                if let Ok(name) = AdbDevice::query_identity(&device).await
                    && let Ok(true_serial) = AdbDevice::query_true_serial(&device).await
                    && let Some(transport_id) = device.info.get("transport_id").cloned()
                {
                    resolved.insert(transport_id, CachedDeviceData { name, true_serial });
                }
            }
        }

        if !resolved.is_empty() {
            self.device_data_cache.write().await.extend(resolved);
            self.emit_devices_list(devices).await;
        }

        Ok(())
    }
}

/// Formats wireless ADB target address for logging
fn display_target(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(_) => format!("{}:{}", addr.ip(), addr.port()),
        SocketAddr::V6(_) => format!("[{}]:{}", addr.ip(), addr.port()),
    }
}
