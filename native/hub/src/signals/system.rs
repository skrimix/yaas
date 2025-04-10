use rinf::RustSignal;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, RustSignal)]
pub struct RustPanic {
    pub message: String,
}
