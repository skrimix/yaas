import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

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
    localeCode: 'system',
  );

  bool _isLoading = false;
  String? _error;

  SettingsState() {
    _registerSignalHandlers();
  }

  void _setIsLoading(bool isLoading) {
    _isLoading = isLoading;
    notifyListeners();
  }

  void _registerSignalHandlers() {
    SettingsChangedEvent.rustSignalStream.listen((event) {
      if (event.message.error != null) {
        _error = event.message.error;
      } else {
        _settings = event.message.settings;
        _error = null;
      }
      _setIsLoading(false);
    });

    SettingsSavedEvent.rustSignalStream.listen((event) {
      _error = event.message.error; // TODO: Show a toast if there is an error
      _setIsLoading(false);
    });
  }

  Future<void> load() async {
    _setIsLoading(true);

    LoadSettingsRequest().sendSignalToRust();
  }

  Future<void> save(Settings settings) async {
    // _setIsLoading(true);

    SaveSettingsRequest(settings: settings).sendSignalToRust();

    // For now, just notify listeners
    // _setIsLoading(false);
  }

  Future<void> resetToDefaults() async {
    _setIsLoading(true);
    ResetSettingsToDefaultsRequest().sendSignalToRust();
  }

  Settings get settings => _settings;
  bool get isLoading => _isLoading;
  String? get error => _error;
  Locale? get locale {
    final code = _settings.localeCode;
    if (code == 'system' || code.isEmpty) return null;
    return Locale(code);
  }

  Future<void> setLocaleCode(String code) async {
    _settings = _settings.copyWith(localeCode: code);
    notifyListeners();
    // Persist immediately via Rust settings handler
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }
}
