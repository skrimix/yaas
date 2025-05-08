import 'package:flutter/material.dart';
import 'package:rql/src/bindings/bindings.dart';

class SettingsState extends ChangeNotifier {
  Settings _settings = Settings(
    rclonePath: '',
    rcloneRemoteName: '',
    adbPath: '',
    preferredConnectionType: ConnectionType.usb,
    downloadsLocation: '',
    backupsLocation: '',
    bandwidthLimit: '',
    cleanupPolicy: DownloadCleanupPolicy.deleteAfterInstall,
  );

  bool _isLoading = false;
  String? _error;

  SettingsState() {
    loadSettings();
    _registerSignalHandlers();
  }

  void _setIsLoading(bool isLoading) {
    print('isLoading: $isLoading');
    _isLoading = isLoading;
    notifyListeners();
  }

  void _registerSignalHandlers() {
    print('registerSignalHandlers');
    SettingsLoadedEvent.rustSignalStream.listen((event) {
      print('SettingsLoadedEvent.rustSignalStream');
      if (event.message.error != null) {
        print(
            'SettingsLoadedEvent.rustSignalStream error: ${event.message.error}');
        _error = event.message.error;
      } else {
        print(
            'SettingsLoadedEvent.rustSignalStream settings: ${event.message.settings}');
        _settings = event.message.settings;
        _error = null;
      }
      _setIsLoading(false);
    });

    SettingsSavedEvent.rustSignalStream.listen((event) {
      print('SettingsSavedEvent.rustSignalStream');
      _error = event.message.error; // TODO: Show a toast if there is an error
      _setIsLoading(false);
    });
  }

  Future<void> loadSettings() async {
    print('loadSettings');
    _setIsLoading(true);

    LoadSettingsRequest().sendSignalToRust();
  }

  Future<void> saveSettings(Settings settings) async {
    print('saveSettings');
    _setIsLoading(true);

    SaveSettingsRequest(settings: settings).sendSignalToRust();

    // For now, just notify listeners
    _setIsLoading(false);
  }

  Settings get settings => _settings;
  bool get isLoading => _isLoading;
  String? get error => _error;
}
