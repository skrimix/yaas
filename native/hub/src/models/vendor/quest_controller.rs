use anyhow::{Context, Result};
use lazy_regex::regex;
use rinf::SignalPiece;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

/// Preferred command to query Quest controllers state
pub static CONTROLLER_INFO_COMMAND_JSON: &str = "rstest info --json";
/// Legacy fallback command (parsing text from dumpsys)
pub static CONTROLLER_INFO_COMMAND_DUMPSYS: &str = "dumpsys OVRRemoteService | grep Battery";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, SignalPiece)]
/// Represents the current status of a Quest controller.
pub enum ControllerStatus {
    Active,
    Disabled,
    Searching,
    Inactive,
    Unknown(String),
}

impl Default for ControllerStatus {
    fn default() -> Self {
        Self::Unknown(String::new())
    }
}

impl From<&str> for ControllerStatus {
    //  #[instrument(level = "trace")]
    fn from(value: &str) -> Self {
        match value {
            // Legacy dumpsys strings
            "Active" => Self::Active,
            "Disabled" => Self::Disabled,
            "Searching" => Self::Searching,
            "Inactive" => Self::Inactive,
            other => {
                // Accept rstest uppercase variants and prefixes
                let v = other.trim();
                match v {
                    "DISABLED" => Self::Disabled,
                    "SEARCHING" => Self::Searching,
                    "CONNECTED_ACTIVE" => Self::Active,
                    "CONNECTED_INACTIVE" => Self::Inactive,
                    _ => Self::Unknown(v.to_string()),
                }
            }
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, SignalPiece)]
/// Info about a Quest controller status.
pub struct ControllerInfo {
    pub battery_level: Option<u8>,
    pub status: ControllerStatus,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, SignalPiece)]
/// Holds info about both controllers connected to the headset.
pub struct HeadsetControllersInfo {
    pub left: Option<ControllerInfo>,
    pub right: Option<ControllerInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RstestControllerItem {
    #[serde(rename = "type")]
    controller_type: String,
    #[serde(default)]
    battery_level: Option<u8>,
    #[serde(default)]
    status: Option<String>,
}

impl HeadsetControllersInfo {
    /// Parses the JSON output of `rstest info --json`.
    /// Returns HeadsetControllersInfo for left/right controllers.
    pub fn from_rstest_json(json: &str) -> Result<Self> {
        let mut result = Self::default();

        let items: Vec<RstestControllerItem> = serde_json::from_str(json)
            .with_context(|| "Failed to deserialize rstest info --json output")?;

        for item in items.into_iter() {
            let status = item.status.as_deref().map(ControllerStatus::from).unwrap_or_default();
            let info = ControllerInfo { battery_level: item.battery_level, status };
            match item.controller_type.as_str() {
                "LeftHand" | "Left" => result.left = Some(info),
                "RightHand" | "Right" => result.right = Some(info),
                other => warn!("unexpected controller type '{}'", other),
            }
        }

        if result.left.is_none() {
            warn!("left controller info not found in rstest json");
        }
        if result.right.is_none() {
            warn!("right controller info not found in rstest json");
        }

        Ok(result)
    }

    // #[instrument(level = "debug")]
    /// Parses the output of `QUEST_CONTROLLER_INFO_COMMAND` command.
    pub fn from_dumpsys(lines: &str) -> Self {
        let mut result = Self::default();

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
                    debug!(
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
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use super::*;

    const SAMPLE_JSON_1: &str = r#"
[
        {
                "batteryLevel" : 90,
                "brightnessLevel" : "GOOD",
                "errors" : "",
                "firmwareVersion" : "1.9.2",
                "hardwareRevision" : "0x08",
                "id" : "56c5083b9f13da12",
                "imuModel" : "ICM42686",
                "lastConnectedTimestamp" : 15.827976419000001,
                "model" : "JEDI",
                "modelId" : 1,
                "serial" : "1WMHCLE0MS1204",
                "status" : "DISABLED",
                "trackingStatus" : "NONE",
                "type" : "LeftHand"
        },
        {
                "batteryLevel" : 40,
                "brightnessLevel" : "GOOD",
                "errors" : "",
                "firmwareVersion" : "1.9.2",
                "hardwareRevision" : "0x08",
                "id" : "c93fff8c9460a480",
                "imuModel" : "ICM42686",
                "lastConnectedTimestamp" : 773005.87873876607,
                "model" : "JEDI",
                "modelId" : 1,
                "serial" : "1WMHCR30LQ1205",
                "status" : "CONNECTED_ACTIVE",
                "trackingStatus" : "NONE",
                "type" : "RightHand"
        }
]
"#;

    const SAMPLE_JSON_2: &str = r#"
[
        {
                "batteryLevel" : 90,
                "brightnessLevel" : "GOOD",
                "errors" : "",
                "firmwareVersion" : "1.9.2",
                "hardwareRevision" : "0x08",
                "id" : "56c5083b9f13da12",
                "imuModel" : "ICM42686",
                "lastConnectedTimestamp" : 15.827976419000001,
                "model" : "JEDI",
                "modelId" : 1,
                "serial" : "1WMHCLE0MS1204",
                "status" : "SEARCHING",
                "trackingStatus" : "NONE",
                "type" : "LeftHand"
        },
        {
                "batteryLevel" : 50,
                "brightnessLevel" : "GOOD",
                "errors" : "",
                "firmwareVersion" : "1.9.2",
                "hardwareRevision" : "0x08",
                "id" : "c93fff8c9460a480",
                "imuModel" : "ICM42686",
                "lastConnectedTimestamp" : 797709.79250015109,
                "model" : "JEDI",
                "modelId" : 1,
                "serial" : "1WMHCR30LQ1205",
                "status" : "CONNECTED_ACTIVE",
                "trackingStatus" : "NONE",
                "type" : "RightHand"
        }
]
"#;

    #[test]
    fn test_parse_rstest_json_sample1() {
        let parsed = HeadsetControllersInfo::from_rstest_json(SAMPLE_JSON_1)
            .expect("json parse should succeed");
        assert_eq!(
            parsed.left,
            Some(ControllerInfo { battery_level: Some(90), status: ControllerStatus::Disabled })
        );
        assert_eq!(
            parsed.right,
            Some(ControllerInfo { battery_level: Some(40), status: ControllerStatus::Active })
        );
    }

    #[test]
    fn test_parse_rstest_json_sample2() {
        let parsed = HeadsetControllersInfo::from_rstest_json(SAMPLE_JSON_2)
            .expect("json parse should succeed");
        assert_eq!(
            parsed.left,
            Some(ControllerInfo { battery_level: Some(90), status: ControllerStatus::Searching })
        );
        assert_eq!(
            parsed.right,
            Some(ControllerInfo { battery_level: Some(50), status: ControllerStatus::Active })
        );
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_both() {
        let lines = "  Paired device: c93fff8c9460a480, Type:  Right, Model: JEDI, Firmware: \
                     1.9.2, ImuModel: ICM42686, Battery: 100%, Status: Active, ExternalStatus: \
                     DISABLED, TrackingStatus: ORIENTATION, BrightnessLevel: GOOD
  Paired device: 56c5083b9f13da12, Type:   Left, Model: JEDI, Firmware: 1.9.2, ImuModel: ICM42686, \
                     Battery:  50%, Status: Disabled, ExternalStatus: DISABLED, TrackingStatus: \
                     POSITION, BrightnessLevel: GOOD
  ";
        let parsed = HeadsetControllersInfo::from_dumpsys(lines);
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
        let parsed = HeadsetControllersInfo::from_dumpsys(lines);
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
        let parsed = HeadsetControllersInfo::from_dumpsys(lines);
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
        let parsed = HeadsetControllersInfo::from_dumpsys(lines);
        assert_eq!(
            parsed.right,
            Some(ControllerInfo { battery_level: None, status: ControllerStatus::Disabled })
        );
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_empty() {
        let lines = "\n";
        let parsed = HeadsetControllersInfo::from_dumpsys(lines);
        assert!(parsed.left.is_none());
        assert!(parsed.right.is_none());
    }

    #[test]
    fn test_quest_parse_dumpsys_controller_noservice() {
        let lines = "Can't find service: battery";
        let parsed = HeadsetControllersInfo::from_dumpsys(lines);
        assert!(parsed.left.is_none());
        assert!(parsed.right.is_none());
    }
}
