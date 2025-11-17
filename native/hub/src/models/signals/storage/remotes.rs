use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetRcloneRemotesRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct RcloneRemotesChanged {
    pub remotes: Vec<String>,
    pub error: Option<String>,
}
