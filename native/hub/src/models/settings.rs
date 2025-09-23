use rinf::SignalPiece;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub enum NavigationRailLabelVisibility {
    #[default]
    Selected,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub enum ConnectionType {
    Usb,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub enum DownloadCleanupPolicy {
    DeleteAfterInstall,
    KeepOneVersion,
    KeepTwoVersions,
    KeepAllVersions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SignalPiece)]
pub struct Settings {
    pub rclone_path: String,
    pub rclone_remote_name: String,
    pub adb_path: String,
    pub preferred_connection_type: ConnectionType, // TODO: implement
    pub downloads_location: String,
    pub backups_location: String, // TODO: implement
    pub bandwidth_limit: String,
    pub cleanup_policy: DownloadCleanupPolicy, // TODO: implement
    #[serde(default)]
    pub locale_code: String,
    #[serde(default)]
    pub navigation_rail_label_visibility: NavigationRailLabelVisibility,
    #[serde(default)]
    pub startup_page_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rclone_path: "rclone".to_string(),
            rclone_remote_name: "FFA-90".to_string(), // TODO: implement first time setup
            adb_path: "adb".to_string(),
            preferred_connection_type: ConnectionType::Usb,
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
            bandwidth_limit: "".to_string(),
            cleanup_policy: DownloadCleanupPolicy::DeleteAfterInstall,
            locale_code: "system".to_string(),
            navigation_rail_label_visibility: NavigationRailLabelVisibility::Selected,
            startup_page_key: "home".to_string(),
        }
    }
}
