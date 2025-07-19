use std::{error::Error, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use derive_more::Debug;
use device::AdbDevice;
use forensic_adb::{AndroidStorageInput, DeviceBrief, DeviceState};
use lazy_regex::{Lazy, Regex, lazy_regex};
use rinf::{DartSignal, RustSignal};
use tokio::{
    sync::{Mutex, RwLock, mpsc::UnboundedSender},
    time::{self, timeout},
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::{
    adb::device::SideloadProgress,
    models::{
        AdbState, Settings,
        signals::{
            adb::{command::*, device::DeviceChangedEvent},
            system::Toast,
        },
    },
};

pub mod device;

pub static PACKAGE_NAME_REGEX: Lazy<Regex> = lazy_regex!(r"^(?:[A-Za-z]{1}[\w]*\.)+[A-Za-z][\w]*$");

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
    // #[instrument]
    pub async fn new(mut settings_stream: WatchStream<Settings>) -> Arc<Self> {
        let adb_path =
            settings_stream.next().await.expect("Settings stream closed on adb init").adb_path;
        let adb_path = if adb_path.is_empty() { None } else { Some(adb_path) };
        let handle = Arc::new(Self {
            adb_host: forensic_adb::Host::default(),
            adb_server_mutex: Mutex::new(()),
            adb_path: RwLock::new(adb_path),
            adb_state: RwLock::new(AdbState::default()),
            device: None.into(),
            cancel_token: RwLock::new(CancellationToken::new()),
        });
        tokio::spawn({
            let handle = handle.clone();
            async move {
                if let Err(e) = handle.ensure_server_running().await {
                    panic!("Failed to start ADB server: {e}")
                }
            } // TODO: handle error
        });
        tokio::spawn(handle.clone().start_tasks(settings_stream));
        handle
    }

    /// Starts all background tasks needed for ADB functionality.
    /// This includes device monitoring, command handling, and periodic refreshes.
    ///
    /// # Arguments
    /// * `settings_stream` - WatchStream for application settings updates
    //  #[instrument(level = "debug")]
    async fn start_tasks(self: Arc<AdbHandler>, mut settings_stream: WatchStream<Settings>) {
        // Handle settings updates
        tokio::spawn({
            let handle = self.clone();
            async move {
                while let Some(settings) = settings_stream.next().await {
                    let new_adb_path = settings.adb_path.clone();
                    let new_adb_path =
                        if new_adb_path.is_empty() { None } else { Some(new_adb_path) };
                    if new_adb_path != *handle.adb_path.read().await {
                        *handle.adb_path.write().await = new_adb_path;
                        handle.clone().restart_adb().await.expect("Failed to restart ADB"); // TODO: handle error
                    }
                }
            }
        });

        self.start_adb_tasks().await;
    }

    /// Starts the ADB tasks
    async fn start_adb_tasks(self: Arc<AdbHandler>) {
        let cancel_token = self.cancel_token.read().await.clone();

        // Listen for ADB device updates
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn({
            let cancel_token = cancel_token.clone();
            let handler = self.clone();
            async move {
                cancel_token.run_until_cancelled(handler.handle_device_updates(receiver)).await
            }
        });

        // Track ADB device changes
        tokio::spawn({
            let cancel_token = cancel_token.clone();
            let handler = self.clone();
            async move { cancel_token.run_until_cancelled(handler.run_device_tracker(sender)).await }
        });

        // Listen for commands
        tokio::spawn({
            let handle = self.clone();
            async move {
                cancel_token.run_until_cancelled(handle.receive_commands()).await;
            }
        });

        // Refresh device info periodically
        tokio::spawn({
            let handle = self.clone();
            let cancel_token = self.cancel_token.read().await.clone();
            async move {
                cancel_token.run_until_cancelled(handle.run_periodic_refresh()).await;
            }
        });
    }

    /// Restarts the ADB handling
    async fn restart_adb(self: Arc<AdbHandler>) -> Result<()> {
        debug!("Restarting ADB server and tasks");
        // Cancel all tasks
        self.cancel_token.write().await.cancel();
        // Disconnect from device
        let _ = self.disconnect_device().await;
        // Kill ADB server
        let _ = self.kill_adb_server().await;
        // Restart ADB server
        self.ensure_server_running().await?;
        // Restart tasks
        *self.cancel_token.write().await = CancellationToken::new();
        tokio::spawn(self.clone().start_adb_tasks());
        Ok(())
    }

    /// Kills the ADB server
    async fn kill_adb_server(&self) -> Result<()> {
        let adb_path = self.adb_path.read().await.clone();
        if let Err(e) = self.adb_host.kill_server(adb_path.as_deref()).await {
            warn!(error = &e as &dyn Error, "Failed to kill ADB server");
            // TODO: kill process?
        }
        self.refresh_adb_state().await;
        Ok(())
    }

    /// Runs the device tracking loop that monitors for device connections and disconnections
    ///
    /// # Arguments
    /// * `sender` - Channel sender to communicate device updates
    //  #[instrument(level = "debug", err)]
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
                        sender.send(device).context("Failed to send track_devices update")?;
                    }
                    Err(e) => {
                        if got_update {
                            // The stream worked, but encountered an error
                            warn!(
                                error = &e as &dyn Error,
                                "track_devices stream returned an unexpected error and will \
                                 attempt to restart"
                            );
                            // Server might have died
                            self.refresh_adb_state().await;
                            // FIXME: device updates stop after this
                            break;
                        } else {
                            // The stream closed immediately
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
    /// * `receiver` - Channel receiver for device updates
    //  #[instrument(level = "debug", err)]
    async fn handle_device_updates(
        self: Arc<AdbHandler>,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<DeviceBrief>,
    ) -> Result<()> {
        while let Some(device_update) = receiver.recv().await {
            debug!(update = ?device_update, "Received device update");

            match (self.try_current_device().await, &device_update.state) {
                // Current device went offline
                (Some(device), DeviceState::Offline) if device.serial == device_update.serial => {
                    info!("Device is offline, disconnecting");
                    if let Err(e) = self.disconnect_device().await {
                        error!(error = e.as_ref() as &dyn Error, "Auto-disconnect failed");
                    }
                }
                // New device available
                (None, DeviceState::Device) => {
                    info!("Auto-connecting to device");
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
                _ => {}
            }

            self.refresh_adb_state().await;
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
        error!("ADB command receiver channel closed");
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
        fn send_toast(title: String, description: String, error: bool, duration: Option<Duration>) {
            Toast::send(title, description, error, duration);
        }

        let device = self.current_device().await?;

        let result = match command.clone() {
            AdbCommand::LaunchApp(package_name) => {
                let result = device.launch(&package_name).await;
                match result {
                    Ok(_) => {
                        send_toast(
                            "App Launched".to_string(),
                            format!("Launched {package_name}"),
                            false,
                            None,
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to launch {package_name}: {e:#}");
                        send_toast("Launch Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to launch {package_name}")))
                    }
                }
            }

            AdbCommand::ForceStopApp(package_name) => {
                let result = device.force_stop(&package_name).await;
                match result {
                    Ok(_) => {
                        send_toast(
                            "App Stopped".to_string(),
                            format!("Stopped {package_name}"),
                            false,
                            None,
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to force stop {package_name}: {e:#}");
                        send_toast("Stop Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to force stop {package_name}")))
                    }
                }
            }

            AdbCommand::UninstallPackage(package_name) => {
                let result = self.uninstall_package(&package_name).await;
                match result {
                    Ok(_) => {
                        send_toast(
                            "App Uninstalled".to_string(),
                            format!("Uninstalled {package_name}"),
                            false,
                            None,
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to uninstall {package_name}: {e:#}");
                        send_toast("Uninstall Failed".to_string(), error_msg, true, None);
                        Err(e.context(format!("Failed to uninstall {package_name}")))
                    }
                }
            }

            AdbCommand::RefreshDevice => {
                let result = self.refresh_device().await;
                match result {
                    Ok(_) => {
                        send_toast(
                            "Refresh".to_string(),
                            "Device data refreshed".to_string(),
                            false,
                            Some(Duration::from_secs(2)),
                        );
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to refresh device: {e:#}");
                        send_toast("Refresh Failed".to_string(), error_msg, true, None);
                        Err(e.context("Failed to refresh device"))
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
        self.device.read().await.as_ref().map(Arc::clone) // TODO: ping device to check if it's still connected?
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
        self.refresh_adb_state().await;
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
        self.refresh_adb_state().await;
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
    pub async fn install_apk(
        &self,
        apk_path: &std::path::Path,
        progress_sender: UnboundedSender<f32>,
    ) -> Result<()> {
        let device = self.current_device().await?;
        let device = (*device).clone();
        device.install_apk_with_progress(apk_path, progress_sender).await?;
        self.refresh_device().await?;
        Ok(())
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
    /// * `progress_sender` - Sender for progress updates
    pub async fn sideload_app(
        &self,
        app_path: &std::path::Path,
        progress_sender: UnboundedSender<SideloadProgress>,
    ) -> Result<()> {
        let device = self.current_device().await?;
        let device = (*device).clone();
        let result = device.sideload_app(app_path, progress_sender).await;
        self.refresh_device().await?;
        result
    }

    /// Ensures the ADB server is running, starting it if necessary
    ///
    /// # Arguments
    /// * `host` - The ADB host instance to check
    ///
    /// # Returns
    /// Result indicating success or failure of ensuring server is running
    // #[instrument(err, level = "debug")]
    async fn ensure_server_running(&self) -> Result<()> {
        let _guard = self.adb_server_mutex.lock().await;
        if !self.is_server_running().await {
            debug!("Starting ADB server");
            let adb_path_buf: PathBuf = self
                .adb_path
                .read()
                .await
                .clone()
                .map(which::which)
                .transpose()
                .unwrap_or_else(|e| {
                    warn!(
                        error = &e as &dyn Error,
                        "Failed to resolve ADB path from settings, trying default"
                    );
                    which::which("adb").ok()
                })
                .expect("Failed to resolve ADB path"); // TODO: handle error
            self.adb_host
                .start_server(adb_path_buf.to_str())
                .await
                .expect("Failed to start ADB server"); // TODO: handle error
            self.refresh_adb_state().await;
        }
        Ok(())
    }

    /// Checks if the ADB server is running
    async fn is_server_running(&self) -> bool {
        timeout(Duration::from_millis(500), self.adb_host.check_host_running()).await.is_ok()
    }

    /// Gets the ADB devices
    async fn get_adb_devices(&self) -> Result<Vec<DeviceBrief>> {
        let adb_host = self.adb_host.clone();
        let devices = adb_host.devices::<Vec<_>>().await?.into_iter().map(|d| d.into()).collect();
        Ok(devices)
    }

    /// Refreshes the ADB state based on the current device and server status
    async fn refresh_adb_state(&self) {
        let mut adb_state = self.adb_state.write().await;
        let new_state = if !self.is_server_running().await {
            AdbState::ServerNotRunning
        } else if self.try_current_device().await.is_some() {
            AdbState::DeviceConnected
        } else {
            let devices: Vec<DeviceBrief> = self.get_adb_devices().await.unwrap_or_else(|_| {
                error!("Failed to get ADB devices");
                vec![]
            });
            if devices.is_empty() {
                AdbState::NoDevices
            } else if devices.iter().all(|d| d.state == DeviceState::Unauthorized) {
                AdbState::DeviceUnauthorized
            } else {
                let device_serials = devices.iter().map(|d| d.serial.clone()).collect();
                AdbState::DevicesAvailable(device_serials)
            }
        };

        // Update state and send signal
        *adb_state = new_state.clone();
        new_state.send_signal_to_dart();
    }
}
