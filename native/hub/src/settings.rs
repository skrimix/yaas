use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal};
use tracing::{debug, error, info};

use crate::models::{Settings, signals::settings::*};

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

        // TODO: Add reset to defaults request
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
