import 'package:flutter/material.dart';
import '../navigation.dart';
import '../src/bindings/bindings.dart';
import '../utils/theme_utils.dart' as app_theme;

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
    writeLegacyReleaseJson: false,
    localeCode: 'system',
    navigationRailLabelVisibility: NavigationRailLabelVisibility.selected,
    startupPageKey: 'home',
    useSystemColor: false,
    seedColorKey: 'deep_purple',
    themePreference: ThemePreference.dark,
    favoritePackages: const [],
  );

  bool _isLoading = false;
  String? _error;
  bool _hasLoaded = false;
  final List<String> _availableStartupKeys = AppPageRegistry.pageKeys;

  List<String> _rcloneRemotes = const [];
  bool _isRemotesLoading = true;
  String? _remotesError;

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
        final changed =
            _normalizeStartupPageKey() | _normalizeAppearanceSettings();
        _error = null;
        if (changed) {
          SaveSettingsRequest(settings: _settings).sendSignalToRust();
          notifyListeners();
        }
      }
      _hasLoaded = true;
      _setIsLoading(false);
    });

    SettingsSavedEvent.rustSignalStream.listen((event) {
      _error = event.message.error; // TODO: Show a toast if there is an error
      _setIsLoading(false);
    });

    RcloneRemotesChanged.rustSignalStream.listen((event) {
      final msg = event.message;
      _rcloneRemotes = List.unmodifiable(msg.remotes.toSet().toList());
      _remotesError = msg.error;
      _isRemotesLoading = false;
      // Keep current remote as custom if not in list
      // (UI will treat as custom when not found)
      notifyListeners();
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
  bool get hasLoaded => _hasLoaded;
  String? get error => _error;
  List<String> get rcloneRemotes => _rcloneRemotes;
  bool get isRemotesLoading => _isRemotesLoading;
  String? get remotesError => _remotesError;
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

  Future<void> setUseSystemColor(bool value) async {
    _settings = _settings.copyWith(useSystemColor: value);
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  Future<void> setSeedColorKey(String key) async {
    _settings = _settings.copyWith(seedColorKey: key);
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  Future<void> setThemePreference(ThemePreference pref) async {
    _settings = _settings.copyWith(themePreference: pref);
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  Future<void> refreshRcloneRemotes() async {
    _isRemotesLoading = true;
    _remotesError = null;
    notifyListeners();
    GetRcloneRemotesRequest().sendSignalToRust();
  }

  bool _normalizeStartupPageKey() {
    if (_availableStartupKeys.isEmpty) {
      return false;
    }
    final currentKey = _settings.startupPageKey;
    if (_availableStartupKeys.contains(currentKey)) {
      return false;
    }

    final fallbackKey = _availableStartupKeys.first;
    _settings = _settings.copyWith(startupPageKey: fallbackKey);

    return true;
  }

  bool _normalizeAppearanceSettings() {
    final normalized = app_theme.normalizeSeedKey(_settings.seedColorKey);
    if (normalized != _settings.seedColorKey) {
      _settings = _settings.copyWith(seedColorKey: normalized);
      return true;
    }
    return false;
  }

  // Favorites
  Set<String> get favoritePackages => _settings.favoritePackages.toSet();

  bool isFavorite(String originalPackageName) =>
      _settings.favoritePackages.contains(originalPackageName);

  void toggleFavorite(String originalPackageName, {bool? value}) {
    final current = _settings.favoritePackages.toList(growable: true);
    final isFav = current.contains(originalPackageName);
    final shouldFav = value ?? !isFav;
    if (shouldFav && !isFav) {
      current.add(originalPackageName);
    } else if (!shouldFav && isFav) {
      current.removeWhere((p) => p == originalPackageName);
    } else {
      return;
    }
    _settings =
        _settings.copyWith(favoritePackages: List.unmodifiable(current));
    notifyListeners();
    // Persist via Rust settings handler
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  void addFavoritesBulk(Iterable<String> originalPackageNames) {
    final set = _settings.favoritePackages.toSet();
    bool changed = false;
    for (final name in originalPackageNames) {
      if (set.add(name)) changed = true;
    }
    if (!changed) return;
    _settings = _settings.copyWith(favoritePackages: List.unmodifiable(set));
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  void removeFavoritesBulk(Iterable<String> originalPackageNames) {
    final set = _settings.favoritePackages.toSet();
    bool changed = false;
    for (final name in originalPackageNames) {
      if (set.remove(name)) changed = true;
    }
    if (!changed) return;
    _settings = _settings.copyWith(favoritePackages: List.unmodifiable(set));
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  void clearFavorites() {
    if (_settings.favoritePackages.isEmpty) return;
    _settings = _settings.copyWith(favoritePackages: const []);
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }
}
