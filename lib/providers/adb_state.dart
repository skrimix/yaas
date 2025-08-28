import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

class AdbStateProvider extends ChangeNotifier {
  AdbState _state = const AdbStateServerNotRunning();
  AdbState get state => _state;

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
    if (_state is AdbStateServerNotRunning || _state is AdbStateNoDevices) {
      return Colors.red;
    } else if (_state is AdbStateDeviceConnected) {
      return Colors.green;
    }
    return Colors.yellow;
  }

  /// Gets a user-friendly description of the current ADB state
  String get statusDescription {
    if (_state is AdbStateServerNotRunning) {
      return 'ADB server not running';
    } else if (_state is AdbStateServerStarting) {
      return 'Starting ADB server';
    } else if (_state is AdbStateNoDevices) {
      return 'No devices found';
    } else if (_state is AdbStateDevicesAvailable) {
      final devices = (_state as AdbStateDevicesAvailable).value;
      return 'Devices available (${devices.length})';
    } else if (_state is AdbStateDeviceUnauthorized) {
      return 'Device unauthorized';
    } else if (_state is AdbStateDeviceConnected) {
      return 'Connected';
    }
    return 'Unknown';
  }
}
