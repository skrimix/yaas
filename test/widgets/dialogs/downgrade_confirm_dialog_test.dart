import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/dialogs/downgrade_confirm_dialog.dart';

class _SettingsState extends SettingsState {
  _SettingsState(this.autoReinstallOnConflict);

  final bool autoReinstallOnConflict;
  int saves = 0;

  @override
  Settings get settings => super.settings.copyWith(
        autoReinstallOnConflict: autoReinstallOnConflict,
      );

  @override
  void save(Settings settings) => saves++;
}

InstalledPackage _installed({
  String packageName = 'com.example.app',
  String label = 'Test App',
  int versionCode = 200,
}) {
  final zero = Uint64.fromBigInt(BigInt.zero);
  return InstalledPackage(
    uid: zero,
    system: false,
    packageName: packageName,
    versionCode: Uint64.fromBigInt(BigInt.from(versionCode)),
    versionName: '2.0',
    label: label,
    launchable: true,
    vr: false,
    size: AppSize(app: zero, data: zero, cache: zero),
    isPackageRenamed: false,
  );
}

CloudApp _cloudApp({
  String packageName = 'com.example.app',
  String appName = 'Test App',
  int versionCode = 100,
}) {
  return CloudApp(
    appName: appName,
    fullName: '$appName v$versionCode',
    packageName: packageName,
    truePackageName: packageName,
    versionCode: versionCode,
    lastUpdated: '2026-01-01',
    size: Uint64.fromBigInt(BigInt.from(1024)),
  );
}

Future<_SettingsState> _pumpHost(
  WidgetTester tester, {
  required Widget Function(BuildContext context) buildTrigger,
  bool autoReinstallOnConflict = true,
  String locale = 'en',
}) async {
  await tester.binding.setSurfaceSize(const Size(520, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  final settings = _SettingsState(autoReinstallOnConflict);
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
              .copyWith(textScaler: const TextScaler.linear(1.5)),
          child: child!,
        ),
        home: Scaffold(
          body: Builder(builder: buildTrigger),
        ),
      ),
    ),
  );
  return settings;
}

void main() {
  for (final locale in ['en', 'ru']) {
    testWidgets(
      '$locale single-app downgrade dialog updates message and returns clearData choice',
      (tester) async {
        DowngradeConfirmResult? lastResult;
        final settings = await _pumpHost(
          tester,
          locale: locale,
          buildTrigger: (context) => TextButton(
            onPressed: () async {
              lastResult = await showDowngradeConfirmDialog(
                context,
                _installed(),
                _cloudApp(),
              );
            },
            child: const Text('Open'),
          ),
        );

        await tester.tap(find.text('Open'));
        await tester.pumpAndSettle();

        final l10n = AppLocalizations.of(
          tester.element(find.byType(DowngradeConfirmDialog)),
        );
        final checkbox = find.byType(CheckboxListTile);
        expect(tester.widget<CheckboxListTile>(checkbox).value, isFalse);
        expect(
          find.text(l10n.downgradeWithBackupConfirmMessage('100')),
          findsOneWidget,
        );

        final dialogSurface = find.descendant(
          of: find.byType(AlertDialog),
          matching: find.byWidgetPredicate(
            (widget) => widget is Material && widget.type == MaterialType.card,
          ),
        );
        final dialogRect = tester.getRect(dialogSurface);
        final checkboxRect = tester.getRect(checkbox);
        final confirmRect = tester.getRect(find.byType(FilledButton));

        await tester.tap(checkbox);
        await tester.pumpAndSettle();
        expect(tester.widget<CheckboxListTile>(checkbox).value, isTrue);
        expect(
          find.text(l10n.downgradeClearDataConfirmMessage('100')),
          findsOneWidget,
        );
        expect(tester.getRect(dialogSurface), dialogRect);
        expect(tester.getRect(checkbox), checkboxRect);
        expect(tester.getRect(find.byType(FilledButton)), confirmRect);

        await tester.tap(find.byType(FilledButton));
        await tester.pumpAndSettle();
        expect(lastResult?.clearData, isTrue);
        expect(settings.saves, 0);

        // Reopening resets checkbox state; confirming unchecked keeps data.
        await tester.tap(find.text('Open'));
        await tester.pumpAndSettle();
        expect(tester.widget<CheckboxListTile>(checkbox).value, isFalse);
        await tester.tap(find.byType(FilledButton));
        await tester.pumpAndSettle();
        expect(lastResult?.clearData, isFalse);

        // Cancelling returns null.
        await tester.tap(find.text('Open'));
        await tester.pumpAndSettle();
        await tester.tap(find.text(l10n.commonCancel));
        await tester.pumpAndSettle();
        expect(lastResult, isNull);
        expect(tester.takeException(), isNull);
      },
    );

    testWidgets(
      '$locale multiple-app downgrade dialog updates message and returns clearData choice',
      (tester) async {
        DowngradeConfirmResult? lastResult;
        await _pumpHost(
          tester,
          locale: locale,
          buildTrigger: (context) => TextButton(
            onPressed: () async {
              lastResult = await showMultipleDowngradeConfirmDialog(
                context,
                [
                  (
                    installed: _installed(
                      packageName: 'com.example.one',
                      label: 'One',
                      versionCode: 200,
                    ),
                    target: _cloudApp(
                      packageName: 'com.example.one',
                      appName: 'One',
                      versionCode: 100,
                    ),
                  ),
                  (
                    installed: _installed(
                      packageName: 'com.example.two',
                      label: 'Two',
                      versionCode: 50,
                    ),
                    target: _cloudApp(
                      packageName: 'com.example.two',
                      appName: 'Two',
                      versionCode: 40,
                    ),
                  ),
                ],
              );
            },
            child: const Text('Open'),
          ),
        );

        await tester.tap(find.text('Open'));
        await tester.pumpAndSettle();

        final l10n = AppLocalizations.of(
          tester.element(find.byType(MultipleDowngradeConfirmDialog)),
        );
        final checkbox = find.byType(CheckboxListTile);
        expect(tester.widget<CheckboxListTile>(checkbox).value, isFalse);
        expect(
          find.text(l10n.downgradeMultipleWithBackupConfirmMessage),
          findsOneWidget,
        );
        expect(
          find.textContaining(l10n.downgradeItemFormat('One', '200', '100')),
          findsOneWidget,
        );

        final dialogSurface = find.descendant(
          of: find.byType(AlertDialog),
          matching: find.byWidgetPredicate(
            (widget) => widget is Material && widget.type == MaterialType.card,
          ),
        );
        final dialogRect = tester.getRect(dialogSurface);

        await tester.tap(checkbox);
        await tester.pumpAndSettle();
        expect(
          find.text(l10n.downgradeMultipleClearDataConfirmMessage),
          findsOneWidget,
        );
        expect(tester.getRect(dialogSurface), dialogRect);

        await tester.tap(find.byType(FilledButton));
        await tester.pumpAndSettle();
        expect(lastResult?.clearData, isTrue);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'disabled autoReinstallOnConflict uses non-backup message until clearData is selected',
    (tester) async {
      DowngradeConfirmResult? lastResult;
      await _pumpHost(
        tester,
        autoReinstallOnConflict: false,
        buildTrigger: (context) => TextButton(
          onPressed: () async {
            lastResult = await showDowngradeConfirmDialog(
              context,
              _installed(),
              _cloudApp(),
            );
          },
          child: const Text('Open'),
        ),
      );

      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();

      final l10n = AppLocalizations.of(
        tester.element(find.byType(DowngradeConfirmDialog)),
      );
      expect(find.text(l10n.downgradeConfirmMessage('100')), findsOneWidget);

      await tester.tap(find.byType(CheckboxListTile));
      await tester.pumpAndSettle();
      expect(
        find.text(l10n.downgradeClearDataConfirmMessage('100')),
        findsOneWidget,
      );

      await tester.tap(find.byType(FilledButton));
      await tester.pumpAndSettle();
      expect(lastResult?.clearData, isTrue);
    },
  );
}
