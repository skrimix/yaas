use std::{error::Error, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use derive_more::Debug;
use device::AdbDevice;
use forensic_adb::{AndroidStorageInput, DeviceBrief, DeviceState};
use lazy_regex::{Lazy, Regex, lazy_regex};
use rinf::{DartSignal, RustSignal};
use tokio::{sync::RwLock, time};
use tokio_stream::StreamExt;
use tracing::{debug, error, trace, warn};

use crate::signals::adb::{command::*, device::DeviceChangedEvent};

pub mod device;

pub static PACKAGE_NAME_REGEX: Lazy<Regex> = lazy_regex!(r"^(?:[A-Za-z]{1}[\w]*\.)+[A-Za-z][\w]*$");

/// Handles ADB device connections and commands
#[derive(Debug)]
pub struct AdbHandler {
    /// The ADB host instance for device communication
    adb_host: forensic_adb::Host,
    /// Currently connected device (if any)
    device: RwLock<Option<Arc<AdbDevice>>>,
}

impl AdbHandler {
    /// Creates a new AdbHandler instance and starts device monitoring.
    /// This is the main entry point for ADB functionality.
    ///
    /// # Returns
    /// Arc-wrapped AdbHandler that manages ADB device connections
    // #[instrument]
    pub fn new() -> Arc<Self> {
        // TODO: check host and launch if not running
        let handle =
            Arc::new(Self { adb_host: forensic_adb::Host::default(), device: None.into() });
        Self::start_tasks(handle.clone());
        handle
    }

    /// Starts all background tasks needed for ADB functionality.
    /// This includes device monitoring, command handling, and periodic refreshes.
    ///
    /// # Arguments
    /// * `adb_handler` - Reference to the AdbHandler instance
    //  #[instrument(level = "debug")]
    fn start_tasks(adb_handler: Arc<AdbHandler>) {
        // Start device monitoring
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(Self::handle_device_updates(adb_handler.clone(), receiver));
        tokio::spawn(Self::run_device_tracker(adb_handler.adb_host.clone(), sender));

        // Start command receiver
        tokio::spawn({
            let handle = adb_handler.clone();
            async move {
                handle.receive_commands().await;
            }
        });

        // Start periodic device info refresh
        tokio::spawn({
            let handle = adb_handler.clone();
            async move {
                handle.run_periodic_refresh().await;
            }
        });
    }

    /// Runs the device tracking loop that monitors for device connections and disconnections
    ///
    /// # Arguments
    /// * `adb_host` - The ADB host instance to track devices from
    /// * `sender` - Channel sender to communicate device updates
    //  #[instrument(level = "debug", err)]
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
    //  #[instrument(level = "debug", err)]
    async fn handle_device_updates(
        adb_handler: Arc<AdbHandler>,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<DeviceBrief>,
    ) -> Result<()> {
        while let Some(device_update) = receiver.recv().await {
            debug!(update = ?device_update, "Received device update");

            match (adb_handler.try_current_device().await, &device_update.state) {
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
    //  #[instrument(level = "debug")]
    async fn receive_commands(&self) {
        let receiver = AdbRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            if let Err(e) = self.execute_command(request.message.command).await {
                error!(error = e.as_ref() as &dyn Error, "ADB command execution failed");
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
    //  #[instrument(level = "debug")]
    async fn execute_command(&self, command: AdbCommand) -> Result<()> {
        fn send_response(command: AdbCommand, success: bool, message: String) {
            AdbResponse { command, success, message }.send_signal_to_dart();
        }

        let device = self.current_device().await?;

        let result = match command.clone() {
            AdbCommand::LaunchApp(package_name) => {
                let result = device.launch(&package_name).await;
                match result {
                    Ok(_) => {
                        send_response(command, true, format!("Launched {}", package_name));
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to launch {}: {:#}", package_name, e);
                        send_response(command, false, error_msg);
                        Err(e.context(format!("Failed to launch {}", package_name)))
                    }
                }
            }

            AdbCommand::ForceStopApp(package_name) => {
                let result = device.force_stop(&package_name).await;
                match result {
                    Ok(_) => {
                        send_response(command, true, format!("Stopped {}", package_name));
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to force stop {}: {:#}", package_name, e);
                        send_response(command, false, error_msg);
                        Err(e.context(format!("Failed to force stop {}", package_name)))
                    }
                }
            }
        };

        result.context("Command execution failed")
    }

    /// Updates the current device state and notifies Dart of the change
    ///
    /// # Arguments
    /// * `device` - Optional new device state
    /// * `update_current` - Whether to update the current device if it exists
    //  #[instrument(level = "debug")]
    async fn set_device(&self, device: Option<AdbDevice>, update_current: bool) {
        fn report_device_change(device: &Option<AdbDevice>) {
            let proto_device = device.clone().map(|d| d.into());
            DeviceChangedEvent { device: proto_device }.send_signal_to_dart();
        }

        let mut current_device = self.device.write().await;

        if update_current {
            if device.is_none() {
                // TODO: should this be an error?
                warn!("Attempted to pass None as a device update");
                return;
            }

            if let (Some(current_device), Some(new_device)) = (current_device.as_ref(), &device) {
                if current_device.serial != new_device.serial {
                    debug!("Ignoring device update for different device");
                    return;
                }
            }
        }

        *current_device = device.clone().map(Arc::new);
        report_device_change(&device);
    }

    /// Attempts to get the currently connected device
    ///
    /// # Returns
    /// Option containing the current device if one is connected
    //  #[instrument(level = "trace")]
    async fn try_current_device(&self) -> Option<Arc<AdbDevice>> {
        self.device.read().await.as_ref().map(Arc::clone)
    }

    /// Gets the currently connected device or returns an error if none is connected
    ///
    /// # Returns
    /// Result containing the current device or an error if no device is connected
    //  #[instrument(level = "trace")]
    async fn current_device(&self) -> Result<Arc<AdbDevice>> {
        self.try_current_device().await.context("No device connected")
    }

    /// Connects to an ADB device
    ///
    /// # Returns
    /// Result containing the connected AdbDevice instance or an error if connection fails
    //  #[instrument(err, ret)]
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
        let first_device = devices.first().context("No device found")?;

        let inner_device = forensic_adb::Device::new(
            adb_host,
            first_device.serial.clone(),
            first_device.info.clone(),
            AndroidStorageInput::default(),
        )
        .await
        .context("Failed to connect to device")?;

        let device = AdbDevice::new(inner_device).await?;

        self.set_device(Some(device.clone()), false).await;
        Ok(device)
    }

    /// Disconnects the current ADB device
    ///
    /// # Returns
    /// Result indicating success or failure of the disconnection
    //  #[instrument(err)]
    async fn disconnect_device(&self) -> Result<()> {
        ensure!(
            self.device.read().await.is_some(),
            "Cannot disconnect from a device when none is connected"
        );
        self.set_device(None, false).await;
        Ok(())
    }

    /// Runs a periodic refresh of device information
    //  #[instrument]
    async fn run_periodic_refresh(&self) {
        let refresh_interval = Duration::from_secs(60);
        let mut interval = time::interval(refresh_interval);

        loop {
            interval.tick().await;
            trace!("Device refresh tick");
            if self.try_current_device().await.is_some() {
                async {
                    let _ = self.refresh_device().await.inspect_err(|e| {
                        error!(error = e.as_ref() as &dyn Error, "Periodic device refresh failed");
                    });
                }
                // .instrument(debug_span!("auto_refresh_run"))
                .await;
            }
        }
    }

    /// Refreshes the currently connected device
    pub async fn refresh_device(&self) -> Result<()> {
        let device = self.current_device().await?;
        let mut device = (*device).clone();
        device.refresh().await?;
        self.set_device(Some(device), true).await;
        Ok(())
    }

    /// Installs an APK on the currently connected device
    ///
    /// # Arguments
    /// * `apk_path` - Path to the APK file to install
    pub async fn install_apk(&self, apk_path: &std::path::Path) -> Result<()> {
        let device = self.current_device().await?;
        let device = (*device).clone();
        let result = device.install_apk(apk_path).await;
        self.refresh_device().await?;
        result
    }

    /// Uninstalls a package from the currently connected device
    ///
    /// # Arguments
    /// * `package_name` - The package name to uninstall
    pub async fn uninstall_package(&self, package_name: &str) -> Result<()> {
        let device = self.current_device().await?;
        let device = (*device).clone();
        let result = device.uninstall_package(package_name).await;
        self.refresh_device().await?;
        result
    }

    /// Sideloads an app by installing its APK and pushing OBB data if present
    ///
    /// # Arguments
    /// * `app_path` - Path to directory containing the app files
    pub async fn sideload_app(&self, app_path: &std::path::Path) -> Result<()> {
        let device = self.current_device().await?;
        let device = (*device).clone();
        let result = device.sideload_app(app_path).await;
        self.refresh_device().await?;
        result
    }
}

/// Ensures the ADB server is running, starting it if necessary
///
/// # Arguments
/// * `host` - The ADB host instance to check
///
/// # Returns
/// Result indicating success or failure of ensuring server is running
// #[instrument(err, level = "debug")]
async fn ensure_server_running(host: &forensic_adb::Host) -> Result<()> {
    if host.check_host_running().await.is_err() {
        debug!("Starting ADB server");
        // TODO: include adb binary
        host.start_server(None).await?;
    }
    Ok(())
}
