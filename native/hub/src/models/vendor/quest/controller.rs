use lazy_regex::regex;
use tracing::{info, instrument, trace, warn};

pub static CONTROLLER_INFO_COMMAND: &str = "dumpsys OVRRemoteService | grep Battery";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Represents the current status of a Quest controller.
pub enum ControllerStatus {
    Active,
    Disabled,
    Searching,
    Unknown,
}

impl Default for ControllerStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

impl From<&str> for ControllerStatus {
    fn from(value: &str) -> Self {
        match value {
            "Active" => Self::Active,
            "Disabled" => Self::Disabled,
            "Searching" => Self::Searching,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
/// Info about a Quest controller status.
pub struct ControllerInfo {
    pub battery_level: Option<u8>,
    pub status: ControllerStatus,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
/// Holds info about both controllers connected to the headset.
pub struct ControllersInfo {
    pub left: Option<ControllerInfo>,
    pub right: Option<ControllerInfo>,
}

#[instrument(level = "debug")]
/// Parses the output of `QUEST_CONTROLLER_INFO_COMMAND` command.
pub fn parse_dumpsys(lines: &str) -> ControllersInfo {
    let mut result = ControllersInfo::default();

    let re = regex!(
        r#"^\s*Paired.+Type:\s*(?<type>\w{4,5}).+Battery:\s*(?<battery>\-*\d{1,3})%.+ Status: (?<status>\w+).+$"#m
    );

    for caps in re.captures_iter(lines) {
        if let (Some(controller_type), Some(battery_str), Some(controller_status)) = (
            caps.name("type").map(|m| m.as_str()),
            caps.name("battery").map(|m| m.as_str()),
            caps.name("status").map(|m| m.as_str()),
        ) {
            let controller_battery = battery_str.parse::<u8>().ok();

            if controller_battery.is_none() {
                info!(
                    "Invalid battery level for {} controller: {}",
                    controller_type.to_lowercase(),
                    battery_str
                );
            }
            match controller_type {
                "Left" => {
                    result.left = Some(ControllerInfo {
                        battery_level: controller_battery,
                        status: controller_status.into(),
                    })
                }
                "Right" => {
                    result.right = Some(ControllerInfo {
                        battery_level: controller_battery,
                        status: controller_status.into(),
                    })
                }
                _ => warn!("unexpected controller type '{}'", controller_type),
            }
        }
    }

    trace!("parsed controller levels: {:?}", result);

    if result.left.is_none() {
        warn!("left controller info not found");
    }
    if result.right.is_none() {
        warn!("right controller info not found");
    }

    result
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use super::*;

    #[test]
    fn test_quest_parse_dumpsys_controller_both() {
        let lines = "  Paired device: c93fff8c9460a480, Type:  Right, Model: JEDI, Firmware: \
                     1.9.2, ImuModel: ICM42686, Battery: 100%, Status: Active, ExternalStatus: \
                     DISABLED, TrackingStatus: ORIENTATION, BrightnessLevel: GOOD
  Paired device: 56c5083b9f13da12, Type:   Left, Model: JEDI, Firmware: 1.9.2, ImuModel: ICM42686, \
                     Battery:  50%, Status: Disabled, ExternalStatus: DISABLED, TrackingStatus: \
                     POSITION, BrightnessLevel: GOOD
  ";
        let parsed = parse_dumpsys(lines);
        assert_eq!(
            parsed.right,
            Some(ControllerInfo { battery_level: Some(100), status: ControllerStatus::Active })
        );
        assert_eq!(
            parsed.left,
            Some(ControllerInfo { battery_level: Some(50), status: ControllerStatus::Disabled })
        );
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_left() {
        let lines = "  Paired device: 56c5083b9f13da12, Type:   Left, Model: JEDI, Firmware: \
                     1.9.2, ImuModel: ICM42686, Battery:  50%, Status: Active, ExternalStatus: \
                     DISABLED, TrackingStatus: POSITION, BrightnessLevel: GOOD
  ";
        let parsed = parse_dumpsys(lines);
        assert_eq!(
            parsed.left,
            Some(ControllerInfo { battery_level: Some(50), status: ControllerStatus::Active })
        );
        assert_eq!(parsed.right, None);
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_right() {
        let lines = "  Paired device: c93fff8c9460a480, Type:  Right, Model: JEDI, Firmware: \
                     1.9.2, ImuModel: ICM42686, Battery: 100%, Status: Disabled, ExternalStatus: \
                     DISABLED, TrackingStatus: ORIENTATION, BrightnessLevel: GOOD
  ";
        let parsed = parse_dumpsys(lines);
        assert_eq!(
            parsed.right,
            Some(ControllerInfo { battery_level: Some(100), status: ControllerStatus::Disabled })
        );
        assert_eq!(parsed.left, None);
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_unknown_battery() {
        let lines = "  Paired device: c93fff8c9460a480, Type:  Right, Model: JEDI, Firmware: \
                     1.9.2, ImuModel: ICM42686, Battery: -1%, Status: Disabled, ExternalStatus: \
                     DISABLED, TrackingStatus: ORIENTATION, BrightnessLevel: GOOD
  ";
        let parsed = parse_dumpsys(lines);
        assert_eq!(
            parsed.right,
            Some(ControllerInfo { battery_level: None, status: ControllerStatus::Disabled })
        );
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_empty() {
        let lines = "\n";
        let parsed = parse_dumpsys(lines);
        assert!(parsed.left.is_none());
        assert!(parsed.right.is_none());
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_noservice() {
        let lines = "Can't find service: battery";
        let parsed = parse_dumpsys(lines);
        assert!(parsed.left.is_none());
        assert!(parsed.right.is_none());
    }
}
