use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

use crate::models::CloudApp;

#[derive(Serialize, Deserialize, DartSignal)]
pub struct LoadCloudAppsRequest {
    pub refresh: bool,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct CloudAppsChangedEvent {
    pub apps: Vec<CloudApp>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetRcloneRemotesRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct RcloneRemotesChanged {
    pub remotes: Vec<String>,
    pub error: Option<String>,
}
