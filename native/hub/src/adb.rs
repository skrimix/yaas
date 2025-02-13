use std::{error::Error, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use arc_swap::ArcSwapOption;
use derive_more::Debug;
use device::AdbDevice;
use forensic_adb::{AndroidStorageInput, DeviceBrief, DeviceState};
use lazy_regex::{Lazy, Regex, lazy_regex};
use tokio::time;
use tokio_stream::StreamExt;
use tracing::{Instrument, debug, debug_span, error, instrument, trace, warn};

use crate::messages::{self as proto, AdbCommand, AdbResponse, DeviceChangedEvent};

pub mod device;

pub static PACKAGE_NAME_REGEX: Lazy<Regex> = lazy_regex!(r"^(?:[A-Za-z]{1}[\w]*\.)+[A-Za-z][\w]*$");

/// Handles ADB device connections and commands
#[derive(Debug)]
pub struct AdbHandler {
    /// The ADB host instance for device communication
    adb_host: forensic_adb::Host,
    /// Currently connected device (if any)
    device: ArcSwapOption<AdbDevice>,
}

impl AdbHandler {
    /// Creates a new AdbHandler instance and starts device monitoring.
    /// This is the main entry point for ADB functionality.
    ///
    /// # Returns
    /// Arc-wrapped AdbHandler that manages ADB device connections
    #[instrument]
    pub fn create() -> Arc<Self> {
        // TODO: check host and launch if not running
        let handle =
            Arc::new(Self { adb_host: forensic_adb::Host::default(), device: None.into() });
        Self::start_device_monitor(handle.clone());

        // Start command receiver
        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.receive_commands().await;
            }
        });

        // Start periodic device info refresh
        tokio::spawn({
            let handle = handle.clone();
            async move {
                handle.run_periodic_refresh().await;
            }
        });

        handle
    }

    /// Starts monitoring for device connection changes.
    /// Sets up the device tracking and update handling infrastructure.
    ///
    /// # Arguments
    /// * `adb_handler` - Reference to the AdbHandler instance to manage device updates
    #[instrument(level = "debug")]
    fn start_device_monitor(adb_handler: Arc<AdbHandler>) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

        // Spawn device state handler
        tokio::spawn(Self::handle_device_updates(adb_handler.clone(), receiver));

        // Spawn device tracking task
        tokio::spawn(Self::run_device_tracker(adb_handler.adb_host.clone(), sender));
    }

    /// Runs the device tracking loop that monitors for device connections and disconnections
    ///
    /// # Arguments
    /// * `adb_host` - The ADB host instance to track devices from
    /// * `sender` - Channel sender to communicate device updates
    #[instrument(level = "debug", err)]
    async fn run_device_tracker(
        adb_host: forensic_adb::Host,
        sender: tokio::sync::mpsc::UnboundedSender<DeviceBrief>,
    ) -> Result<()> {
        loop {
            ensure_server_running(&adb_host).await?;
            debug!("Starting track_devices loop");
            let stream = adb_host.track_devices();
            tokio::pin!(stream);
            let mut got_update = false;

            while let Some(device_result) = stream.next().await {
                match device_result {
                    Ok(device) => {
                        got_update = true;
                        sender.send(device).context("Failed to send track_devices update")?;
                    }
                    Err(e) => {
                        if got_update {
                            warn!(error = &e as &dyn Error, "Track_devices stream returned error");
                            break;
                        } else {
                            return Err(e).context("Failed to start track_devices stream");
                        }
                    }
                }
            }

            // Wait before retrying the tracking loop
            time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Handles device state updates received from the device tracker
    ///
    /// # Arguments
    /// * `adb_handler` - Reference to the AdbHandler instance
    /// * `receiver` - Channel receiver for device updates
    #[instrument(level = "debug", err)]
    async fn handle_device_updates(
        adb_handler: Arc<AdbHandler>,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<DeviceBrief>,
    ) -> Result<()> {
        while let Some(device_update) = receiver.recv().await {
            debug!(update = ?device_update, "Received device update");

            match (adb_handler.try_current_device(), &device_update.state) {
                // Current device went offline
                (Some(device), DeviceState::Offline) if device.serial == device_update.serial => {
                    debug!("Device is offline, disconnecting");
                    if let Err(e) = adb_handler.disconnect_device().await {
                        error!(error = e.as_ref() as &dyn Error, "Auto-disconnect failed");
                    }
                }
                // New device available
                (None, DeviceState::Device) => {
                    debug!("Auto-connecting to device");
                    if let Err(e) = adb_handler.connect_device().await {
                        error!(error = e.as_ref() as &dyn Error, "Auto-connect failed");
                    }
                }
                // TODO: handle other state combinations
                _ => {}
            }
        }

        bail!("Device update channel closed unexpectedly");
    }

    /// Listens for and processes ADB commands received from Dart
    #[instrument(level = "debug")]
    async fn receive_commands(&self) {
        let receiver = proto::AdbRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            match AdbCommand::try_from(request.message.command) {
                Ok(command) => {
                    if let Err(e) = self.execute_command(command, request.message.parameters).await
                    {
                        error!(error = e.as_ref() as &dyn Error, "ADB command execution failed");
                    }
                }
                Err(unknown_value) => {
                    error!(command = ?unknown_value, "Received invalid command from Dart");
                }
            }
        }
    }

    /// Executes a received ADB command with the given parameters
    ///
    /// # Arguments
    /// * `command` - The ADB command to execute
    /// * `parameters` - Optional parameters for the command
    ///
    /// # Returns
    /// Result indicating success or failure of the command execution
    #[instrument(level = "debug")]
    async fn execute_command(
        &self,
        command: AdbCommand,
        parameters: Option<proto::adb_request::Parameters>,
    ) -> Result<()> {
        fn send_response(command: AdbCommand, success: bool, message: String) {
            AdbResponse { command: command as i32, success, message }.send_signal_to_dart();
        }

        let mut device = (*self.current_device()?).clone();

        let result = match (command, parameters) {
            (
                AdbCommand::LaunchApp,
                Some(proto::adb_request::Parameters::PackageName(package_name)),
            ) => {
                let result = device.launch(&package_name).await;
                match result {
                    Ok(_) => {
                        send_response(
                            AdbCommand::LaunchApp,
                            true,
                            format!("Launched {}", package_name),
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to launch {}: {}", package_name, e);
                        send_response(AdbCommand::LaunchApp, false, error_msg.clone());
                        Err(e.context(format!("Failed to launch {}", package_name)))
                    }
                }
            }

            (
                AdbCommand::ForceStopApp,
                Some(proto::adb_request::Parameters::PackageName(package_name)),
            ) => {
                let result = device.force_stop(&package_name).await;
                match result {
                    Ok(_) => {
                        send_response(
                            AdbCommand::ForceStopApp,
                            true,
                            format!("Stopped {}", package_name),
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to force stop {}: {}", package_name, e);
                        send_response(AdbCommand::ForceStopApp, false, error_msg.clone());
                        Err(e.context(format!("Failed to force stop {}", package_name)))
                    }
                }
            }

            (AdbCommand::InstallApk, Some(proto::adb_request::Parameters::ApkPath(apk_path))) => {
                let result = device.install_apk(Path::new(&apk_path)).await;
                match result {
                    Ok(_) => {
                        self.set_device(Some(device), true);
                        send_response(
                            AdbCommand::InstallApk,
                            true,
                            format!("Installed {}", apk_path),
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to install {}: {}", apk_path, e);
                        send_response(AdbCommand::InstallApk, false, error_msg.clone());
                        Err(e.context(format!("Failed to install {}", apk_path)))
                    }
                }
            }

            (
                AdbCommand::UninstallPackage,
                Some(proto::adb_request::Parameters::PackageName(package_name)),
            ) => {
                let result = device.uninstall_package(&package_name).await;
                match result {
                    Ok(_) => {
                        self.set_device(Some(device), true);
                        send_response(
                            AdbCommand::UninstallPackage,
                            true,
                            format!("Uninstalled {}", package_name),
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to uninstall {}: {}", package_name, e);
                        send_response(AdbCommand::UninstallPackage, false, error_msg.clone());
                        Err(e.context(format!("Failed to uninstall {}", package_name)))
                    }
                }
            }

            (AdbCommand::SideloadApp, Some(proto::adb_request::Parameters::AppPath(app_path))) => {
                let result = device.sideload_app(Path::new(&app_path)).await;
                match result {
                    Ok(_) => {
                        self.set_device(Some(device), true);
                        send_response(
                            AdbCommand::SideloadApp,
                            true,
                            format!("Sideloaded {}", app_path),
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to sideload {}: {}", app_path, e);
                        send_response(AdbCommand::SideloadApp, false, error_msg.clone());
                        Err(e.context(format!("Failed to sideload {}", app_path)))
                    }
                }
            }

            (cmd, params) => {
                error!(command = ?cmd, parameters = ?params, "Invalid parameters for command");
                let error_msg = format!("Invalid parameters for command {:?}", cmd);
                send_response(cmd, false, error_msg.clone());
                bail!(error_msg)
            }
        };

        result.context("Command execution failed")
    }

    /// Updates the current device state and notifies Dart of the change
    ///
    /// # Arguments
    /// * `device` - Optional new device state
    /// * `update_current` - Whether to update the current device if it exists
    #[instrument(level = "debug")]
    fn set_device(&self, device: Option<AdbDevice>, update_current: bool) {
        if update_current {
            if let Some(current_device) = self.try_current_device() {
                if let Some(ref new_device) = device {
                    if current_device.serial != new_device.serial {
                        debug!("Ignoring device update for different device");
                        return;
                    }
                } else {
                    warn!("Attempted to update device when current device is None");
                    return;
                }
            }
        }

        let proto_device = device.clone().map(|d| d.into_proto());
        self.device.swap(device.map(Arc::new));
        DeviceChangedEvent { device: proto_device }.send_signal_to_dart();
    }

    /// Attempts to get the currently connected device
    ///
    /// # Returns
    /// Option containing the current device if one is connected
    #[instrument(level = "trace")]
    fn try_current_device(&self) -> Option<Arc<AdbDevice>> {
        self.device.load().as_ref().map(Arc::clone)
    }

    /// Gets the currently connected device or returns an error if none is connected
    ///
    /// # Returns
    /// Result containing the current device or an error if no device is connected
    #[instrument(level = "trace")]
    fn current_device(&self) -> Result<Arc<AdbDevice>> {
        self.try_current_device().context("No device connected")
    }

    /// Connects to an ADB device
    ///
    /// # Returns
    /// Result containing the connected AdbDevice instance or an error if connection fails
    #[instrument(err, ret)]
    async fn connect_device(&self) -> Result<AdbDevice> {
        // TODO: wait for device to be ready (boot_completed)
        let adb_host = self.adb_host.clone();
        let devices = adb_host
            .devices::<Vec<_>>()
            .await?
            .into_iter()
            .filter(|d| d.state == DeviceState::Device)
            .collect::<Vec<_>>();

        // TODO: handle multiple devices
        let first_device = devices.first().ok_or_else(|| anyhow::anyhow!("No device found"))?;

        let inner_device = forensic_adb::Device::new(
            adb_host,
            first_device.serial.clone(),
            first_device.info.clone(),
            AndroidStorageInput::default(),
        )
        .await
        .context("Failed to connect to device")?;

        let device = AdbDevice::new(inner_device).await?;

        self.set_device(Some(device.clone()), false);
        Ok(device)
    }

    /// Disconnects the current ADB device
    ///
    /// # Returns
    /// Result indicating success or failure of the disconnection
    #[instrument(err)]
    async fn disconnect_device(&self) -> Result<()> {
        ensure!(self.device.load().is_some(), "Already disconnected");
        self.set_device(None, false);
        Ok(())
    }

    /// Runs a periodic refresh of device information
    #[instrument]
    async fn run_periodic_refresh(&self) {
        let refresh_interval = Duration::from_secs(180); // 3 minutes
        let mut interval = time::interval(refresh_interval);

        loop {
            interval.tick().await;
            trace!("Device refresh tick");
            if let Some(device) = self.try_current_device() {
                let mut device = (*device).clone();
                async {
                    match device.refresh_all().await {
                        Ok(_) => self.set_device(Some(device), true),
                        Err(e) => error!(
                            error = e.as_ref() as &dyn Error,
                            "Periodic device refresh failed"
                        ),
                    }
                }
                .instrument(debug_span!("auto_refresh_run"))
                .await;
            }
        }
    }
}

/// Ensures the ADB server is running, starting it if necessary
///
/// # Arguments
/// * `host` - The ADB host instance to check
///
/// # Returns
/// Result indicating success or failure of ensuring server is running
#[instrument(err, level = "debug")]
async fn ensure_server_running(host: &forensic_adb::Host) -> Result<()> {
    if host.check_host_running().await.is_err() {
        debug!("Starting ADB server");
        // TODO: include adb binary
        host.start_server(None).await?;
    }
    Ok(())
}
