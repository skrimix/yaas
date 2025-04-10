use std::fmt::Display;

use crate::signals::adb::device as device_signals;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Quest,
    Quest2,
    Quest3,
    Quest3S,
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
            "seacliff" => DeviceType::QuestPro,
            // TODO: add user setting for allowing unknown devices
            _ => DeviceType::Unknown,
        }
    }

    pub fn into_proto(self) -> device_signals::DeviceType {
        match self {
            Self::Quest => device_signals::DeviceType::Quest,
            Self::Quest2 => device_signals::DeviceType::Quest2,
            Self::Quest3 => device_signals::DeviceType::Quest3,
            Self::Quest3S => device_signals::DeviceType::Quest3S,
            Self::QuestPro => device_signals::DeviceType::QuestPro,
            Self::Unknown => device_signals::DeviceType::Unknown,
        }
    }
}
