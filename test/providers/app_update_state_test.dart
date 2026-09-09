import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/utils/app_update_exit.dart';

import '../support/app_update_fixture.dart';

void main() {
  late UpdateFixture fixture;
  setUp(() => fixture = UpdateFixture());
  tearDown(() => fixture.dispose());

  test('subscribes before snapshot and waits for settings and matching channel',
      () async {
    await fixture.dispose();
    fixture = UpdateFixture(loaded: false, startup: true);
    expect(fixture.commands, ['snapshot']);
    expect(fixture.events.hasListener, isTrue);
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    expect(fixture.commands, ['snapshot']);
    fixture.settings.publish(
        fixture.settings.value.copyWith(updateChannel: UpdateChannel.nightly));
    expect(fixture.commands, ['snapshot']);
    fixture.emit(
        updateSnapshot(AppUpdatePhase.idle, channel: UpdateChannel.nightly));
    expect(fixture.commands, ['snapshot', 'check']);
    fixture.emit(updateSnapshot(AppUpdatePhase.checking,
        channel: UpdateChannel.nightly));
    fixture.emit(updateSnapshot(AppUpdatePhase.upToDate,
        channel: UpdateChannel.nightly));
    fixture.settings.publish(fixture.settings.value);
    expect(fixture.commands, ['snapshot', 'check']);
  });

  test('startup also works when settings arrive before snapshot', () async {
    await fixture.dispose();
    fixture = UpdateFixture(startup: true);
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    expect(fixture.commands, ['snapshot', 'check']);
  });

  test('opt-out and later enabling do not check this session', () {
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    fixture.state.savePreferences(checkOnStartup: true);
    fixture.settings.publish(fixture.settings.saves.single);
    expect(fixture.commands, ['snapshot']);
  });

  test('startup respects settings load failures and active operations',
      () async {
    await fixture.dispose();
    fixture = UpdateFixture(startup: true);
    fixture.settings.loadError = 'Cannot read settings';
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    expect(fixture.commands, ['snapshot']);
    fixture.settings.loadError = null;
    fixture.emit(updateSnapshot(AppUpdatePhase.downloading));
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    expect(fixture.commands, ['snapshot']);
  });

  test('manual check is sent once and uses backend results', () {
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    fixture.state.check();
    fixture.state.check();
    expect(fixture.commands, ['snapshot', 'check']);
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    expect(fixture.state.canCheck, isFalse,
        reason: 'Old snapshot is not an acknowledgement');
    fixture.emit(updateSnapshot(AppUpdatePhase.checking));
    expect(fixture.state.canCancel, isTrue);
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    expect(fixture.state.canDownload, isTrue);
  });

  test('download and install send current IDs and ignore duplicate actions',
      () {
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    fixture.state.download();
    fixture.state.download();
    fixture.emit(updateSnapshot(AppUpdatePhase.downloading));
    fixture.emit(updateSnapshot(AppUpdatePhase.ready));
    fixture.state.install();
    fixture.state.install();
    fixture.emit(updateSnapshot(AppUpdatePhase.preparing));
    expect(fixture.state.canChangePreferences, isFalse);
    fixture.emit(updateSnapshot(AppUpdatePhase.awaitingExit));
    expect(fixture.state.canCancel, isFalse);
    expect(fixture.commands,
        ['snapshot', 'download:candidate-123', 'install:candidate-123']);
  });

  test(
      'cancel waits for backend and preserves downloaded release after exit cancellation',
      () async {
    fixture.emit(updateSnapshot(AppUpdatePhase.downloading));
    fixture.state.cancel();
    fixture.state.cancel();
    expect(fixture.commands, ['snapshot', 'cancel']);
    fixture.emit(updateSnapshot(AppUpdatePhase.downloading));
    expect(fixture.state.canCancel, isFalse);
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    expect(fixture.state.canDownload, isTrue);
    fixture.emit(updateSnapshot(AppUpdatePhase.awaitingExit));
    final exit = AppUpdateExit(
      requestExit: () async => AppExitResponse.cancel,
      cancelUpdate: () {
        fixture.commands.add('exit-cancel');
        fixture.emit(updateSnapshot(AppUpdatePhase.ready));
      },
      isExiting: () => false,
    );
    await exit.request(updateRelease.candidateId);
    expect(fixture.state.canInstall, isTrue);
    expect(fixture.state.snapshot!.release!.candidateId,
        updateRelease.candidateId);
    expect(fixture.commands.last, 'exit-cancel');
  });

  test('installation failure retains verified download and permits retry', () {
    fixture.emit(updateSnapshot(AppUpdatePhase.ready));
    fixture.state.install();
    fixture.emit(updateSnapshot(AppUpdatePhase.ready,
        errorKind: AppUpdateErrorKind.installation, error: 'Disk full'));
    expect(fixture.state.canInstall, isTrue);
    expect(fixture.state.snapshot!.release, updateRelease);
    fixture.emit(updateSnapshot(AppUpdatePhase.ready,
        unavailable: 'Read-only installation'));
    expect(fixture.state.canInstall, isFalse);
  });

  test(
      'preference saves preserve latest settings and wait for channel agreement',
      () {
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    fixture.settings
        .publish(fixture.settings.value.copyWith(bandwidthLimit: '5M'));
    fixture.state.savePreferences(channel: UpdateChannel.nightly);
    fixture.state.savePreferences(checkOnStartup: true);
    expect(fixture.settings.saves, hasLength(1));
    expect(fixture.settings.saves.single.bandwidthLimit, '5M');
    expect(fixture.state.canDownload, isFalse);
    expect(fixture.state.channel, UpdateChannel.stable);
    fixture.settings.publish(fixture.settings.saves.single);
    expect(fixture.state.canCheck, isFalse);
    fixture.emit(
        updateSnapshot(AppUpdatePhase.idle, channel: UpdateChannel.nightly));
    expect(fixture.state.canCheck, isTrue);
    expect(fixture.state.snapshot!.release, isNull);
    expect(fixture.commands, ['snapshot']);
  });

  test('backend channel acknowledgement can arrive before saved settings', () {
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    fixture.state.savePreferences(channel: UpdateChannel.nightly);
    fixture.emit(
        updateSnapshot(AppUpdatePhase.idle, channel: UpdateChannel.nightly));
    expect(fixture.state.canCheck, isFalse);
    fixture.settings.publish(fixture.settings.saves.single);
    expect(fixture.state.canCheck, isTrue);
  });

  test('failed save keeps confirmed preferences and reports the error', () {
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    fixture.state.savePreferences(channel: UpdateChannel.nightly);
    fixture.settings.failSave('Read-only settings');
    expect(fixture.state.channel, UpdateChannel.stable);
    expect(fixture.state.saveError, 'Read-only settings');
    expect(fixture.state.canDownload, isTrue);
    expect(fixture.state.savingPreferences, isFalse);
  });

  test('banner dismissal persists for candidate, but not other candidates', () {
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    expect(fixture.state.showBanner, isTrue);
    fixture.state.dismissBanner();
    fixture.emit(updateSnapshot(AppUpdatePhase.ready));
    expect(fixture.state.showBanner, isFalse);
    fixture.emit(updateSnapshot(AppUpdatePhase.available).copyWith(
      release: () => updateRelease.copyWith(candidateId: 'next-candidate'),
    ));
    expect(fixture.state.showBanner, isTrue);
  });

  test('dispose removes stream and settings listeners', () async {
    final extra = UpdateFixture(startup: true);
    extra.state.dispose();
    await Future<void>.delayed(Duration.zero);
    expect(extra.events.hasListener, isFalse);
    extra.settings.publish(extra.settings.value);
    extra.settings.failSave('Late failure');
    expect(extra.commands, ['snapshot']);
    extra.settings.dispose();
    await extra.events.close();
  });
}
