import 'package:flutter/material.dart';
import '../messages/all.dart';

class DeviceState extends ChangeNotifier {
  AdbDevice? _device;
  AdbDevice? get device => _device;

  DeviceState() {
    DeviceChangedEvent.rustSignalStream.listen((event) {
      if (event.message.hasDevice()) {
        _device = event.message.device;
      } else {
        _device = null;
      }
      notifyListeners();
    });
  }

  bool get isConnected => _device != null;

  String get deviceName => _device?.name ?? 'No device connected';
  String get deviceSerial => _device?.serial ?? '';
  String get productName => _device?.product ?? '';
  int get batteryLevel => _device?.batteryLevel.toInt() ?? 0;

  ControllerInfo? get leftController => _device?.controllers.left;
  ControllerInfo? get rightController => _device?.controllers.right;

  SpaceInfo? get spaceInfo => _device?.spaceInfo;

  String controllerStatusString(ControllerInfo? controller) {
    if (controller == null) return 'Not Connected';
    switch (controller.status) {
      case ControllerStatus.CONTROLLER_STATUS_ACTIVE:
        return 'Active';
      case ControllerStatus.CONTROLLER_STATUS_DISABLED:
        return 'Disabled';
      case ControllerStatus.CONTROLLER_STATUS_SEARCHING:
        return 'Searching';
      default:
        return 'Unknown';
    }
  }

  int controllerBatteryLevel(ControllerInfo? controller) {
    return controller?.batteryLevel.toInt() ?? 0;
  }
}
