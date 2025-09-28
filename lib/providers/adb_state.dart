import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

class AdbStateProvider extends ChangeNotifier {
  AdbState _state = const AdbStateServerNotRunning();
  AdbState get state => _state;
  int get devicesCount {
    if (_state is AdbStateDevicesAvailable) {
      return (_state as AdbStateDevicesAvailable).value.length;
    } else if (_state is AdbStateDeviceConnected) {
      return (_state as AdbStateDeviceConnected).count;
    } else if (_state is AdbStateDeviceUnauthorized) {
      return (_state as AdbStateDeviceUnauthorized).count;
    }
    return 0;
  }

  AdbStateProvider() {
    AdbState.rustSignalStream.listen((event) {
      _state = event.message;
      notifyListeners();
    });
  }

  bool get isConnected => _state is AdbStateDeviceConnected;

  List<String> get availableDevices {
    if (_state is AdbStateDevicesAvailable) {
      return (_state as AdbStateDevicesAvailable).value;
    }
    return [];
  }

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
      return l10n.statusAdbDevicesAvailable(devicesCount);
    } else if (_state is AdbStateDeviceConnected) {
      return l10n.statusAdbConnected;
    }
    return l10n.statusAdbUnknown;
  }
}
