import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/common/settings_tiles.dart';
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

/// Finds the tile that shows [label] as its title.
Finder _tile(String label) => find.ancestor(
      of: find.text(label),
      matching: find.byType(SettingTile),
    );

Finder _switchOf(String label) =>
    find.descendant(of: _tile(label), matching: find.byType(Switch));

Future<void> _selectChoice<T>(WidgetTester tester, String label) async {
  final dropdown = find.byType(DropdownMenu<T>);
  await tester.ensureVisible(dropdown);
  await tester.pumpAndSettle();
  await tester.tap(dropdown);
  await tester.pumpAndSettle();
  final entry = find.widgetWithText(MenuItemButton, label).last;
  await tester.ensureVisible(entry);
  await tester.pumpAndSettle();
  await tester.tap(entry);
  await tester.pumpAndSettle();
}

T? _choiceValue<T>(WidgetTester tester) => tester
    .widget<SettingChoiceTile<T>>(find.byType(SettingChoiceTile<T>))
    .value;

Future<void> _revert(WidgetTester tester, AppLocalizations l10n) async {
  final button =
      find.widgetWithText(TextButton, l10n.settingsRevertChanges).first;
  await tester.tap(button);
  await tester.pumpAndSettle();
}

Future<void> _save(WidgetTester tester, AppLocalizations l10n) async {
  await tester
      .tap(find.widgetWithText(FilledButton, l10n.settingsSaveChanges).first);
  await tester.pumpAndSettle();
}

void main() {
  for (final locale in ['en', 'ru']) {
    testWidgets('$locale uninstall backup setting saves and reverts',
        (tester) async {
      await _pumpSettings(tester, locale: locale, width: 480, textScale: 1.5);
      final context = tester.element(find.byType(SettingsScreen));
      final l10n = AppLocalizations.of(context);
      final state = context.read<SettingsState>() as _SettingsState;
      final toggle = _switchOf(l10n.settingsAutoBackupOnUninstall);

      await tester.ensureVisible(toggle);
      await tester.pumpAndSettle();
      expect(tester.widget<Switch>(toggle).value, isTrue);
      await tester.tap(toggle);
      await tester.pumpAndSettle();
      expect(tester.widget<Switch>(toggle).value, isFalse);
      await _revert(tester, l10n);
      expect(tester.widget<Switch>(toggle).value, isTrue);

      await tester.ensureVisible(toggle);
      await tester.tap(toggle);
      await tester.pumpAndSettle();
      await _save(tester, l10n);
      expect(state.savedSettings?.autoBackupOnUninstall, isFalse);
      expect(tester.takeException(), isNull);
    });
  }

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

          final menu = find.byType(DropdownMenu<DownloadCleanupPolicy>);
          await tester.ensureVisible(menu);
          await tester.pumpAndSettle();
          await tester.tap(menu);
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
      final timing = find.byType(SettingChoiceTile<DownloadCleanupTiming>);

      expect(timing, findsNothing);
      await _selectChoice<DownloadCleanupPolicy>(
          tester, l10n.settingsCleanupKeepOneVersion);
      expect(timing, findsOneWidget);
      expect(_choiceValue<DownloadCleanupTiming>(tester),
          DownloadCleanupTiming.afterInstall);
      await _selectChoice<DownloadCleanupTiming>(
          tester, l10n.settingsCleanupAfterDownload);
      await _selectChoice<DownloadCleanupPolicy>(
          tester, l10n.settingsCleanupKeepTwoVersions);
      expect(_choiceValue<DownloadCleanupTiming>(tester),
          DownloadCleanupTiming.afterDownload);

      await _selectChoice<DownloadCleanupPolicy>(
          tester, l10n.settingsCleanupKeepAllVersions);
      expect(timing, findsNothing);
      await _selectChoice<DownloadCleanupPolicy>(
          tester, l10n.settingsCleanupDeleteAfterInstall);
      expect(timing, findsNothing);
      await _selectChoice<DownloadCleanupPolicy>(
          tester, l10n.settingsCleanupKeepOneVersion);
      expect(_choiceValue<DownloadCleanupTiming>(tester),
          DownloadCleanupTiming.afterDownload);

      await _revert(tester, l10n);
      expect(timing, findsNothing);
      await _selectChoice<DownloadCleanupPolicy>(
          tester, l10n.settingsCleanupKeepTwoVersions);
      expect(_choiceValue<DownloadCleanupTiming>(tester),
          DownloadCleanupTiming.afterInstall);
      await _selectChoice<DownloadCleanupTiming>(
          tester, l10n.settingsCleanupAfterDownload);
      await _save(tester, l10n);
      expect(state.savedSettings?.cleanupPolicy,
          DownloadCleanupPolicy.keepTwoVersions);
      expect(state.savedSettings?.cleanupTiming,
          DownloadCleanupTiming.afterDownload);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('revert restores choices, text fields, and switches',
      (tester) async {
    await _pumpSettings(tester);
    final l10n =
        AppLocalizations.of(tester.element(find.byType(SettingsScreen)));

    await _selectChoice<NavigationRailLabelVisibility>(
        tester, l10n.settingsNavigationRailLabelsAll);
    expect(_choiceValue<NavigationRailLabelVisibility>(tester),
        NavigationRailLabelVisibility.all);

    final pathInput = find.descendant(
      of: _tile(l10n.settingsDownloadsLocation),
      matching: find.byType(TextField),
    );
    await tester.ensureVisible(pathInput);
    await tester.enterText(pathInput, '/tmp/downloads');
    await tester.pump();

    final toggle = _switchOf(l10n.settingsMdnsAutoConnect);
    await tester.ensureVisible(toggle);
    await tester.tap(find.text(l10n.settingsMdnsAutoConnect));
    await tester.pumpAndSettle();
    expect(tester.widget<Switch>(toggle).value, isFalse);
    expect(
      tester
          .widget<FilledButton>(
              find.widgetWithText(FilledButton, l10n.settingsSaveChanges).first)
          .onPressed,
      isNotNull,
    );

    await _revert(tester, l10n);
    expect(tester.widget<TextField>(pathInput).controller!.text,
        '/home/user/Downloads/Android applications');
    expect(_choiceValue<NavigationRailLabelVisibility>(tester),
        NavigationRailLabelVisibility.selected);
    expect(tester.widget<Switch>(toggle).value, isTrue);
    expect(tester.takeException(), isNull);
  });
}
