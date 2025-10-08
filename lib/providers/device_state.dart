import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

class DeviceState extends ChangeNotifier {
  AdbDevice? _device;
  AdbDevice? get device => _device;
  Map<String, InstalledPackage> _installedByPackage = const {};

  DeviceState() {
    DeviceChangedEvent.rustSignalStream.listen((event) {
      _device = event.message.device;
      // Build a quick lookup map for installed packages by package name.
      final pkgs = _device?.installedPackages ?? const <InstalledPackage>[];
      _installedByPackage = {for (final p in pkgs) p.packageName: p};
      notifyListeners();
    });
  }

  bool get isConnected => _device != null;

  String get deviceName {
    final device = _device;
    if (device == null) return 'N/A';
    return device.name ?? 'Unknown (${device.product})';
  }

  String get deviceSerial => _device?.serial ?? 'N/A';
  bool get isWireless => _device?.isWireless ?? false;
  String get productName => _device?.product ?? 'N/A';
  int get batteryLevel => _device?.batteryLevel.toInt() ?? 0;

  ControllerInfo? get leftController => _device?.controllers.left;
  ControllerInfo? get rightController => _device?.controllers.right;

  SpaceInfo? get spaceInfo => _device?.spaceInfo;

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
