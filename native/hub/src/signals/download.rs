use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct CloudApp {
    pub app_name: String,
    pub full_name: String,
    pub package_name: String,
    pub version_code: u32,
    pub last_updated: String,
    pub size: u32,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct LoadCloudAppsRequest {
    pub refresh: bool,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct CloudAppsChangedEvent {
    pub apps: Vec<CloudApp>,
    pub error: Option<String>,
}
