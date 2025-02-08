use std::{error::Error, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use arc_swap::ArcSwapOption;
use derive_more::Debug;
use device::AdbDevice;
use forensic_adb::{AndroidStorageInput, DeviceState};
use tokio::time;
use tokio_stream::StreamExt;
use tracing::{debug, error, instrument, warn};

use crate::messages::DeviceChangedEvent;

pub mod device;

#[derive(Debug)]
pub enum AdbEvent {
    DeviceChanged(Option<Arc<AdbDevice>>),
}

#[derive(Debug)]
pub struct AdbHandler {
    adb_host: forensic_adb::Host,
    device: ArcSwapOption<AdbDevice>,
}

impl Default for AdbHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl AdbHandler {
    pub fn new() -> Self {
        // TODO: check host and launch if not running
        Self { adb_host: forensic_adb::Host::default(), device: None.into() }
    }

    #[instrument(level = "debug")]
    pub async fn start_device_monitor(adb_handler: Arc<AdbHandler>) {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        // Spawn the device tracking task
        tokio::spawn({
            let sender = sender.clone();
            let adb_host = adb_handler.adb_host.clone();
            async move {
                ensure_server_running(&adb_host).await.expect("adb server failed to start");
                loop {
                    let mut got_update = false;
                    debug!("starting track_devices loop");
                    let stream = adb_host.track_devices();
                    tokio::pin!(stream);
                    while let Some(device_result) = stream.next().await {
                        match device_result {
                            Ok(device) => {
                                got_update = true;
                                sender.send(device).expect("failed to send track_devices update");
                            }
                            Err(e) => {
                                if got_update {
                                    warn!(
                                        error = &e as &dyn Error,
                                        "track_devices stream returned error"
                                    );
                                    break;
                                } else {
                                    error!(
                                        error = &e as &dyn Error,
                                        "failed to start track_devices stream"
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        // Handle device updates
        tokio::spawn({
            let adb_handler = adb_handler.clone();
            async move {
                while let Some(device_update) = receiver.recv().await {
                    debug!(update = ?device_update, "received device update");
                    // TODO: match other states
                    if let Some(device) = adb_handler.try_current_device() {
                        if device_update.state == DeviceState::Offline
                            && device_update.serial == device.serial
                        {
                            debug!("device is offline, disconnecting");
                            if let Err(e) = adb_handler.disconnect_device().await {
                                error!(error = e.as_ref() as &dyn Error, "auto-disconnect failed");
                            }
                        }
                    } else if device_update.state == DeviceState::Device {
                        debug!("auto-connecting to device");
                        if let Err(e) = adb_handler.connect_device().await {
                            error!(error = e.as_ref() as &dyn Error, "auto-connect failed");
                        }
                    };
                }
            }
        });
    }

    #[instrument(level = "debug")]
    fn set_device(&self, device: Option<AdbDevice>, update_current: bool) {
        if update_current {
            if let Some(current_device) = self.try_current_device() {
                if let Some(ref new_device) = device {
                    if current_device.serial != new_device.serial {
                        debug!("ignoring device update for different device");
                        return;
                    }
                } else {
                    warn!("attempted to update device when current device is None");
                    return;
                }
            }
        }

        let proto_device = device.clone().map(|d| d.into_proto());
        self.device.swap(device.map(Arc::new));
        DeviceChangedEvent { device: proto_device }.send_signal_to_dart();
    }

    #[instrument(level = "trace")]
    fn try_current_device(&self) -> Option<Arc<AdbDevice>> {
        self.device.load().as_ref().map(Arc::clone)
    }

    #[instrument(level = "trace")]
    fn current_device(&self) -> Result<Arc<AdbDevice>> {
        self.try_current_device().context("no device connected")
    }

    #[instrument(err, ret)]
    async fn connect_device(&self) -> Result<AdbDevice> {
        // TODO: wait for device to be ready (boot_completed)
        let device = AdbDevice::new(
            self.adb_host
                .clone()
                .device_or_default(Option::<&String>::None, AndroidStorageInput::default())
                .await
                .context("failed to connect to device")?,
        )
        .await?;
        self.set_device(Some(device.clone()), false);
        Ok(device)
    }

    #[instrument(err)]
    async fn disconnect_device(&self) -> Result<()> {
        ensure!(self.device.load().is_some(), "already disconnected");
        self.set_device(None, false);
        // TODO: on_device_disconnected
        Ok(())
    }
}

#[instrument(err, level = "debug")]
async fn ensure_server_running(host: &forensic_adb::Host) -> Result<()> {
    if host.check_host_running().await.is_err() {
        debug!("starting adb server");
        host.start_server(None).await?;
    }
    Ok(())
}
