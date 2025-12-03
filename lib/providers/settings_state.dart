import 'package:flutter/material.dart';
import '../navigation.dart';
import '../src/bindings/bindings.dart';
import '../utils/theme_utils.dart' as app_theme;

class SettingsState extends ChangeNotifier {
  Settings _settings = Settings(
    installationId: '',
    rcloneRemoteName: '',
    adbPath: '',
    preferredConnectionType: ConnectionKind.usb,
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
    mdnsAutoConnect: true,
    popularityRange: PopularityRange.day7,
  );

  bool _isLoading = false;
  String? _error;
  bool _hasLoaded = false;
  final List<String> _availableStartupKeys = AppPageRegistry.pageKeys;
  // TODO: move to a separate state object or to app state or put in a struct
  bool _downloaderAvailable = false;
  bool _downloaderInitializing = false;
  String? _downloaderError;
  String? _downloaderConfigId;
  int _downloaderInitBytes = 0;
  int? _downloaderInitTotal;
  bool _downloaderIsDonationConfigured = false;
  bool _downloaderNeedsSetup = false;

  List<String> _rcloneRemotes = const [];
  bool _isRemotesLoading = true;
  String? _remotesError;

  final List<void Function(String error)> _saveErrorCallbacks = [];

  SettingsState() {
    _registerSignalHandlers();
  }

  /// Register a callback to be notified when settings save fails.
  /// Returns a function to unregister the callback.
  VoidCallback addSaveErrorListener(void Function(String error) callback) {
    _saveErrorCallbacks.add(callback);
    return () => _saveErrorCallbacks.remove(callback);
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
      _error = event.message.error;
      if (_error != null) {
        for (final callback in _saveErrorCallbacks) {
          callback(_error!);
        }
      }
      _setIsLoading(false);
    });

    RcloneRemotesChanged.rustSignalStream.listen((event) {
      final msg = event.message;
      _rcloneRemotes = List.unmodifiable(msg.remotes.toSet().toList());
      _remotesError = msg.error;
      _isRemotesLoading = false;
      notifyListeners();
    });

    DownloaderAvailabilityChanged.rustSignalStream.listen((event) {
      final msg = event.message;
      _downloaderAvailable = msg.available;
      _downloaderInitializing = msg.initializing;
      _downloaderError = msg.error;
      _downloaderConfigId = msg.configId;
      _downloaderIsDonationConfigured = msg.isDonationConfigured;
      _downloaderNeedsSetup = msg.needsSetup;
      notifyListeners();
    });

    DownloaderInitProgress.rustSignalStream.listen((event) {
      final p = event.message;
      _downloaderInitBytes = p.bytes.toInt();
      _downloaderInitTotal = p.totalBytes?.toInt();
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
  bool get isDownloaderAvailable => _downloaderAvailable;
  bool get isDownloaderInitializing => _downloaderInitializing;
  String? get downloaderError => _downloaderError;
  String? get downloaderConfigId => _downloaderConfigId;
  int get downloaderInitBytes => _downloaderInitBytes;
  int? get downloaderInitTotal => _downloaderInitTotal;
  double? get downloaderInitProgress =>
      _downloaderInitTotal == null || _downloaderInitTotal == 0
          ? null
          : _downloaderInitBytes / _downloaderInitTotal!;
  bool get isDownloaderDonationConfigured => _downloaderIsDonationConfigured;
  bool get downloaderNeedsSetup => _downloaderNeedsSetup;
  PopularityRange get popularityRange => _settings.popularityRange;
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

  Future<void> setPopularityRange(PopularityRange range) async {
    if (_settings.popularityRange == range) return;
    _settings = _settings.copyWith(popularityRange: range);
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  Future<void> setRcloneRemoteName(String name) async {
    if (_settings.rcloneRemoteName == name) return;
    _settings = _settings.copyWith(rcloneRemoteName: name);
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

  bool isFavorite(String truePackageName) =>
      _settings.favoritePackages.contains(truePackageName);

  void toggleFavorite(String truePackageName, {bool? value}) {
    final current = _settings.favoritePackages.toList(growable: true);
    final isFav = current.contains(truePackageName);
    final shouldFav = value ?? !isFav;
    if (shouldFav && !isFav) {
      current.add(truePackageName);
    } else if (!shouldFav && isFav) {
      current.removeWhere((p) => p == truePackageName);
    } else {
      return;
    }
    _settings =
        _settings.copyWith(favoritePackages: List.unmodifiable(current));
    notifyListeners();
    // Persist via Rust settings handler
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  void addFavoritesBulk(Iterable<String> truePackageNames) {
    final set = _settings.favoritePackages.toSet();
    bool changed = false;
    for (final name in truePackageNames) {
      if (set.add(name)) changed = true;
    }
    if (!changed) return;
    _settings = _settings.copyWith(favoritePackages: List.unmodifiable(set));
    notifyListeners();
    SaveSettingsRequest(settings: _settings).sendSignalToRust();
  }

  void removeFavoritesBulk(Iterable<String> truePackageNames) {
    final set = _settings.favoritePackages.toSet();
    bool changed = false;
    for (final name in truePackageNames) {
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
