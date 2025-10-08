import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

class AdbStateProvider extends ChangeNotifier {
  AdbState _state = const AdbStateServerNotRunning();
  AdbState get state => _state;
  List<AdbDeviceBrief> _devicesList = const [];
  int get devicesCount => _devicesList.length;

  AdbStateProvider() {
    AdbState.rustSignalStream.listen((event) {
      _state = event.message;
      notifyListeners();
    });
    AdbDevicesList.rustSignalStream.listen((event) {
      _devicesList = List.unmodifiable(event.message.value);
      notifyListeners();
    });
  }

  bool get isConnected => _state is AdbStateDeviceConnected;

  List<AdbDeviceBrief> get availableDevices => _devicesList;

  /// Gets the connection status color based on the current ADB state
  /// - Server not running/no devices: red
  /// - Connected: green
  /// - Everything else: yellow
  Color get connectionColor {
    if (_state is AdbStateServerNotRunning ||
        _state is AdbStateServerStartFailed ||
        _state is AdbStateNoDevices) {
      return Colors.red;
    } else if (_state is AdbStateDeviceConnected) {
      return Colors.green;
    }
    return Colors.yellow;
  }

  /// Gets a user-friendly description of the current ADB state
  String statusDescription(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    if (_state is AdbStateServerNotRunning) {
      return l10n.statusAdbServerNotRunning;
    } else if (_state is AdbStateServerStarting) {
      return l10n.statusAdbServerStarting;
    } else if (_state is AdbStateServerStartFailed) {
      return l10n.statusAdbServerStartFailed;
    } else if (_state is AdbStateNoDevices) {
      return l10n.statusAdbNoDevices;
    } else if (_state is AdbStateDevicesAvailable) {
      final devices = (_state as AdbStateDevicesAvailable).value;
      return l10n.statusAdbDevicesAvailable(devices.length);
    } else if (_state is AdbStateDeviceUnauthorized) {
      return l10n.statusAdbDeviceUnauthorized;
    } else if (_state is AdbStateDeviceConnected) {
      return l10n.statusAdbConnected;
    }
    return l10n.statusAdbUnknown;
  }
}
