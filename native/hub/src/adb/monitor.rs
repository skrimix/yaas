use crate::{
    adb::device::{DeviceRefreshComponents, charging_from_battery_status},
    models::SpaceInfo,
};

pub(super) const EVENT_BATCH_WINDOW: std::time::Duration = std::time::Duration::from_millis(750);
pub(super) const RECONCILIATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(90);

/// Logcat shell command streaming the buffers and tags parsed by [`parse_logcat_line`].
pub(super) const LOGCAT_COMMAND: &str = "logcat -b all -T 1 -v epoch AppInfoRetrieverService:D \
                                         GuardianGatekeeperAndSysPropMgr:I SyncBossHAL:I \
                                         ControllerManagement:D SensorService:D battery_level:I \
                                         battery_status:I storage_state:I *:S";

const INTERNAL_STORAGE_UUID: &str = "41217664-9172-527a-b3d5-edabb50a7d69";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeviceMonitorEvent {
    Query(DeviceRefreshComponents),
    Charging(Option<bool>),
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
        "battery_status" => parse_battery_status_event(message).map(DeviceMonitorEvent::Charging),
        "SyncBossHAL" if is_controller_event(message) => {
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        }
        "ControllerManagement" if is_controller_management_event(message) => {
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        }
        "SensorService"
            if message.starts_with("stateChangeNotifierFunc(): Reporting controller info.") =>
        {
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
    let message = message
        .strip_prefix('[')
        .and_then(|message| message.split_once("] "))
        .and_then(|(_, message)| message.split_once("): "))
        .map_or(message, |(_, message)| message);

    (message.starts_with("Controller ")
        && (message.contains(" battery level changed:") || message.contains(" state change:")))
        || message.starts_with("Pulsar connected devices state change:")
        || message.starts_with("Refreshing input cache")
        || message.starts_with("Cache refresh complete")
}

fn is_controller_management_event(message: &str) -> bool {
    message.starts_with("processStateChange([PairedControllerInfo ")
        || message.starts_with("RemoteService::handleDeviceDisconnected - device ")
        || message.starts_with("RemoteService::recordControllerStatusUpdate: device ")
}

fn parse_battery_status_event(message: &str) -> Option<Option<bool>> {
    let fields = message.strip_prefix('[')?.strip_suffix(']')?.split(',').collect::<Vec<_>>();
    if fields.len() != 5 {
        return None;
    }

    let values = fields[..4]
        .iter()
        .map(|field| field.trim().parse::<u8>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    Some(charging_from_battery_status(values[0]))
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
    fn parses_battery_events() {
        assert_eq!(
            parse_logcat_line("1786559500.100  100  200 I battery_level: [85,4201,342]"),
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        );
        for status in [2, 5] {
            assert_eq!(
                parse_logcat_line(&format!(
                    "1786559500.100  100  200 I battery_status: [{status},2,1,1,Li-ion]"
                )),
                Some(DeviceMonitorEvent::Charging(Some(true)))
            );
        }
        for status in [3, 4] {
            assert_eq!(
                parse_logcat_line(&format!(
                    "1786559500.100  100  200 I battery_status: [{status},2,1,0,Li-ion]"
                )),
                Some(DeviceMonitorEvent::Charging(Some(false)))
            );
        }
        assert_eq!(
            parse_logcat_line("1786559500.100  100  200 I battery_status: [1,2,1,0,Li-ion]"),
            Some(DeviceMonitorEvent::Charging(None))
        );
    }

    #[test]
    fn ignores_malformed_battery_status_events() {
        for line in [
            "1786559500.100  100  200 I battery_status: [2,2,1,1]",
            "1786559500.100  100  200 I battery_status: [charging,2,1,1,Li-ion]",
            "1786559500.100  100  200 I battery_status: [2,good,1,1,Li-ion]",
            "1786559500.100  100  200 I unrelated: [2,2,1,1,Li-ion]",
        ] {
            assert_eq!(parse_logcat_line(line), None);
        }
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
            for prefix in ["", "[info   ] syncboss_hal_input_controller.c(164): "] {
                let line = format!("1786559500.100  100  200 I SyncBossHAL: {prefix}{message}");
                assert_eq!(
                    parse_logcat_line(&line),
                    Some(DeviceMonitorEvent::Query(
                        DeviceRefreshComponents::BATTERY_AND_CONTROLLERS
                    )),
                    "{line}"
                );
            }
        }
    }

    #[test]
    fn parses_controller_wake_event() {
        assert_eq!(
            parse_logcat_line(
                "09-07 00:34:16.092   989  6729 I SyncBossHAL: [info   ] \
                 syncboss_hal_input_controller.c(164): Controller a890c1643879e835 state change: \
                 asleep 1 -> 0"
            ),
            Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS))
        );
    }

    #[test]
    fn ignores_unrelated_controller_messages() {
        for prefix in ["", "[info   ] syncboss_hal_input_controller.c(164): "] {
            let line = format!(
                "1786559500.100  100  200 I SyncBossHAL: {prefix}Controller telemetry uploaded"
            );
            assert_eq!(parse_logcat_line(&line), None);
        }
    }

    #[test]
    fn parses_controller_disconnect_and_reconnect_logs() {
        for line in [
            "09-07 00:51:42.424   989  1070 I SyncBossHAL: [info   ] \
             syncboss_hal_impl_input.c(1251): Pulsar connected devices state change: 8, host \
             current time: 9873202ms",
            "09-07 00:51:42.424   989  1082 I SyncBossHAL: [info   ] pulsar_input_cache.c(893): \
             Refreshing input cache",
            "09-07 00:51:42.427   989  1082 I SyncBossHAL: [info   ] pulsar_input_cache.c(909): \
             Cache refresh complete in 3ms",
            "09-07 00:51:42.430   989  6728 D SensorService: stateChangeNotifierFunc(): Reporting \
             controller info. Change token: 22, paired=2 connected=0",
            "09-07 00:51:42.430  2251  6734 D ControllerManagement: \
             processStateChange([PairedControllerInfo JEDI a890c1643879e835 disconnected detached \
             update-required mcnt=0])",
            "09-07 00:51:42.430  2251  6734 D ControllerManagement: \
             RemoteService::handleDeviceDisconnected - device a890c1643879e835; Exiting \
             StreamingMode?: 0",
            "09-07 00:51:42.431  2251  6734 D ControllerManagement: \
             RemoteService::recordControllerStatusUpdate: device a890c1643879e835: status: \
             SEARCHING tracking: ORIENTATION, errors: --",
            "09-07 00:51:52.060   989  6728 D SensorService: stateChangeNotifierFunc(): Reporting \
             controller info. Change token: 23, paired=2 connected=1",
            "09-07 00:51:52.060  2251  6734 D ControllerManagement: \
             processStateChange([PairedControllerInfo JEDI a890c1643879e835 connected detached \
             mcnt=0])",
            "09-07 00:51:52.061  2251  6734 D ControllerManagement: \
             RemoteService::recordControllerStatusUpdate: device a890c1643879e835: status: \
             CONNECTED_ACTIVE tracking: NONE, errors: --",
            "09-07 00:48:11.310   989  6729 I SyncBossHAL: [info   ] \
             syncboss_hal_input_controller.c(179): Controller a890c1643879e835 battery level \
             changed: 0% -> 100%",
            "09-07 00:48:12.820   989  6729 I SyncBossHAL: [info   ] \
             syncboss_hal_input_controller.c(179): Controller a890c1643879e835 battery level \
             changed: 100% -> 90%",
            "09-07 00:51:52.107   989  6729 I SyncBossHAL: [info   ] \
             syncboss_hal_input_controller.c(179): Controller a890c1643879e835 battery level \
             changed: 0% -> 90%",
        ] {
            assert_eq!(
                parse_logcat_line(line),
                Some(DeviceMonitorEvent::Query(DeviceRefreshComponents::BATTERY_AND_CONTROLLERS)),
                "{line}"
            );
        }
    }

    #[test]
    fn ignores_controller_queries_and_telemetry() {
        for line in [
            "09-07 00:51:52.062   989  6728 D SensorService: Found paired controllers: 2",
            "09-07 00:51:52.062   989  6728 I SensorService: preparePairedControllerInfo is \
             adding device with valid 1",
            "09-07 00:51:13.162   989  6729 I SyncBossHAL: [info   ] \
             syncboss_hal_input_controller.c(58): a890c1643879e835: IMU 0 missed/60000 expected \
             (PER 0.00%), 0 max consecutive drops",
            "1786559500.100  100  200 D ControllerManagement: unrelated message",
        ] {
            assert_eq!(parse_logcat_line(line), None, "{line}");
        }
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
