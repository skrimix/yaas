use rinf::RustSignal;
use serde::{Deserialize, Serialize};

/// Response signal carrying raw battery dump output
#[derive(Serialize, Deserialize, RustSignal)]
pub struct BatteryDumpResponse {
    pub command_key: String,
    pub dump: String,
}
