use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal};
use tokio::sync::watch;
use tracing::{debug, error, info, instrument, trace, warn};

use crate::models::{Settings, signals::settings::*};

/// Handles application settings
#[derive(Debug, Clone)]
pub(crate) struct SettingsHandler {
    settings_file_path: PathBuf,
    watch_tx: watch::Sender<Settings>,
    write_guard: Arc<Mutex<()>>,
}

impl SettingsHandler {
    #[instrument(level = "debug", skip(app_dir))]
    pub(crate) fn new(app_dir: PathBuf, portable_mode: bool) -> Result<Arc<Self>> {
        ensure!(app_dir.is_dir(), "App directory is not a directory or does not exist");
        ensure!(app_dir.is_absolute(), "App directory is not absolute");

        let watch_tx = watch::Sender::<Settings>::new(Settings::new(portable_mode));
        let handler = Arc::new(Self {
            settings_file_path: app_dir.join("settings.json"),
            watch_tx,
            write_guard: Arc::new(Mutex::new(())),
        });

        match handler.load_settings(portable_mode) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = e.as_ref() as &dyn Error, "Failed to load settings, using defaults.");
                handler
                    .load_default_settings(None, portable_mode)
                    .context("Failed to load default settings")?
            }
        };

        // Start receiving settings requests
        tokio::spawn({
            let handler = handler.clone();
            async move {
                handler.receive_settings_requests(portable_mode).await;
            }
        });

        Ok(handler)
    }

    #[instrument(level = "debug", skip(self))]
    async fn receive_settings_requests(&self, portable_mode: bool) {
        let load_receiver = LoadSettingsRequest::get_dart_signal_receiver();
        let save_receiver = SaveSettingsRequest::get_dart_signal_receiver();
        let reset_receiver = ResetSettingsToDefaultsRequest::get_dart_signal_receiver();

        debug!("Starting to listen for settings requests");

        loop {
            tokio::select! {
                request = load_receiver.recv() => {
                    if request.is_some() {
                        debug!("Received LoadSettingsRequest");
                        let handler = self.clone();
                        let result = handler.load_settings(portable_mode);

                        if let Err(e) = result {
                            error!(error = e.as_ref() as &dyn Error, "Failed to load settings, using defaults");
                                let settings = handler
                                    .load_default_settings(None, portable_mode)
                                    .expect("Failed to load default settings"); // TODO: handle error?
                                handler.on_settings_change(
                                    settings.clone(),
                                    Some(format!("Failed to load settings: {e:#}")),
                                    true,
                                );
                        }
                    } else {
                        panic!("LoadSettingsRequest receiver closed");
                    }
                }
                request = save_receiver.recv() => {
                    if let Some(request) = request {
                        debug!("Received SaveSettingsRequest");
                        let handler = self.clone();
                        let settings = request.message.settings;
                        let result = handler.save_settings(&settings);

                        if let Err(e) = result {
                            error!(error = e.as_ref() as &dyn Error, "Failed to save settings");
                            SettingsSavedEvent {
                                error: Some(format!("Failed to save settings: {e:#}")),
                            }
                            .send_signal_to_dart();
                        }
                    } else {
                        panic!("SaveSettingsRequest receiver closed");
                    }
                }
                request = reset_receiver.recv() => {
                    if request.is_some() {
                        debug!("Received ResetSettingsToDefaultsRequest");
                        let handler = self.clone();
                        let installation_id = handler.watch_tx.borrow().installation_id.clone();
                        let result = handler.load_default_settings(Some(installation_id), portable_mode);

                        match result {
                            Ok(settings) => {
                                // Force notify to ensure UI updates even if values match
                                handler.on_settings_change(settings.clone(), None, true);
                            }
                            Err(e) => {
                                error!(error = e.as_ref() as &dyn Error, "Failed to reset settings to defaults");
                                handler.on_settings_change(
                                    handler.watch_tx.borrow().clone(),
                                    Some(format!("Failed to reset to defaults: {e:#}")),
                                    true,
                                );
                            }
                        }
                    } else {
                        panic!("ResetSettingsToDefaultsRequest receiver closed");
                    }
                }
            }
        }
    }

    /// Handle settings change
    ///
    /// # Arguments
    ///
    /// * `settings` - The new settings
    /// * `error` - An optional error message for the UI
    /// * `force_notify` - If true, always send event to Dart even if unchanged
    #[instrument(level = "debug", skip(self, settings, error))]
    fn on_settings_change(&self, settings: Settings, error: Option<String>, force_notify: bool) {
        trace!("on_settings_change called");

        let mut changed = false;
        self.watch_tx.send_if_modified(|s| {
            if s != &settings {
                debug!(settings = ?settings, "Active settings changed");
                *s = settings.clone();
                changed = true;
                true
            } else {
                false
            }
        });

        if changed || force_notify {
            debug!(changed = changed, force_notify = force_notify, "Sending settings to Dart");
            SettingsChangedEvent { settings, error }.send_signal_to_dart();
        } else {
            trace!("Settings unchanged, not sending event");
        }
    }

    /// Create a receiver for settings changes
    pub(crate) fn subscribe(&self) -> watch::Receiver<Settings> {
        self.watch_tx.subscribe()
    }

    /// Load settings from file or return defaults if file doesn't exist
    #[instrument(level = "debug", skip(self))]
    fn load_settings(&self, portable_mode: bool) -> Result<Settings> {
        if !self.settings_file_path.exists() {
            info!(path = %self.settings_file_path.display(), "Settings file doesn't exist, using defaults");
            return self
                .load_default_settings(None, portable_mode)
                .context("Failed to load default settings");
        }

        debug!(path = %self.settings_file_path.display(), "Loading settings from file");

        let _guard = self.write_guard.lock().unwrap();
        let settings = Settings::load_from_file(&self.settings_file_path, portable_mode)
            .context("Failed to load settings from file")?;

        debug!(settings = ?settings, "Loaded application settings successfully");
        self.on_settings_change(settings.clone(), None, true);
        Ok(settings)
    }

    /// Save settings to file and notify subscribers/UI
    #[instrument(level = "debug", skip(self, settings))]
    pub(crate) fn save_settings(&self, settings: &Settings) -> Result<()> {
        let _guard = self.write_guard.lock().unwrap();
        self.save_settings_locked(settings)
    }

    pub(crate) fn update_active_downloader(&self, id: &str) -> Result<()> {
        self.update_settings(|settings| settings.active_downloader_config_id = id.to_string())
    }

    pub(crate) fn update_downloader_remote(&self, expected: &str, remote: &str) -> Result<()> {
        self.update_settings(|settings| {
            if settings.rclone_remote_name == expected {
                settings.rclone_remote_name = remote.to_string();
            }
        })
    }

    fn update_settings(&self, update: impl FnOnce(&mut Settings)) -> Result<()> {
        let _guard = self.write_guard.lock().unwrap();
        let mut settings = self.watch_tx.borrow().clone();
        update(&mut settings);
        if settings == *self.watch_tx.borrow() {
            return Ok(());
        }
        self.save_settings_locked(&settings)
    }

    fn save_settings_locked(&self, settings: &Settings) -> Result<()> {
        info!(path = %self.settings_file_path.display(), settings = ?settings, "Saving settings to file");

        // Ensure parent directory exists
        if let Some(parent) = self.settings_file_path.parent()
            && !parent.exists()
        {
            info!(path = %parent.display(), "Creating settings directory");
            fs::create_dir_all(parent).context("Failed to create settings directory")?;
        }

        settings
            .save_to_file(&self.settings_file_path)
            .context("Failed to save settings to file")?;

        info!("Saved application settings successfully");
        self.on_settings_change(settings.clone(), None, false);

        Ok(())
    }

    /// Load default settings, optionally retaining provided installation id
    #[instrument(level = "debug", skip(self))]
    fn load_default_settings(
        &self,
        installation_id: Option<String>,
        portable_mode: bool,
    ) -> Result<Settings> {
        info!("Loading default settings");
        let mut settings = Settings::new(portable_mode);

        // Retain installation id if provided
        if let Some(installation_id) = installation_id {
            settings.installation_id = installation_id
        }

        let app_dir =
            self.settings_file_path.parent().context("Failed to get settings directory")?;
        settings.prepare_directories(app_dir, portable_mode)?;

        self.save_settings(&settings)?;
        info!("Default settings loaded and saved");
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn downloader_updates_preserve_current_settings_and_newer_remote_selection() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsHandler::new(dir.path().to_path_buf(), true).unwrap();
        let mut settings = handler.subscribe().borrow().clone();
        settings.bandwidth_limit = "12M".into();
        settings.rclone_remote_name = "user-choice".into();
        handler.save_settings(&settings).unwrap();
        handler.update_active_downloader("source-b").unwrap();
        handler.update_downloader_remote("obsolete-choice", "fallback").unwrap();
        let latest = handler.subscribe().borrow().clone();
        assert_eq!(latest.bandwidth_limit, "12M");
        assert_eq!(latest.active_downloader_config_id, "source-b");
        assert_eq!(latest.rclone_remote_name, "user-choice");
        handler.update_downloader_remote("user-choice", "resolved-choice").unwrap();
        assert_eq!(handler.subscribe().borrow().rclone_remote_name, "resolved-choice");
    }
}
