import 'dart:async';
import 'dart:typed_data';

import 'package:rinf/rinf.dart';
import 'package:yaas/providers/app_update_state.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';

Uint64 uint64(int value) => Uint64.fromBigInt(BigInt.from(value));

final updateRelease = AppUpdateRelease(
  candidateId: 'candidate-123',
  version: '1.2.3',
  buildNumber: 4,
  channel: UpdateChannel.stable,
  commit: 'abcdef0123456789',
  runNumber: uint64(123),
  runAttempt: uint64(2),
  notes:
      '## Changes\n\n- Faster downloads\n- **Better** updates\n\n[Details](https://example.com/notes)',
  releaseUrl: 'https://github.com/skrimix/yaas/releases/tag/v1.2.3',
  packageSize: uint64(100000000),
);

AppUpdateStateChanged updateSnapshot(
  AppUpdatePhase phase, {
  UpdateChannel channel = UpdateChannel.stable,
  String? unavailable,
  AppUpdateErrorKind? errorKind,
  String? error,
}) =>
    AppUpdateStateChanged(
      channel: channel,
      phase: phase,
      release: switch (phase) {
        AppUpdatePhase.idle ||
        AppUpdatePhase.checking ||
        AppUpdatePhase.upToDate =>
          null,
        _ => updateRelease.copyWith(channel: channel),
      },
      receivedBytes:
          uint64(phase == AppUpdatePhase.ready ? 100000000 : 25000000),
      installationUnavailableReason: unavailable,
      errorKind: errorKind,
      error: error,
    );

class UpdateTestSettings extends SettingsState {
  UpdateTestSettings({this.loaded = true, bool startup = false}) {
    value = super.settings.copyWith(checkUpdatesOnStartup: startup);
  }

  late Settings value;
  bool loaded;
  String? loadError;
  final saves = <Settings>[];
  final _saveErrors = <void Function(String)>[];
  @override
  Settings get settings => value;
  @override
  bool get hasLoaded => loaded;
  @override
  String? get error => loadError;
  @override
  void save(Settings settings) => saves.add(settings);
  @override
  void Function() addSaveErrorListener(void Function(String) callback) {
    _saveErrors.add(callback);
    return () => _saveErrors.remove(callback);
  }

  void publish(Settings settings) {
    value = settings;
    loaded = true;
    notifyListeners();
  }

  void failSave(String error) {
    for (final callback in _saveErrors.toList()) {
      callback(error);
    }
  }
}

class UpdateFixture {
  UpdateFixture({bool loaded = true, bool startup = false}) {
    settings = UpdateTestSettings(loaded: loaded, startup: startup);
    state = AppUpdateState(
      settings: settings,
      events: events.stream,
      requestSnapshot: () => commands.add('snapshot'),
      check: () => commands.add('check'),
      download: (id) => commands.add('download:$id'),
      install: (id) => commands.add('install:$id'),
      cancel: () => commands.add('cancel'),
    );
  }
  final events =
      StreamController<RustSignalPack<AppUpdateStateChanged>>(sync: true);
  final commands = <String>[];
  late final UpdateTestSettings settings;
  late final AppUpdateState state;
  void emit(AppUpdateStateChanged value) =>
      events.add(RustSignalPack(value, Uint8List(0)));
  Future<void> dispose() async {
    state.dispose();
    settings.dispose();
    await events.close();
  }
}
