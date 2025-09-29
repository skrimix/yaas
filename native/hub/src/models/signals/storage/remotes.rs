use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetRcloneRemotesRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct RcloneRemotesChanged {
    pub remotes: Vec<String>,
    pub error: Option<String>,
}
