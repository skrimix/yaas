# Device info monitoring references

This note records the reactive device monitor used by YAAS, the firmware sources behind it, and
approaches that depend on root access.

No implementation described here should assume that ADB runs as root.

## Local reference trees

- Android source: `/mnt/870evo/work/android_source`
- Quest 2 firmware: `/home/skrimix/Downloads/q2_51584880084600150`
  - Extracted partitions: `extracted/`
  - Decoded APKs: `apks/`
- Quest 3 firmware: `/home/skrimix/Downloads/q3_52345320027600520`
  - Extracted partitions: `extracted/`
  - Decoded APKs: `apks/`

Both firmware images are Android 14/API 34. Their build IDs are in:

- `q2_51584880084600150/extracted/system/system/build.prop`
- `q3_52345320027600520/extracted/system/system/build.prop`

## Current YAAS refresh path

The periodic full refresh runs every 5 minutes. Selective refresh requests and direct patches go
through one coordinator.

Relevant code:

- `native/hub/src/adb/service.rs`
  - `AdbService::run_device_update_coordinator()` coalesces compatible selective queries and
    applies patches in order.
  - `AdbService::run_periodic_refresh()` contains the 5-minute interval.
  - `AdbService::run_device_monitor()` runs logcat on the selected ADB transport and restarts it
    on failure.
  - `AdbService::run_device_reconciliation()` refreshes cheap control state every 90 seconds.
  - `AdbService::refresh_device()` requests all refresh components through the coordinator.
  - `AdbService::set_device()` emits the complete `DeviceChangedEvent` and notifies the monitor
    when the selected transport changes.
  - `AdbService::execute_command()` requests targeted Guardian and proximity refreshes. A
    successful storage/MTP command patches its requested value without querying over the changing
    transport. Timed proximity overrides schedule a verification 500 ms after their expiry.
- `native/hub/src/adb/monitor.rs`
  - Parses known logcat events and ignores unrelated or malformed lines.
  - Defines the 750 ms event batch window and 90-second reconciliation interval.
- `native/hub/src/adb/device/mod.rs`
  - `AdbDevice::query_components()` runs selected queries and returns a partial patch.
  - `AdbDevice::apply_patch()` retains old values for failed or unrequested components.
  - `query_package_list()` runs the pushed `list_apps.dex` helper.
  - `query_battery_info()` runs `dumpsys battery` and `rstest info --json`.
  - `query_space_info()` runs the filesystem `stat` command.
  - `query_guardian_state()` reads `debug.oculus.guardian_pause`.
  - `query_proximity_state()` parses the VR power-manager dump.
  - `query_usb_state()` calls `svc usb getFunctions` and `svc usb getUsbSpeed`.
- `native/hub/src/models/device_space.rs`
  - `SPACE_INFO_COMMAND` is `stat -fc %S:%b:%a /data`.
- `native/hub/src/models/vendor/quest_controller.rs`
  - Preferred query: `rstest info --json`.
  - Legacy fallback: `dumpsys OVRRemoteService | grep Battery`.
- `native/hub/src/models/signals/adb/device.rs`
  - Defines the full device payload sent to Dart.
- `lib/providers/device_state.dart`
  - Ignores identical device events and rebuilds its package lookup only when packages change.

Three coordinator rules matter for monitors:

1. Route monitor queries and direct values through the coordinator rather than writing device
   state independently.
2. Target patches by serial and ADB transport ID so results from an old connection are discarded.
3. Updates are processed in order. A direct patch waits for an active query to finish; revisit this
   if slow package queries make future event-driven updates noticeably late.

## Candidate event sources

### Installed packages

Preferred source: `AppInfoRetrieverService` in the normal logcat buffers.

Useful tag and messages:

```text
AppInfoRetrieverService
onPackageAdded: <package>
onPackageChanged: <package>
onPackageRemoved: <package>
```

The service exists in both firmware images:

```text
extracted/system_ext/framework/oculus-system-services.jar
class: oculus.internal.AppInfoRetrieverService
```

It registers a `PackageManagerInternal.PackageListObserver`, so its messages describe semantic
package changes. This is preferable to using the earlier `packages.list` rewrite as the main
signal. Multiple messages for one operation should be debounced before refreshing packages.

A package event should also schedule a storage refresh because installs and removals usually
change free space substantially.

`logd` has its own package-list watcher in:

```text
/mnt/870evo/work/android_source/system/logging/logd/PkgIds.cpp
```

Relevant strings:

```text
start watching /data/system/packages.list ...
ReadPackageList, total packages: <count>
```

These messages are an implementation detail, can happen before an install is fully applied, and
may be in the kernel log buffer. Keep them as a fallback or rooted-device diagnostic rather than
the primary source.

Root-only alternative:

```sh
inotifyd - /data/system/packages.list
```

On the attached rooted Quest 2 this worked while ADB ran as UID 0. It failed with permission
denied when forced to UID 2000. Stock shell is not in the `package_info` group, and
`/data/system/packages.list` is normally mode `0640 system:package_info`.

The live checks used rooted Quest 2 build `51716910066900150`, not the referenced Quest 2 firmware
build `51584880084600150`. The forced UID 2000 process still had a Magisk SELinux context, so those
checks are useful for UID and file-mode behavior but are not a complete stock SELinux test.

Toybox `inotifyd` source and mask documentation:

```text
/mnt/870evo/work/android_source/external/toybox/toys/other/inotifyd.c
```

### Headset battery

Preferred source: the `events` log buffer.

Event formats present in both firmware images:

```text
2722 battery_level (level|1|6),(voltage|1|1),(temperature|1|1)
2723 battery_status (status|1|5),(health|1|5),(present|1|5),(plugged|1|5),(technology|3)
```

Example decoded output:

```text
I/battery_level: [85,4201,342]
I/battery_status: [5,2,1,1,Li-ion]
```

Use `battery_level` as a low-noise trigger for the combined battery/controller query. This keeps
both values on the same refresh path. `battery_status` is useful if charging state is added to the
UI later.

References:

```text
q2_51584880084600150/extracted/system/system/etc/event-log-tags
q3_52345320027600520/extracted/system/system/etc/event-log-tags
/mnt/870evo/work/android_source/frameworks/base/services/core/java/com/android/server/BatteryService.java
/mnt/870evo/work/android_source/frameworks/base/services/core/java/com/android/server/EventLogTags.logtags
```

`BatteryService` emits `battery_level` when the broadcast battery level changes and
`battery_status` when status, health, presence, or plug type changes.

The frequent `healthd` lines are not needed. They include voltage/current changes and are much
noisier than the event-buffer records.

### Free space

Preferred source: `storage_state` in the `events` buffer.

Format present in both firmware images:

```text
2749 storage_state (uuid|3),(old_state|1),(new_state|1),(usable|2),(total|2)
```

Example:

```text
I/storage_state: [41217664-9172-527a-b3d5-edabb50a7d69,0,0,28776951808,56573177856]
```

The final two fields can update `SpaceInfo` directly. On the attached Quest 2, `total` matched
`stat -fc %S:%b:%a /data` exactly. `usable` differed only by changes after the event was recorded.

AOSP behavior is in:

```text
/mnt/870evo/work/android_source/frameworks/base/services/core/java/com/android/server/storage/DeviceStorageMonitorService.java
```

Important constants and behavior:

- Storage is checked once per minute.
- An event is logged after more than 64 MiB of usable-space change or after a low/full state
  transition.
- Small changes therefore still need the periodic reconciliation timer.

### Controllers

Candidate source: selected `SyncBossHAL` log messages, followed by `rstest info --json`.

Useful message fragments observed on Quest 2:

```text
Controller <id> battery level changed:
Controller <id> state change:
Pulsar connected devices state change:
Refreshing input cache
Cache refresh complete
```

Ignore unrelated `SyncBossHAL` power-state and telemetry messages. Debounce matching controller
messages, then run one combined battery/controller query.

Both firmware images contain:

```text
extracted/system_ext/bin/rstest
```

`rstest --help` advertises:

```text
monitor    Monitor for changes to device status
```

However, `rstest monitor` is not a stock-shell solution. On the rooted Quest 2 it worked as UID 0,
but forcing UID 2000 caused `registerStatusCallback` to fail with `EX_SECURITY`. The ordinary
`rstest info --json` query still worked at UID 2000.

The `SyncBossHAL` route still needs a live stock Quest 3 confirmation. The provided Quest 3
partitions contain `rstest`, but the HAL producing that tag may live in a partition or boot image
not included in the extracted tree.

### Guardian pause

Preferred source: the Guardian native log tag, followed by a property query.

Useful tag and match:

```text
GuardianGatekeeperAndSysPropMgr
debug.oculus.guardian_pause
```

Observed messages include transitions such as:

```text
Feature debug.oculus.guardian_pause was disabled by default, is now enabled by sysprop
Feature debug.oculus.guardian_pause was enabled by sysprop, is now disabled by sysprop
```

Query after a match:

```sh
getprop debug.oculus.guardian_pause
```

Both Guardian libraries contain the tag and property name:

```text
q2_51584880084600150/apks/Guardian/lib/arm64-v8a/libguardian.so
q3_52345320027600520/apks/Guardian/lib/arm64-v8a/libguardian.so
```

Do not use general Guardian lifecycle or mount/unmount messages for this field. They describe the
runtime lifecycle, not whether the pause sysprop is set.

### Virtual proximity override

Current query:

```sh
dumpsys oculus.internal.power.IVrPowerManager/default
```

Relevant first line:

```text
Virtual proximity state: CLOSE
Virtual proximity state: DISABLED
```

`CLOSE` means the real proximity sensor is overridden and the current YAAS field is `true`.
`DISABLED` means the override is off and the field is `false`.

Control broadcasts:

```text
com.oculus.vrpowermanager.prox_close
com.oculus.vrpowermanager.prox_far
com.oculus.vrpowermanager.automation_disable
```

The implementations are in `oculus.internal.VrPowerManagerService` inside both firmware copies of
`oculus-system-services.jar`. The Quest 2 image exposes interface version 2; the Quest 3 image
exposes version 3 and also accepts `--json` in its dump.

Both versions record virtual-state changes in their internal `EventHistory`, visible in `dumpsys`,
but do not emit a dependable public log event when a timed override expires. The Quest 2 handler
uses its internal message 13 for expiry. The Quest 3 handler releases the corresponding power-state
lock and restores `DISABLED` through its internal messages.

The `ActivityManager` lines below only prove that an `am broadcast` command ran:

```text
Broadcasting: Intent { act=com.oculus.vrpowermanager.prox_close ... }
Enqueued broadcast Intent { act=com.oculus.vrpowermanager.automation_disable ... }
```

They come from `ActivityManagerShellCommand` and do not cover automatic expiry or broadcasts sent
through other APIs. Reference:

```text
/mnt/870evo/work/android_source/frameworks/base/services/core/java/com/android/server/am/ActivityManagerShellCommand.java
```

Recommended handling:

- Refresh only proximity state after a YAAS proximity command.
- When YAAS supplied a duration, schedule a local verification at that deadline.
- Poll the first dump line every 60-120 seconds as a cheap fallback.
- Do not react to normal physical mount/unmount broadcasts; they do not change the override field.

To reduce dump output, test a device-side filter such as:

```sh
dumpsys oculus.internal.power.IVrPowerManager/default \
  | grep -m1 '^Virtual proximity state:'
```

### USB and MTP

Current queries:

```sh
svc usb getFunctions
svc usb getUsbSpeed
```

Possible physical-port trigger:

```text
tag: UsbPortManager
message prefix: USB port changed:
```

One cable transition produces many USB HAL and `UsbPortManager` messages. In most relevant cases,
unplugging USB also removes the ADB transport, which the existing device tracker already handles.
Do not monitor all `android.hardware.usb` messages.

Recommended handling:

- After a successful `SetStorageConnection`, apply the requested MTP value directly. Do not issue a
  follow-up query because changing USB functions can interrupt that ADB transport.
- Let the ADB device tracker handle physical disconnection and reconnection.
- If external MTP changes need to be detected later, debounce only `UsbPortManager` lines beginning
  with `USB port changed:` and refresh USB state once negotiation settles.

## Logcat monitor

YAAS starts the configured ADB executable for the selected transport:

```sh
adb -t <transport-id> --exit-on-write-error \
  logcat -b main,system,events -T 1 -v epoch \
  AppInfoRetrieverService:D \
  GuardianGatekeeperAndSysPropMgr:I \
  SyncBossHAL:I \
  battery_level:I \
  storage_state:I \
  '*:S'
```

Notes:

- `-T 1` may deliver one historical line. The initial full device refresh makes one harmless stale
  trigger acceptable.
- A selected-device watch stops the process when the serial or transport ID changes.
- Failed processes restart with exponential backoff capped at 30 seconds.
- Query events are combined in fixed 750 ms windows and sent to the coordinator without awaiting
  the results. Storage values are applied immediately through the same coordinator.
- `battery_status` is intentionally excluded because current YAAS state does not use it and it is
  noisy on tested Quest 2 firmware.

`forensic-adb` currently reads shell output through completion rather than exposing a streaming
reader. Its checked-out source is normally under:

```text
~/.cargo/git/checkouts/forensic-adb-*/<revision>/src/lib.rs
```

YAAS uses the configured ADB executable and consumes stdout with Tokio because `forensic-adb` does
not expose a streaming shell reader.

## Refresh coordination and cadence

The monitor uses these refresh components:

```text
PACKAGES
BATTERY_AND_CONTROLLERS
STORAGE
GUARDIAN
PROXIMITY
USB
FULL
```

The coordinator combines pending reasons, runs independent queries in parallel, and applies
successful results to the matching stored device value.

Current cadence:

- Immediate direct update: `storage_state`.
- Event followed by a selective query: packages, battery/controllers, Guardian. Either a headset
  battery or controller trigger refreshes both battery and controller state.
- Every 90 seconds: Guardian and proximity reconciliation, plus USB state for USB connections.
- Every 5 minutes: full safety refresh.
- After YAAS mutations: refresh only affected fields. A timed proximity override is re-queried
  500 ms after it expires.

After stock Q2 and Q3 testing shows that the monitors survive sleep, reconnect, install, controller
wake/sleep, and timed proximity expiry, the full safety interval can be increased further.

## Reproducing firmware inspection

List the important event tags:

```sh
rg -n 'battery_level|battery_status|storage_state' \
  /home/skrimix/Downloads/q2_51584880084600150/extracted/system/system/etc/event-log-tags \
  /home/skrimix/Downloads/q3_52345320027600520/extracted/system/system/etc/event-log-tags
```

Decompile the Meta system-service JAR without resources:

```sh
jadx --no-res --no-debug-info -d /tmp/q2-oculus-services \
  /home/skrimix/Downloads/q2_51584880084600150/extracted/system_ext/framework/oculus-system-services.jar

jadx --no-res --no-debug-info -d /tmp/q3-oculus-services \
  /home/skrimix/Downloads/q3_52345320027600520/extracted/system_ext/framework/oculus-system-services.jar
```

Classes to inspect:

```text
oculus.internal.AppInfoRetrieverService
oculus.internal.VrPowerManagerService
oculus.internal.vrpowermanager.ClientList
oculus.internal.power.IVrPowerManager
oculus.internal.power.IVrPowerManagerClient
```

Search both Guardian libraries:

```sh
strings /home/skrimix/Downloads/q2_51584880084600150/apks/Guardian/lib/arm64-v8a/libguardian.so \
  | rg 'GuardianGatekeeperAndSysPropMgr|guardian_pause'

strings /home/skrimix/Downloads/q3_52345320027600520/apks/Guardian/lib/arm64-v8a/libguardian.so \
  | rg 'GuardianGatekeeperAndSysPropMgr|guardian_pause'
```

## Remaining device checks

- Confirm the selected `SyncBossHAL` messages on a stock Quest 3 during controller connect,
  disconnect, sleep, wake, and battery changes.
- Confirm the combined logcat filters work with a normal non-root ADB shell on stock Q2 and Q3.
- Confirm package events for installs, updates, removals, enable/disable, and multi-package sessions.
- Check monitor behavior while the headset enters standby and after ADB reconnects.
- Check whether external MTP changes need detection beyond YAAS commands and transport reconnects.
- Measure each selective query before choosing final debounce and fallback intervals.
