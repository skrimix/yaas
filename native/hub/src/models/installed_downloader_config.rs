use rinf::SignalPiece;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub(crate) struct InstalledDownloaderConfig {
    pub id: String,
    pub display_name: String,
    pub description: String,
}
