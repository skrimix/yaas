use rinf::SignalPiece;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    Dark,
    Light,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub enum NavigationRailLabelVisibility {
    #[default]
    Selected,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub enum ConnectionType {
    #[default]
    Usb,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub enum DownloadCleanupPolicy {
    #[default]
    DeleteAfterInstall,
    KeepOneVersion,
    KeepTwoVersions,
    KeepAllVersions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SignalPiece)]
#[serde(default)]
pub struct Settings {
    pub rclone_path: String,
    pub rclone_remote_name: String,
    pub adb_path: String,
    pub preferred_connection_type: ConnectionType, // TODO: implement
    pub downloads_location: String,
    pub backups_location: String, // TODO: implement
    pub bandwidth_limit: String,
    pub cleanup_policy: DownloadCleanupPolicy, // TODO: implement
    /// Also write legacy release.json metadata alongside download.json
    pub write_legacy_release_json: bool,
    pub locale_code: String,
    pub navigation_rail_label_visibility: NavigationRailLabelVisibility,
    pub startup_page_key: String,
    /// Whether to use system/dynamic color when available
    pub use_system_color: bool,
    /// Seed color key from a fixed palette (e.g. "deep_purple")
    pub seed_color_key: String,
    /// Preferred theme mode (dark is default)
    pub theme_preference: ThemePreference,
    /// List of favorited apps (by original package name)
    pub favorite_packages: Vec<String>,
    /// Discover and auto-connect ADB over Wi‑Fi devices via mDNS
    pub mdns_auto_connect: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rclone_path: "rclone".to_string(),
            rclone_remote_name: "FFA-90".to_string(), // TODO: implement first time setup
            adb_path: "adb".to_string(),
            preferred_connection_type: ConnectionType::default(),
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
        }
    }
}
