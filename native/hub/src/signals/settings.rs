use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

#[derive(Serialize, Deserialize, DartSignal)]
pub struct LoadSettingsRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, DartSignal)]
pub struct SaveSettingsRequest {
    pub settings: Settings,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct SettingsLoadedEvent {
    pub settings: Settings,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct SettingsSavedEvent {
    pub error: Option<String>,
}
