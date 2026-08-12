use crate::{adb::device::DeviceRefreshComponents, models::SpaceInfo};

pub(super) const EVENT_BATCH_WINDOW: std::time::Duration = std::time::Duration::from_millis(750);
pub(super) const RECONCILIATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(90);

/// Logcat shell command streaming the buffers and tags parsed by [`parse_logcat_line`].
pub(super) const LOGCAT_COMMAND: &str =
    "logcat -b main,system,events -T 1 -v epoch AppInfoRetrieverService:D \
     GuardianGatekeeperAndSysPropMgr:I SyncBossHAL:I battery_level:I storage_state:I *:S";

const INTERNAL_STORAGE_UUID: &str = "41217664-9172-527a-b3d5-edabb50a7d69";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeviceMonitorEvent {
    Query(DeviceRefreshComponents),
    Storage(SpaceInfo),
}

pub(super) fn parse_logcat_line(line: &str) -> Option<DeviceMonitorEvent> {
    let (tag, message) = split_tag_and_message(line)?;

    match tag {
        "AppInfoRetrieverService" if is_package_event(message) => {
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::PACKAGES))
        }
        "battery_level" => {
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        }
        "SyncBossHAL" if is_controller_event(message) => {
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        }
        "GuardianGatekeeperAndSysPropMgr" if message.contains("debug.oculus.guardian_pause") => {
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::GUARDIAN))
        }
        "storage_state" => parse_storage_event(message).map(DeviceMonitorEvent::Storage),
        _ => None,
    }
}

fn split_tag_and_message(line: &str) -> Option<(&str, &str)> {
    let (_, rest) = line.split_once(" I ").or_else(|| line.split_once(" D "))?;
    let (tag, message) = rest.split_once(": ")?;
    let tag = tag.split_whitespace().last()?;
    Some((tag, message))
}

fn is_package_event(message: &str) -> bool {
    ["onPackageAdded: ", "onPackageChanged: ", "onPackageRemoved: "]
        .iter()
        .any(|prefix| message.starts_with(prefix))
}

fn is_controller_event(message: &str) -> bool {
    (message.starts_with("Controller ")
        && (message.contains(" battery level changed:") || message.contains(" state change:")))
        || message.starts_with("Pulsar connected devices state change:")
        || message.starts_with("Refreshing input cache")
        || message.starts_with("Cache refresh complete")
}

fn parse_storage_event(message: &str) -> Option<SpaceInfo> {
    let fields = message.strip_prefix('[')?.strip_suffix(']')?.split(',').collect::<Vec<_>>();
    if fields.len() != 5 || fields[0].trim() != INTERNAL_STORAGE_UUID {
        return None;
    }

    let available = fields[3].trim().parse().ok()?;
    let total = fields[4].trim().parse().ok()?;
    if available > total {
        return None;
    }

    Some(SpaceInfo { total, available })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_events() {
        for action in ["Added", "Changed", "Removed"] {
            let line = format!(
                "1786559500.100  100  200 D AppInfoRetrieverService: onPackage{action}: \
                 com.example.app"
            );
            assert_eq!(
                parse_logcat_line(&line),
                Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::PACKAGES))
            );
        }
    }

    #[test]
    fn parses_battery_event_but_ignores_battery_status() {
        assert_eq!(
            parse_logcat_line("1786559500.100  100  200 I battery_level: [85,4201,342]"),
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        );
        assert_eq!(
            parse_logcat_line("1786559500.100  100  200 I battery_status: [5,2,1,1,Li-ion]"),
            None
        );
    }

    #[test]
    fn parses_known_controller_events() {
        for message in [
            "Controller 1 battery level changed: 80",
            "Controller 1 state change: connected",
            "Pulsar connected devices state change: 3",
            "Refreshing input cache",
            "Cache refresh complete",
        ] {
            let line = format!("1786559500.100  100  200 I SyncBossHAL: {message}");
            assert_eq!(
                parse_logcat_line(&line),
                Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
            );
        }

        assert_eq!(
            parse_logcat_line(
                "1786559500.100  100  200 I SyncBossHAL: Controller telemetry uploaded"
            ),
            None
        );
    }

    #[test]
    fn parses_guardian_property_event() {
        assert_eq!(
            parse_logcat_line(
                "1786559500.100  100  200 I GuardianGatekeeperAndSysPropMgr: Feature \
                 debug.oculus.guardian_pause is now enabled"
            ),
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::GUARDIAN))
        );
        assert_eq!(
            parse_logcat_line(
                "1786559500.100  100  200 I GuardianGatekeeperAndSysPropMgr: Guardian started"
            ),
            None
        );
    }

    #[test]
    fn parses_internal_storage_event() {
        assert_eq!(
            parse_logcat_line(
                "1786561460.987  1520  1679 I storage_state: \
                 [41217664-9172-527a-b3d5-edabb50a7d69,0,0,25717096448,56573177856]"
            ),
            Some(DeviceMonitorEvent::Storage(SpaceInfo {
                total: 56_573_177_856,
                available: 25_717_096_448,
            }))
        );
    }

    #[test]
    fn ignores_other_or_malformed_storage_events() {
        for line in [
            "1786561460.987  1520  1679 I storage_state: \
             [fafafafa-fafa-5afa-8afa-fafa12345678,0,0,10,20]",
            "1786561460.987  1520  1679 I storage_state: \
             [41217664-9172-527a-b3d5-edabb50a7d69,0,0,not-a-number,20]",
            "1786561460.987  1520  1679 I storage_state: \
             [41217664-9172-527a-b3d5-edabb50a7d69,0,0,30,20]",
            "malformed",
        ] {
            assert_eq!(parse_logcat_line(line), None);
        }
    }
}
