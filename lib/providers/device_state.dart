import 'dart:async';

import 'package:flutter/material.dart';
import 'package:rinf/rinf.dart';

import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

class DeviceState extends ChangeNotifier {
  AdbDevice? _device;
  AdbDevice? get device => _device;
  Map<String, InstalledPackage> _installedByPackage = const {};
  late final StreamSubscription<RustSignalPack<DeviceChangedEvent>>
      _deviceSubscription;

  DeviceState({Stream<RustSignalPack<DeviceChangedEvent>>? deviceEvents}) {
    _deviceSubscription =
        (deviceEvents ?? DeviceChangedEvent.rustSignalStream).listen((event) {
      final nextDevice = event.message.device;
      if (_device == nextDevice) return;

      final oldPackages =
          _device?.installedPackages ?? const <InstalledPackage>[];
      final newPackages =
          nextDevice?.installedPackages ?? const <InstalledPackage>[];
      if (!listEquals(oldPackages, newPackages)) {
        _installedByPackage = {
          for (final package in newPackages) package.packageName: package,
        };
      }

      _device = nextDevice;
      notifyListeners();
    });
  }

  @override
  void dispose() {
    _deviceSubscription.cancel();
    super.dispose();
  }

  bool get isConnected => _device != null;

  String get deviceName {
    final device = _device;
    if (device == null) return 'N/A';
    return device.name ?? 'Unknown (${device.product})';
  }

  String get deviceSerial => _device?.serial ?? 'N/A';
  String get deviceTrueSerial => _device?.trueSerial ?? 'N/A';
  bool get isWireless => _device?.isWireless ?? false;
  String get productName => _device?.product ?? 'N/A';
  int get batteryLevel => _device?.batteryLevel.toInt() ?? 0;
  bool? get isCharging => _device?.isCharging;

  ControllerInfo? get leftController => _device?.controllers.left;
  ControllerInfo? get rightController => _device?.controllers.right;

  SpaceInfo? get spaceInfo => _device?.spaceInfo;
  bool? get guardianPaused => _device?.guardianPaused;
  String? get usbSpeed => _device?.usbSpeed;
  bool? get isStorageConnected => _device?.storageConnected;

  /// Whether the proximity sensor is currently disabled (faked/overridden).
  /// - true: sensor is disabled (faked as close)
  /// - false: sensor is enabled (real sensor active)
  /// - null: state unknown
  bool? get proximityDisabled => _device?.proximityDisabled;

  String controllerStatusString(
      BuildContext context, ControllerInfo? controller) {
    final l10n = AppLocalizations.of(context);
    if (controller == null) return l10n.controllerStatusNotConnected;
    final ControllerStatus status = controller.status;
    if (status is ControllerStatusActive) {
      return l10n.controllerStatusActive;
    } else if (status is ControllerStatusDisabled) {
      return l10n.controllerStatusDisabled;
    } else if (status is ControllerStatusSearching) {
      return l10n.controllerStatusSearching;
    } else if (status is ControllerStatusInactive) {
      return l10n.controllerStatusInactive;
    } else if (status is ControllerStatusUnknown) {
      return status.value;
    } else {
      return l10n.controllerStatusUnknown;
    }
  }

  int controllerBatteryLevel(ControllerInfo? controller) {
    return controller?.batteryLevel ?? 0;
  }

  // Installed apps helpers
  Map<String, InstalledPackage> get installedByPackage => _installedByPackage;

  InstalledPackage? findInstalled(String packageName) =>
      _installedByPackage[packageName];
}
