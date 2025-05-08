use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::signals::settings::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub enum ConnectionType {
    Usb,
    Wireless,
}

impl Default for ConnectionType {
    fn default() -> Self {
        Self::Usb
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub enum DownloadCleanupPolicy {
    DeleteAfterInstall,
    KeepOneVersion,
    KeepTwoVersions,
    KeepAllVersions,
}

impl Default for DownloadCleanupPolicy {
    fn default() -> Self {
        Self::DeleteAfterInstall
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub struct Settings {
    pub rclone_path: String,
    pub rclone_remote_name: String,
    pub adb_path: String,
    pub preferred_connection_type: ConnectionType,
    pub downloads_location: String,
    pub backups_location: String,
    pub bandwidth_limit: String,
    pub cleanup_policy: DownloadCleanupPolicy,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rclone_path: "rclone".to_string(),
            rclone_remote_name: "".to_string(),
            adb_path: "adb".to_string(),
            preferred_connection_type: ConnectionType::default(),
            downloads_location: dirs::download_dir()
                .expect("Failed to get download directory")
                .join("RQL")
                .to_string_lossy()
                .to_string(),
            backups_location: dirs::document_dir()
                .expect("Failed to get document directory")
                .join("RQL_backups")
                .to_string_lossy()
                .to_string(),
            bandwidth_limit: "".to_string(),
            cleanup_policy: DownloadCleanupPolicy::default(),
        }
    }
}

/// Handles application settings
#[derive(Debug, Clone)]
pub struct SettingsHandler {
    settings_file_path: PathBuf,
}

impl SettingsHandler {
    pub fn new(app_dir: PathBuf) -> Arc<Self> {
        let handler = Arc::new(Self { settings_file_path: app_dir.join("settings.json") });

        // Start receiving settings requests
        tokio::spawn({
            let handler = handler.clone();
            async move {
                handler.receive_settings_requests().await;
            }
        });

        handler
    }

    async fn receive_settings_requests(&self) {
        let load_receiver = LoadSettingsRequest::get_dart_signal_receiver();
        let save_receiver = SaveSettingsRequest::get_dart_signal_receiver();

        // Handle load requests
        let handler_clone = self.clone();
        tokio::spawn(async move {
            while load_receiver.recv().await.is_some() {
                let handler = handler_clone.clone();
                let result = handler.load_settings();

                match result {
                    Ok(settings) => {
                        SettingsLoadedEvent { settings, error: None }.send_signal_to_dart();
                    }
                    Err(e) => {
                        error!("Failed to load settings: {}", e);
                        SettingsLoadedEvent {
                            settings: Settings::default(),
                            error: Some(format!("Failed to load settings: {:#}", e)),
                        }
                        .send_signal_to_dart();
                    }
                }
            }
        });

        // Handle save requests
        let handler_clone = self.clone();
        tokio::spawn(async move {
            while let Some(signal_pack) = save_receiver.recv().await {
                let handler = handler_clone.clone();
                let result = handler.save_settings(&signal_pack.message.settings);

                match result {
                    Ok(_) => {
                        SettingsSavedEvent { error: None }.send_signal_to_dart();
                    }
                    Err(e) => {
                        error!("Failed to save settings: {}", e);
                        SettingsSavedEvent {
                            error: Some(format!("Failed to save settings: {:#}", e)),
                        }
                        .send_signal_to_dart();
                    }
                }
            }
        });
    }

    /// Load settings from file or return defaults if file doesn't exist
    fn load_settings(&self) -> Result<Settings> {
        if !self.settings_file_path.exists() {
            info!("Settings file doesn't exist, using defaults");
            return self.load_default_settings().context("Failed to load default settings");
        }

        let file_content =
            fs::read_to_string(&self.settings_file_path).context("Failed to read settings file")?;

        let settings: Settings =
            serde_json::from_str(&file_content).context("Failed to parse settings file")?;

        // TODO: Validate settings

        debug!("Settings loaded successfully");
        Ok(settings)
    }

    /// Save settings to file
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let settings_json =
            serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;

        // Ensure parent directory exists
        if let Some(parent) = self.settings_file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).context("Failed to create settings directory")?;
            }
        }

        // TODO: Validate settings

        fs::write(&self.settings_file_path, settings_json)
            .context("Failed to write settings file")?;

        debug!("Settings saved successfully");
        Ok(())
    }

    /// Load default settings
    pub fn load_default_settings(&self) -> Result<Settings> {
        let settings = Settings::default();

        // Create default directories if they don't exist (and parents do)
        let downloads_parent = Path::new(&settings.downloads_location)
            .parent()
            .context("Failed to get downloads directory parent")?;
        let backups_parent = Path::new(&settings.backups_location)
            .parent()
            .context("Failed to get backups directory parent")?;
        ensure!(
            downloads_parent.exists(),
            format!(
                "Downloads directory parent ({}) does not exist",
                downloads_parent.to_string_lossy()
            )
        );
        ensure!(
            backups_parent.exists(),
            format!(
                "Backups directory parent ({}) does not exist",
                backups_parent.to_string_lossy()
            )
        );
        fs::create_dir_all(&settings.downloads_location)
            .context("Failed to create downloads directory")?;
        fs::create_dir_all(&settings.backups_location)
            .context("Failed to create backups directory")?;

        self.save_settings(&settings)?;
        Ok(settings)
    }
}
