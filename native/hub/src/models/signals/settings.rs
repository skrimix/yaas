use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

use crate::models::Settings;

#[derive(Serialize, Deserialize, DartSignal)]
pub struct LoadSettingsRequest {}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct ResetSettingsToDefaultsRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, DartSignal)]
pub struct SaveSettingsRequest {
    pub settings: Settings,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct SettingsChangedEvent {
    pub settings: Settings,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct SettingsSavedEvent {
    pub error: Option<String>,
}
