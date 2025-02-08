use std::fmt::Display;

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
        write!(f, "{}", match self {
            Self::Quest => "Meta Quest",
            Self::Quest2 => "Meta Quest 2",
            Self::Quest3S => "Meta Quest 3S",
            Self::QuestPro => "Meta Quest Pro",
            Self::Quest3 => "Meta Quest 3",
            Self::Unknown => "Unknown",
        })
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
}
