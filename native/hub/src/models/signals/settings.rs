use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

use crate::models::Settings;

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct LoadSettingsRequest {}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct ResetSettingsToDefaultsRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, DartSignal)]
pub(crate) struct SaveSettingsRequest {
    pub settings: Settings,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct SettingsChangedEvent {
    pub settings: Settings,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct SettingsSavedEvent {
    pub error: Option<String>,
}
