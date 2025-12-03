use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rinf::SignalPiece;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemePreference {
    #[default]
    Dark,
    Light,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NavigationRailLabelVisibility {
    #[default]
    Selected,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub(crate) enum ConnectionKind {
    #[default]
    Usb,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PopularityRange {
    Day1,
    #[default]
    Day7,
    Day30,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub(crate) enum DownloadCleanupPolicy {
    #[default]
    DeleteAfterInstall,
    KeepOneVersion,
    KeepTwoVersions,
    KeepAllVersions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SignalPiece)]
#[serde(default)]
pub(crate) struct Settings {
    pub installation_id: String,
    pub rclone_remote_name: String,
    pub adb_path: String,
    pub preferred_connection_type: ConnectionKind, // TODO: implement
    downloads_location: String,
    backups_location: String,
    pub bandwidth_limit: String,
    pub cleanup_policy: DownloadCleanupPolicy,
    /// Also write legacy release.json metadata alongside download.json
    pub write_legacy_release_json: bool,
    /// Locale code (language) for the UI
    locale_code: String,
    navigation_rail_label_visibility: NavigationRailLabelVisibility,
    startup_page_key: String,
    /// Whether to use system/dynamic color when available
    use_system_color: bool,
    /// Seed color key from a fixed palette (e.g. "deep_purple")
    seed_color_key: String,
    /// Preferred theme mode (dark is default)
    theme_preference: ThemePreference,
    /// List of favorited apps (by true package name)
    favorite_packages: Vec<String>,
    /// Discover and auto-connect ADB over Wi‑Fi devices via mDNS
    pub mdns_auto_connect: bool,
    /// Popularity display range
    popularity_range: PopularityRange,
}

impl Default for Settings {
    /// For serde only. Use `Settings::new` instead.
    fn default() -> Self {
        Self {
            installation_id: Uuid::new_v4().to_string(),
            rclone_remote_name: "FFA-90".to_string(),
            adb_path: "adb".to_string(),
            preferred_connection_type: ConnectionKind::default(),
            downloads_location: dirs::download_dir()
                .expect("Failed to get download directory")
                .join("YAAS")
                .to_string_lossy()
                .to_string(),
            backups_location: dirs::document_dir()
                .expect("Failed to get document directory")
                .join("YAAS_backups")
                .to_string_lossy()
                .to_string(),
            bandwidth_limit: String::new(),
            cleanup_policy: DownloadCleanupPolicy::default(),
            write_legacy_release_json: false,
            locale_code: "system".to_string(),
            navigation_rail_label_visibility: NavigationRailLabelVisibility::default(),
            startup_page_key: "home".to_string(),
            use_system_color: false,
            seed_color_key: "deep_purple".to_string(),
            theme_preference: ThemePreference::Dark,
            favorite_packages: Vec::new(),
            mdns_auto_connect: true,
            popularity_range: PopularityRange::default(),
        }
    }
}

impl Settings {
    pub(crate) fn new(portable_mode: bool) -> Self {
        let mut settings = Settings::default();

        if portable_mode {
            settings.downloads_location = "downloads".to_string();
            settings.backups_location = "backups".to_string();
        }

        settings
    }

    pub(crate) fn load_from_file(settings_file: &Path, portable_mode: bool) -> Result<Self> {
        let file_content =
            fs::read_to_string(settings_file).context("Failed to read settings file")?;

        let mut settings: Settings =
            serde_json::from_str(&file_content).context("Failed to parse settings file")?;

        // TODO: Validate settings
        let defaults = Settings::new(portable_mode);

        let downloads_path = Path::new(&settings.downloads_location);
        let backups_path = Path::new(&settings.backups_location);
        let default_downloads_path = Path::new(&defaults.downloads_location);
        let default_backups_path = Path::new(&defaults.backups_location);

        // If paths came from defaults, ensure those directories exist.
        if downloads_path == default_downloads_path {
            let _ = fs::create_dir_all(downloads_path);
        }
        if backups_path == default_backups_path {
            let _ = fs::create_dir_all(backups_path);
        }

        // Check that effective paths exist; if not, fall back to defaults.
        if !downloads_path.exists() {
            warn!(
                path = %downloads_path.display(),
                "Downloads directory does not exist, resetting to default"
            );
            settings.downloads_location = defaults.downloads_location;
        }
        if !backups_path.exists() {
            warn!(
                path = %backups_path.display(),
                "Backups directory does not exist, resetting to default"
            );
            settings.backups_location = defaults.backups_location;
        }

        Ok(settings)
    }

    pub(crate) fn save_to_file(&self, settings_file: &Path) -> Result<()> {
        // TODO: Validate settings

        let settings_json =
            serde_json::to_string_pretty(self).context("Failed to serialize settings")?;
        fs::write(settings_file, settings_json).context("Failed to write settings file")?;
        Ok(())
    }

    pub(crate) fn downloads_location(&self) -> PathBuf {
        PathBuf::from(&self.downloads_location)
    }

    pub(crate) fn backups_location(&self) -> PathBuf {
        PathBuf::from(&self.backups_location)
    }
}
