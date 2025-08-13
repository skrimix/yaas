use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal};
use tokio::sync::watch;
use tracing::{debug, error, info, instrument, trace, warn};

use crate::models::{Settings, signals::settings::*};

/// Handles application settings
#[derive(Debug, Clone)]
pub struct SettingsHandler {
    settings_file_path: PathBuf,
    watch_tx: watch::Sender<Settings>,
}

impl SettingsHandler {
    #[instrument(skip(app_dir))]
    pub fn new(app_dir: PathBuf) -> Arc<Self> {
        let watch_tx = watch::Sender::<Settings>::new(Settings::default());
        let handler =
            Arc::new(Self { settings_file_path: app_dir.join("settings.json"), watch_tx });

        let settings = match handler.load_settings() {
            Ok(s) => s,
            Err(e) => {
                warn!(error = e.as_ref() as &dyn Error, "Failed to load settings, using defaults.");
                handler.load_default_settings().expect("Failed to load default settings")
            }
        };
        handler.on_settings_change(settings, None);

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

        info!("Starting to listen for settings requests");

        loop {
            tokio::select! {
                Some(_) = load_receiver.recv() => {
                    info!("Received LoadSettingsRequest");
                    let handler = self.clone();
                    let result = handler.load_settings();

                    match result {
                        Ok(settings) => {
                            handler.on_settings_change(settings.clone(), None);
                        }
                        Err(e) => {
                            error!(error = e.as_ref() as &dyn Error, "Failed to load settings, using defaults");
                            let settings = handler
                                .load_default_settings()
                                .expect("Failed to load default settings"); // TODO: handle error?
                            handler.on_settings_change(
                                settings.clone(),
                                Some(format!("Failed to load settings: {e:#}")),
                            );
                        }
                    }
                },
                Some(signal_pack) = save_receiver.recv() => {
                    info!("Received SaveSettingsRequest");
                    let handler = self.clone();
                    let settings = signal_pack.message.settings;
                    let result = handler.save_settings(&settings);

                    match result {
                        Ok(_) => {
                            handler.on_settings_change(settings.clone(), None);
                        }
                        Err(e) => {
                            error!(error = e.as_ref() as &dyn Error, "Failed to save settings");
                            SettingsSavedEvent {
                                error: Some(format!("Failed to save settings: {e:#}")),
                            }
                            .send_signal_to_dart();
                        }
                    }
                },
                else => {
                    error!("All settings request channels closed");
                    break;
                }
            }
        }
        panic!("Settings request receiver loop ended");
        // TODO: Add reset to defaults request
    }

    /// Handle settings change
    ///
    /// # Arguments
    ///
    /// * `settings` - The new settings
    /// * `error` - An optional error message for the UI
    #[instrument(skip(self, settings, error))]
    fn on_settings_change(&self, settings: Settings, error: Option<String>) {
        trace!("on_settings_change called");
        self.watch_tx.send_if_modified(|s| {
            if s != &settings {
                debug!(settings = ?settings, "Active settings changed");
                *s = settings.clone();
                SettingsChangedEvent { settings, error }.send_signal_to_dart();
                true
            } else {
                trace!("Settings unchanged, not sending event");
                false
            }
        });
    }

    /// Create a receiver for settings changes
    pub fn subscribe(&self) -> watch::Receiver<Settings> {
        self.watch_tx.subscribe()
    }

    /// Load settings from file or return defaults if file doesn't exist
    #[instrument(skip(self))]
    fn load_settings(&self) -> Result<Settings> {
        if !self.settings_file_path.exists() {
            info!(path = %self.settings_file_path.display(), "Settings file doesn't exist, using defaults");
            return self.load_default_settings().context("Failed to load default settings");
        }

        info!(path = %self.settings_file_path.display(), "Loading settings from file");
        let file_content =
            fs::read_to_string(&self.settings_file_path).context("Failed to read settings file")?;

        let settings: Settings =
            serde_json::from_str(&file_content).context("Failed to parse settings file")?;

        // TODO: Validate settings

        debug!("Loaded application settings successfully");
        Ok(settings)
    }

    /// Save settings to file
    #[instrument(skip(self, settings))]
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        info!(path = %self.settings_file_path.display(), "Saving settings to file");
        let settings_json =
            serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;

        // Ensure parent directory exists
        if let Some(parent) = self.settings_file_path.parent()
            && !parent.exists()
        {
                info!(path = %parent.display(), "Creating settings directory");
                fs::create_dir_all(parent).context("Failed to create settings directory")?;
            }

        // TODO: Validate settings

        fs::write(&self.settings_file_path, settings_json)
            .context("Failed to write settings file")?;

        info!("Saved application settings successfully");
        Ok(())
    }

    /// Load default settings
    #[instrument(skip(self))]
    pub fn load_default_settings(&self) -> Result<Settings> {
        info!("Loading default settings");
        let settings = Settings::default();

        // Create default directories if they don't exist (and parents do)
        debug!(path = %settings.downloads_location, "Ensuring downloads directory exists");
        let downloads_parent = Path::new(&settings.downloads_location)
            .parent()
            .context("Failed to get downloads directory parent")?;
        debug!(path = %settings.backups_location, "Ensuring backups directory exists");
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
        info!("Default settings loaded and saved");
        Ok(settings)
    }
}
