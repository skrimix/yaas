use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

use crate::models::CloudApp;

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct LoadCloudAppsRequest {
    pub refresh: bool,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct CloudAppsChangedEvent {
    /// Whether a load is in progress
    pub is_loading: bool,
    /// New app list if it changed. None means no change since last
    pub apps: Option<Vec<CloudApp>>,
    pub error: Option<String>,
}
