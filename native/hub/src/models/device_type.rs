use std::fmt::Display;

use rinf::SignalPiece;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, SignalPiece)]
pub enum DeviceType {
    Quest,
    Quest2,
    Quest3,
    Quest3S,
    Quest3SXbox,
    QuestPro,
    Unknown,
}

impl Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Quest => "Meta Quest",
                Self::Quest2 => "Meta Quest 2",
                Self::Quest3S => "Meta Quest 3S",
                Self::Quest3SXbox => "Meta Quest 3S Xbox Edition",
                Self::QuestPro => "Meta Quest Pro",
                Self::Quest3 => "Meta Quest 3",
                Self::Unknown => "Unknown",
            }
        )
    }
}

impl DeviceType {
    pub fn from_product_name(product_name: &str) -> Self {
        match product_name {
            "vr_monterey" => DeviceType::Quest,
            "hollywood" => DeviceType::Quest2,
            "eureka" => DeviceType::Quest3,
            "panther" => DeviceType::Quest3S,
            "xse_panther" => DeviceType::Quest3SXbox,
            "seacliff" => DeviceType::QuestPro,
            _ => DeviceType::Unknown,
        }
    }
}
