import 'dart:async';
import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:rinf/rinf.dart';

import '../src/bindings/bindings.dart';
import 'settings_state.dart';

/// Keeps application update progress and preferences available across pages.
class AppUpdateState extends ChangeNotifier {
  AppUpdateState({
    required SettingsState settings,
    Stream<RustSignalPack<AppUpdateStateChanged>>? events,
    VoidCallback? requestSnapshot,
    VoidCallback? check,
    VoidCallback? download,
    VoidCallback? cancel,
    Future<AppExitResponse> Function()? requestExit,
  })  : _settings = settings,
        _check =
            check ?? (() => const CheckAppUpdateRequest().sendSignalToRust()),
        _download = download ??
            (() => const DownloadAppUpdateRequest().sendSignalToRust()),
        _cancel =
            cancel ?? (() => const CancelAppUpdateRequest().sendSignalToRust()),
        _requestExit = requestExit ??
            (() => ServicesBinding.instance
                .exitApplication(AppExitType.cancelable)) {
    _subscription = (events ?? AppUpdateStateChanged.rustSignalStream)
        .listen((event) => _receive(event.message));
    _settings.addListener(_settingsChanged);
    _removeSaveErrorListener = _settings.addSaveErrorListener((error) {
      if (_pendingSettings == null) return;
      _pendingSettings = null;
      _saveError = error;
      notifyListeners();
    });
    (requestSnapshot ??
        () => const GetAppUpdateStateRequest().sendSignalToRust())();
  }

  final SettingsState _settings;
  final VoidCallback _check;
  final VoidCallback _download;
  final VoidCallback _cancel;
  final Future<AppExitResponse> Function() _requestExit;
  late final StreamSubscription<RustSignalPack<AppUpdateStateChanged>>
      _subscription;
  late final VoidCallback _removeSaveErrorListener;
  AppUpdateStateChanged? _snapshot;

  /// Set after sending a request until the next snapshot arrives.
  bool _requestPending = false;
  bool _installOnExit = false;
  bool _startupHandled = false;
  Settings? _pendingSettings;
  String? _saveError;
  bool _bannerDismissed = false;

  AppUpdateStateChanged? get snapshot => _snapshot;
  String? get saveError => _saveError;
  bool get savingPreferences => _pendingSettings != null;
  UpdateChannel get channel => _settings.settings.updateChannel;
  bool get checkUpdatesOnStartup => _settings.settings.checkUpdatesOnStartup;

  /// Whether the app is exiting to install the ready update.
  bool get installOnExit => _installOnExit;
  bool get requestPending => _requestPending || _installOnExit;
  bool get synchronized =>
      _settings.hasLoaded &&
      _snapshot != null &&
      _snapshot!.channel == channel &&
      !savingPreferences;
  bool get canChangePreferences => synchronized && !requestPending;
  bool get canCheck =>
      canChangePreferences &&
      switch (_snapshot!.phase) {
        AppUpdatePhase.idle ||
        AppUpdatePhase.upToDate ||
        AppUpdatePhase.available ||
        AppUpdatePhase.ready =>
          true,
        _ => false,
      };
  bool get canDownload =>
      canChangePreferences &&
      _snapshot!.phase == AppUpdatePhase.available &&
      _snapshot!.release != null;
  bool get canInstall =>
      canChangePreferences &&
      _snapshot!.phase == AppUpdatePhase.ready &&
      _snapshot!.release != null &&
      _snapshot!.installationUnavailableReason == null;
  bool get canCancel =>
      canChangePreferences &&
      (_snapshot!.phase == AppUpdatePhase.checking ||
          _snapshot!.phase == AppUpdatePhase.downloading);
  bool get showBanner =>
      synchronized &&
      !_bannerDismissed &&
      _snapshot!.release != null &&
      (_snapshot!.phase == AppUpdatePhase.available ||
          _snapshot!.phase == AppUpdatePhase.ready);

  void _receive(AppUpdateStateChanged value) {
    _snapshot = value;
    _requestPending = false;
    _maybeCheckOnStartup();
    notifyListeners();
  }

  void _settingsChanged() {
    final pending = _pendingSettings;
    if (pending != null &&
        _settings.settings.updateChannel == pending.updateChannel &&
        _settings.settings.checkUpdatesOnStartup ==
            pending.checkUpdatesOnStartup) {
      _pendingSettings = null;
    }
    _maybeCheckOnStartup();
    notifyListeners();
  }

  void _maybeCheckOnStartup() {
    if (_startupHandled ||
        !_settings.hasLoaded ||
        _settings.error != null ||
        !synchronized) {
      return;
    }
    _startupHandled = true;
    if (checkUpdatesOnStartup &&
        _snapshot!.phase == AppUpdatePhase.idle &&
        _snapshot!.error == null &&
        canCheck) {
      _requestPending = true;
      _check();
    }
  }

  void _send(VoidCallback request) {
    _requestPending = true;
    notifyListeners();
    request();
  }

  void check() {
    if (!canCheck) return;
    _startupHandled = true;
    _send(_check);
  }

  void download() {
    if (canDownload) _send(_download);
  }

  void cancel() {
    if (canCancel) _send(_cancel);
  }

  /// Requests the usual cancellable app exit; the backend installs the update
  /// while shutting down.
  Future<void> install() async {
    if (!canInstall) return;
    _installOnExit = true;
    notifyListeners();
    try {
      if (await _requestExit() == AppExitResponse.exit) return;
    } catch (error) {
      debugPrint('[Update] Exit request failed: $error');
    }
    _installOnExit = false;
    notifyListeners();
  }

  void savePreferences({UpdateChannel? channel, bool? checkOnStartup}) {
    if (!canChangePreferences) return;
    final settings = _settings.settings.copyWith(
      updateChannel: channel,
      checkUpdatesOnStartup: checkOnStartup,
    );
    if (settings == _settings.settings) return;
    _startupHandled = true;
    _pendingSettings = settings;
    _saveError = null;
    notifyListeners();
    _settings.save(settings);
  }

  /// Hides the update banner for this session.
  void dismissBanner() {
    _bannerDismissed = true;
    notifyListeners();
  }

  @override
  void dispose() {
    _subscription.cancel();
    _settings.removeListener(_settingsChanged);
    _removeSaveErrorListener();
    super.dispose();
  }
}
