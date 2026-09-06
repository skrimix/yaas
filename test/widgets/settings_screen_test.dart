import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/common/setting_row.dart';
import 'package:yaas/widgets/screens/settings_screen.dart';

class _SettingsState extends SettingsState {
  Settings? savedSettings;

  @override
  void save(Settings settings) {
    savedSettings = settings;
  }

  @override
  Settings get settings => super.settings.copyWith(
        seedColorKey: '__custom__FF5733',
        downloadsLocation: '/home/user/Downloads/Android applications',
        backupsLocation: '/home/user/Documents/Android device backups',
        rcloneRemoteName: 'community-applications',
      );

  @override
  bool get isDownloaderInitializing => false;
  @override
  bool get isDownloaderAvailable => true;
  @override
  bool get downloaderSupportsRemoteSelection => true;
  @override
  bool get downloaderSupportsBandwidthLimit => true;
  @override
  bool get downloaderSupportsDownloadModeSelection => true;
  @override
  bool get isRemotesLoading => false;
  @override
  List<String> get rcloneRemotes => ['community-applications'];
}

Future<void> _pumpSettings(
  WidgetTester tester, {
  String locale = 'en',
  double width = 960,
  double textScale = 1,
  Brightness brightness = Brightness.light,
}) async {
  await tester.binding.setSurfaceSize(Size(width, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  final state = _SettingsState();
  addTearDown(state.dispose);
  await tester.pumpWidget(
    ChangeNotifierProvider<SettingsState>.value(
      value: state,
      child: MaterialApp(
        locale: Locale(locale),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        theme: ThemeData(
          colorScheme: ColorScheme.fromSeed(
            seedColor: Colors.deepPurple,
            brightness: brightness,
          ),
        ),
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(
            textScaler: TextScaler.linear(textScale),
          ),
          child: child!,
        ),
        home: const Scaffold(body: SettingsScreen()),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  for (final locale in ['en', 'ru']) {
    for (final width in [480.0, 960.0]) {
      for (final textScale in [1.0, 1.5]) {
        testWidgets('$locale settings fit at $width with $textScale text',
            (tester) async {
          await _pumpSettings(
            tester,
            locale: locale,
            width: width,
            textScale: textScale,
            brightness: width == 480 ? Brightness.dark : Brightness.light,
          );
          expect(tester.takeException(), isNull);

          final l10n =
              AppLocalizations.of(tester.element(find.byType(SettingsScreen)));
          await tester.ensureVisible(find.text(l10n.settingsDownloadMode));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);

          final row = find.ancestor(
            of: find.text(l10n.settingsDownloadsCleanup),
            matching: find.byType(SettingRow),
          );
          await tester.ensureVisible(row);
          await tester.pumpAndSettle();
          await tester.tap(find.descendant(
            of: row,
            matching:
                find.byType(DropdownButtonFormField<DownloadCleanupPolicy>),
          ));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
        });
      }
    }
  }

  for (final locale in ['en', 'ru']) {
    testWidgets('$locale cleanup timing follows retention policy and saves',
        (tester) async {
      await _pumpSettings(tester, locale: locale, width: 480, textScale: 1.5);
      final context = tester.element(find.byType(SettingsScreen));
      final l10n = AppLocalizations.of(context);
      final state =
          Provider.of<SettingsState>(context, listen: false) as _SettingsState;
      final policy =
          find.byType(DropdownButtonFormField<DownloadCleanupPolicy>);
      final timing =
          find.byType(DropdownButtonFormField<DownloadCleanupTiming>);

      Future<void> selectPolicy(String label) async {
        await tester.ensureVisible(policy);
        await tester.tap(policy);
        await tester.pumpAndSettle();
        await tester.tap(find.text(label).last);
        await tester.pumpAndSettle();
      }

      expect(timing, findsNothing);
      await selectPolicy(l10n.settingsCleanupKeepOneVersion);
      expect(timing, findsOneWidget);
      expect(tester.state<FormFieldState<DownloadCleanupTiming>>(timing).value,
          DownloadCleanupTiming.afterInstall);
      await tester.ensureVisible(timing);
      await tester.tap(timing);
      await tester.pumpAndSettle();
      await tester.tap(find.text(l10n.settingsCleanupAfterDownload).last);
      await tester.pumpAndSettle();
      await selectPolicy(l10n.settingsCleanupKeepTwoVersions);
      expect(tester.state<FormFieldState<DownloadCleanupTiming>>(timing).value,
          DownloadCleanupTiming.afterDownload);

      await selectPolicy(l10n.settingsCleanupKeepAllVersions);
      expect(timing, findsNothing);
      await selectPolicy(l10n.settingsCleanupDeleteAfterInstall);
      expect(timing, findsNothing);
      await selectPolicy(l10n.settingsCleanupKeepOneVersion);
      expect(tester.state<FormFieldState<DownloadCleanupTiming>>(timing).value,
          DownloadCleanupTiming.afterDownload);

      await tester.tap(find.byTooltip(l10n.settingsRevertChangesTooltip));
      await tester.pumpAndSettle();
      expect(timing, findsNothing);
      await selectPolicy(l10n.settingsCleanupKeepTwoVersions);
      expect(tester.state<FormFieldState<DownloadCleanupTiming>>(timing).value,
          DownloadCleanupTiming.afterInstall);
      await tester.ensureVisible(timing);
      await tester.tap(timing);
      await tester.pumpAndSettle();
      await tester.tap(find.text(l10n.settingsCleanupAfterDownload).last);
      await tester.pumpAndSettle();
      await tester
          .tap(find.widgetWithText(FilledButton, l10n.settingsSaveChanges));
      await tester.pumpAndSettle();
      expect(state.savedSettings?.cleanupPolicy,
          DownloadCleanupPolicy.keepTwoVersions);
      expect(state.savedSettings?.cleanupTiming,
          DownloadCleanupTiming.afterDownload);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('revert restores dropdowns, text fields, and switches',
      (tester) async {
    await _pumpSettings(tester);
    final l10n =
        AppLocalizations.of(tester.element(find.byType(SettingsScreen)));
    final dropdown =
        find.byType(DropdownButtonFormField<NavigationRailLabelVisibility>);
    await tester.ensureVisible(dropdown);
    await tester.tap(dropdown);
    await tester.pumpAndSettle();
    await tester.tap(find.text(l10n.settingsNavigationRailLabelsAll).last);
    await tester.pumpAndSettle();

    final pathRow = find.ancestor(
      of: find.text(l10n.settingsDownloadsLocation),
      matching: find.byType(SettingRow),
    );
    final pathInput = find.descendant(
      of: pathRow,
      matching: find.byType(TextField),
    );
    await tester.ensureVisible(pathInput);
    await tester.enterText(pathInput, '/tmp/downloads');
    await tester.pump();
    final toggleRow = find.ancestor(
      of: find.text(l10n.settingsMdnsAutoConnect),
      matching: find.byType(SettingRow),
    );
    final toggle =
        find.descendant(of: toggleRow, matching: find.byType(Switch));
    await tester.ensureVisible(toggleRow);
    await tester.tap(find.text(l10n.settingsMdnsAutoConnect));
    await tester.pumpAndSettle();
    expect(tester.widget<Switch>(toggle).value, isFalse);
    expect(
      tester
          .widget<FilledButton>(find.widgetWithText(
            FilledButton,
            l10n.settingsSaveChanges,
          ))
          .onPressed,
      isNotNull,
    );

    await tester.tap(find.byTooltip(l10n.settingsRevertChangesTooltip));
    await tester.pumpAndSettle();
    expect(tester.widget<TextField>(pathInput).controller!.text,
        '/home/user/Downloads/Android applications');
    final fieldState =
        tester.state<FormFieldState<NavigationRailLabelVisibility>>(dropdown);
    expect(fieldState.value, NavigationRailLabelVisibility.selected);
    expect(tester.widget<Switch>(toggle).value, isTrue);
    expect(tester.takeException(), isNull);
  });
}
