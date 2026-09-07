import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/dialogs/animated_uninstall_dialog.dart';

class _SettingsState extends SettingsState {
  _SettingsState(this.enabled);

  final bool enabled;
  int saves = 0;

  @override
  Settings get settings =>
      super.settings.copyWith(autoBackupOnUninstall: enabled);

  @override
  void save(Settings settings) => saves++;
}

const _packageName = 'com.example.app';

InstalledPackage _app() {
  final zero = Uint64.fromBigInt(BigInt.zero);
  return InstalledPackage(
    uid: zero,
    system: false,
    packageName: _packageName,
    versionCode: zero,
    versionName: '1.0',
    label: 'Test app',
    launchable: true,
    vr: false,
    size: AppSize(app: zero, data: zero, cache: zero),
    isPackageRenamed: false,
  );
}

RustSignalPack<AdbCommandCompletedEvent> _completion({
  bool success = true,
  String packageName = _packageName,
  AdbCommandKind kind = AdbCommandKind.uninstallPackage,
}) =>
    RustSignalPack(
      AdbCommandCompletedEvent(
        commandType: kind,
        commandKey: packageName,
        success: success,
      ),
      Uint8List(0),
    );

Future<_SettingsState> _pumpDialog(
  WidgetTester tester, {
  required Stream<RustSignalPack<AdbCommandCompletedEvent>> completions,
  required ValueChanged<AdbRequest> requestSender,
  bool enabled = true,
  String locale = 'en',
}) async {
  await tester.binding.setSurfaceSize(const Size(480, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  final settings = _SettingsState(enabled);
  addTearDown(settings.dispose);
  await tester.pumpWidget(
    ChangeNotifierProvider<SettingsState>.value(
      value: settings,
      child: MaterialApp(
        locale: Locale(locale),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context)
              .copyWith(textScaler: TextScaler.linear(1.5)),
          child: child!,
        ),
        home: Scaffold(
          body: Builder(
              builder: (context) => TextButton(
                    onPressed: () => showDialog<void>(
                      context: context,
                      builder: (_) => AnimatedUninstallDialog(
                        app: _app(),
                        completionEvents: completions,
                        requestSender: requestSender,
                      ),
                    ),
                    child: const Text('Open'),
                  )),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Open'));
  await tester.pumpAndSettle();
  return settings;
}

void main() {
  for (final locale in ['en', 'ru']) {
    testWidgets(
        '$locale skip applies to one dialog and survives a failed attempt',
        (tester) async {
      final completions = StreamController<
          RustSignalPack<AdbCommandCompletedEvent>>.broadcast();
      addTearDown(completions.close);
      final requests = <AdbRequest>[];
      final settings = await _pumpDialog(
        tester,
        completions: completions.stream,
        requestSender: requests.add,
        locale: locale,
      );
      final l10n = AppLocalizations.of(
          tester.element(find.byType(AnimatedUninstallDialog)));
      final checkbox = find.byType(CheckboxListTile);
      expect(tester.widget<CheckboxListTile>(checkbox).value, isFalse);
      expect(find.text(l10n.uninstallWithBackupConfirmMessage('Test app')),
          findsOneWidget);
      final dialogSurface = find.descendant(
        of: find.byType(AlertDialog),
        matching: find.byWidgetPredicate(
            (widget) => widget is Material && widget.type == MaterialType.card),
      );
      final dialogRect = tester.getRect(dialogSurface);
      final checkboxRect = tester.getRect(checkbox);
      final uninstallRect = tester.getRect(find.byType(FilledButton));

      await tester.tap(checkbox);
      await tester.pumpAndSettle();
      expect(
          find.text(l10n.uninstallConfirmMessage('Test app')), findsOneWidget);
      expect(tester.getRect(dialogSurface), dialogRect);
      expect(tester.getRect(checkbox), checkboxRect);
      expect(tester.getRect(find.byType(FilledButton)), uninstallRect);
      await tester.tap(find.byType(FilledButton));
      await tester.pump();
      expect((requests.single.command as AdbCommandUninstallPackage).skipBackup,
          isTrue);
      expect(
          (requests.single.command as AdbCommandUninstallPackage).packageName,
          _packageName);
      expect(requests.single.commandKey, _packageName);
      expect(find.text(l10n.uninstalling), findsOneWidget);
      expect(tester.widget<CheckboxListTile>(checkbox).onChanged, isNull);

      completions.add(_completion(success: false));
      await tester.pumpAndSettle();
      expect(tester.widget<CheckboxListTile>(checkbox).value, isTrue);
      expect(tester.widget<CheckboxListTile>(checkbox).onChanged, isNotNull);
      await tester.tap(find.text(l10n.commonCancel));
      await tester.pumpAndSettle();
      expect(completions.hasListener, isFalse);
      expect(settings.settings.autoBackupOnUninstall, isTrue);
      expect(settings.saves, 0);

      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();
      expect(tester.widget<CheckboxListTile>(checkbox).value, isFalse);
      expect(find.text(l10n.uninstallWithBackupConfirmMessage('Test app')),
          findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.tap(find.text(l10n.commonCancel));
      await tester.pumpAndSettle();
    });
  }

  testWidgets(
      'backup stays busy beyond 30 seconds and waits for matching completion',
      (tester) async {
    final completions =
        StreamController<RustSignalPack<AdbCommandCompletedEvent>>.broadcast();
    addTearDown(completions.close);
    final requests = <AdbRequest>[];
    await _pumpDialog(tester,
        completions: completions.stream, requestSender: requests.add);
    final l10n = AppLocalizations.of(
        tester.element(find.byType(AnimatedUninstallDialog)));
    await tester.tap(find.byType(FilledButton));
    await tester.pump();
    expect((requests.single.command as AdbCommandUninstallPackage).skipBackup,
        isFalse);
    expect(find.text(l10n.backingUpAndUninstalling), findsOneWidget);

    await tester.pump(const Duration(seconds: 31));
    expect(tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
        isNull);
    expect(
        tester
            .widget<CheckboxListTile>(find.byType(CheckboxListTile))
            .onChanged,
        isNull);
    await tester.tap(find.byType(FilledButton));
    await tester.pump();
    expect(requests, hasLength(1));

    completions.add(_completion(packageName: 'com.example.other'));
    completions.add(_completion(kind: AdbCommandKind.forceStopApp));
    await tester.pump();
    expect(tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
        isNull);
    completions.add(_completion());
    await tester.pump();
    expect(find.text(l10n.uninstalledDone), findsOneWidget);
    await tester.pumpAndSettle();
    await tester.pump(const Duration(milliseconds: 250));
    await tester.pumpAndSettle();
    expect(find.byType(AnimatedUninstallDialog), findsNothing);
    expect(completions.hasListener, isFalse);
  });

  testWidgets(
      'disabled automatic backups hide the checkbox and use normal uninstall text',
      (tester) async {
    final completions =
        StreamController<RustSignalPack<AdbCommandCompletedEvent>>.broadcast();
    addTearDown(completions.close);
    final requests = <AdbRequest>[];
    await _pumpDialog(
      tester,
      completions: completions.stream,
      requestSender: requests.add,
      enabled: false,
    );
    final l10n = AppLocalizations.of(
        tester.element(find.byType(AnimatedUninstallDialog)));
    expect(find.byType(CheckboxListTile), findsNothing);
    expect(find.text(l10n.uninstallConfirmMessage('Test app')), findsOneWidget);
    await tester.tap(find.byType(FilledButton));
    await tester.pump();
    expect(find.text(l10n.uninstalling), findsOneWidget);
    expect((requests.single.command as AdbCommandUninstallPackage).skipBackup,
        isFalse);
    completions.add(_completion(success: false));
    await tester.pumpAndSettle();
    expect(tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
        isNotNull);
    await tester.tap(find.text(l10n.commonCancel));
    await tester.pumpAndSettle();
  });

  testWidgets(
      'closing during backup cancels the subscription and permits late completion',
      (tester) async {
    final completions =
        StreamController<RustSignalPack<AdbCommandCompletedEvent>>.broadcast();
    addTearDown(completions.close);
    await _pumpDialog(tester,
        completions: completions.stream, requestSender: (_) {});
    final l10n = AppLocalizations.of(
        tester.element(find.byType(AnimatedUninstallDialog)));
    await tester.tap(find.byType(FilledButton));
    await tester.pump(const Duration(seconds: 2));
    await tester.tap(find.byTooltip(l10n.commonClose));
    await tester.pumpAndSettle();
    expect(completions.hasListener, isFalse);
    completions.add(_completion());
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });
}
