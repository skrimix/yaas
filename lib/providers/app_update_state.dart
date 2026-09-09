import 'dart:async';

import 'package:flutter/foundation.dart';
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
    void Function(String)? download,
    void Function(String)? install,
    VoidCallback? cancel,
  })  : _settings = settings,
        _check =
            check ?? (() => const CheckAppUpdateRequest().sendSignalToRust()),
        _download = download ??
            ((id) =>
                DownloadAppUpdateRequest(candidateId: id).sendSignalToRust()),
        _install = install ??
            ((id) =>
                InstallAppUpdateRequest(candidateId: id).sendSignalToRust()),
        _cancel = cancel ??
            (() => const CancelAppUpdateRequest().sendSignalToRust()) {
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
  final void Function(String) _download;
  final void Function(String) _install;
  final VoidCallback _cancel;
  late final StreamSubscription<RustSignalPack<AppUpdateStateChanged>>
      _subscription;
  late final VoidCallback _removeSaveErrorListener;
  AppUpdateStateChanged? _snapshot;
  AppUpdatePhase? _pendingPhase;
  bool _cancelling = false;
  bool _startupHandled = false;
  Settings? _pendingSettings;
  String? _saveError;
  String? _dismissedCandidateId;

  AppUpdateStateChanged? get snapshot => _snapshot;
  String? get saveError => _saveError;
  bool get savingPreferences => _pendingSettings != null;
  UpdateChannel get channel => _settings.settings.updateChannel;
  bool get checkUpdatesOnStartup => _settings.settings.checkUpdatesOnStartup;
  bool get requestPending => _pendingPhase != null || _cancelling;
  bool get synchronized =>
      _settings.hasLoaded &&
      _snapshot != null &&
      _snapshot!.channel == channel &&
      !savingPreferences;
  bool get canChangePreferences =>
      synchronized &&
      !requestPending &&
      _snapshot!.phase != AppUpdatePhase.preparing &&
      _snapshot!.phase != AppUpdatePhase.awaitingExit;
  bool get _canAct => synchronized && !requestPending;
  bool get canCheck =>
      _canAct &&
      switch (_snapshot!.phase) {
        AppUpdatePhase.idle ||
        AppUpdatePhase.upToDate ||
        AppUpdatePhase.available ||
        AppUpdatePhase.ready =>
          true,
        _ => false,
      };
  bool get canDownload =>
      _canAct &&
      _snapshot!.phase == AppUpdatePhase.available &&
      _snapshot!.release != null;
  bool get canInstall =>
      _canAct &&
      _snapshot!.phase == AppUpdatePhase.ready &&
      _snapshot!.release != null &&
      _snapshot!.installationUnavailableReason == null;
  bool get canCancel =>
      _canAct &&
      switch (_snapshot!.phase) {
        AppUpdatePhase.checking ||
        AppUpdatePhase.downloading ||
        AppUpdatePhase.preparing =>
          true,
        _ => false,
      };
  bool get showBanner =>
      synchronized &&
      _snapshot!.release != null &&
      _dismissedCandidateId != _snapshot!.release!.candidateId &&
      (_snapshot!.phase == AppUpdatePhase.available ||
          _snapshot!.phase == AppUpdatePhase.ready);

  void _receive(AppUpdateStateChanged value) {
    final previous = _snapshot;
    _snapshot = value;
    if (value.phase == _pendingPhase ||
        value.error != null ||
        (previous != null && previous.channel != value.channel)) {
      _pendingPhase = null;
    }
    if (_cancelling &&
        value.phase != AppUpdatePhase.checking &&
        value.phase != AppUpdatePhase.downloading &&
        value.phase != AppUpdatePhase.preparing &&
        value.phase != AppUpdatePhase.awaitingExit) {
      _cancelling = false;
    }
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
      _pendingPhase = AppUpdatePhase.checking;
      _check();
    }
  }

  void check() {
    if (!canCheck) return;
    _startupHandled = true;
    _pendingPhase = AppUpdatePhase.checking;
    notifyListeners();
    _check();
  }

  void download() {
    if (!canDownload) return;
    final id = _snapshot!.release!.candidateId;
    _pendingPhase = AppUpdatePhase.downloading;
    notifyListeners();
    _download(id);
  }

  void install() {
    if (!canInstall) return;
    final id = _snapshot!.release!.candidateId;
    _pendingPhase = AppUpdatePhase.preparing;
    notifyListeners();
    _install(id);
  }

  void cancel() {
    if (!canCancel) return;
    _cancelling = true;
    notifyListeners();
    _cancel();
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

  void dismissBanner() {
    final id = _snapshot?.release?.candidateId;
    if (id == null) return;
    _dismissedCandidateId = id;
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
