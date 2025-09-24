import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

class DeviceState extends ChangeNotifier {
  AdbDevice? _device;
  AdbDevice? get device => _device;

  DeviceState() {
    DeviceChangedEvent.rustSignalStream.listen((event) {
      _device = event.message.device;
      notifyListeners();
    });
  }

  bool get isConnected => _device != null;

  String get deviceName => _device?.name ?? 'N/A';
  String get deviceSerial => _device?.serial ?? 'N/A';
  String get productName => _device?.product ?? 'N/A';
  int get batteryLevel => _device?.batteryLevel.toInt() ?? 0;

  ControllerInfo? get leftController => _device?.controllers.left;
  ControllerInfo? get rightController => _device?.controllers.right;

  SpaceInfo? get spaceInfo => _device?.spaceInfo;

  String controllerStatusString(
      BuildContext context, ControllerInfo? controller) {
    final l10n = AppLocalizations.of(context);
    if (controller == null) return l10n.controllerStatusNotConnected;
    switch (controller.status) {
      case ControllerStatus.active:
        return l10n.controllerStatusActive;
      case ControllerStatus.disabled:
        return l10n.controllerStatusDisabled;
      case ControllerStatus.searching:
        return l10n.controllerStatusSearching;
      default:
        return l10n.controllerStatusUnknown;
    }
  }

  int controllerBatteryLevel(ControllerInfo? controller) {
    return controller?.batteryLevel ?? 0;
  }
}
