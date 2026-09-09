import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:yaas/providers/app_state.dart';
import 'package:yaas/providers/app_update_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/common/app_update_banner.dart';
import 'package:yaas/widgets/common/app_update_section.dart';

import '../support/app_update_fixture.dart';

Future<AppLocalizations> pumpUpdates(
  WidgetTester tester,
  UpdateFixture fixture, {
  String locale = 'en',
  double width = 960,
  double textScale = 1,
  bool banner = false,
  AppState? app,
}) async {
  await tester.binding.setSurfaceSize(Size(width, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(MultiProvider(
    providers: [
      ChangeNotifierProvider<AppUpdateState>.value(value: fixture.state),
      if (app != null) ChangeNotifierProvider<AppState>.value(value: app),
    ],
    child: MaterialApp(
      locale: Locale(locale),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      theme: ThemeData(
          colorScheme: ColorScheme.fromSeed(
        seedColor: Colors.deepPurple,
        brightness: width < 600 ? Brightness.dark : Brightness.light,
      )),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context)
            .copyWith(textScaler: TextScaler.linear(textScale)),
        child: child!,
      ),
      home: Scaffold(
          body: banner
              ? const AppUpdateBanner()
              : const SingleChildScrollView(child: AppUpdateSection())),
    ),
  ));
  await tester.pump();
  return AppLocalizations.of(tester.element(find.byType(Scaffold)));
}

void main() {
  late UpdateFixture fixture;
  setUp(() => fixture = UpdateFixture());
  tearDown(() => fixture.dispose());

  for (final locale in ['en', 'ru']) {
    for (final width in [480.0, 960.0]) {
      for (final phase in AppUpdatePhase.values) {
        testWidgets('$locale $phase fits at $width', (tester) async {
          fixture.emit(updateSnapshot(phase));
          final l10n = await pumpUpdates(tester, fixture,
              locale: locale, width: width, textScale: width == 480 ? 1.5 : 1);
          final expected = switch (phase) {
            AppUpdatePhase.idle => l10n.updatesIdle,
            AppUpdatePhase.checking => l10n.updatesChecking,
            AppUpdatePhase.available => l10n.updatesAvailable,
            AppUpdatePhase.upToDate => l10n.updatesUpToDate,
            AppUpdatePhase.downloading => l10n.updatesDownloading,
            AppUpdatePhase.ready => l10n.updatesReady,
            AppUpdatePhase.preparing => l10n.updatesPreparing,
            AppUpdatePhase.awaitingExit => l10n.updatesAwaitingExit,
          };
          expect(find.text(expected), findsOneWidget);
          if (phase == AppUpdatePhase.downloading) {
            expect(
                tester
                    .widget<LinearProgressIndicator>(
                        find.byType(LinearProgressIndicator))
                    .value,
                0.25);
            expect(find.textContaining('25%'), findsOneWidget);
          }
          if (phase == AppUpdatePhase.available ||
              phase == AppUpdatePhase.ready) {
            await tester.ensureVisible(find.text(l10n.updatesReleaseNotes));
            await tester.tap(find.text(l10n.updatesReleaseNotes));
            await tester.pump(const Duration(milliseconds: 300));
            expect(find.byType(MarkdownBody), findsOneWidget);
          }
          await tester.drag(
              find.byType(SingleChildScrollView).first, const Offset(0, -1200));
          await tester.pump();
          expect(tester.takeException(), isNull);
        });
      }
    }
  }

  testWidgets('snapshot loading disables controls', (tester) async {
    final l10n = await pumpUpdates(tester, fixture);
    expect(find.text(l10n.updatesLoading), findsOneWidget);
    expect(tester.widget<Switch>(find.byType(Switch)).onChanged, isNull);
    expect(
        tester
            .widget<OutlinedButton>(
                find.widgetWithText(OutlinedButton, l10n.updatesCheck))
            .onPressed,
        isNull);
  });

  testWidgets('download, cancel and install buttons use current candidate',
      (tester) async {
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    final l10n = await pumpUpdates(tester, fixture);
    await tester.ensureVisible(find.text(l10n.updatesDownload));
    await tester.tap(find.text(l10n.updatesDownload));
    await tester.pump();
    fixture.emit(updateSnapshot(AppUpdatePhase.downloading));
    await tester.pump();
    await tester.ensureVisible(find.text(l10n.commonCancel));
    await tester.tap(find.text(l10n.commonCancel));
    fixture.emit(updateSnapshot(AppUpdatePhase.ready));
    await tester.pump();
    await tester.ensureVisible(find.text(l10n.updatesInstall));
    await tester.tap(find.text(l10n.updatesInstall));
    expect(fixture.commands, [
      'snapshot',
      'download:candidate-123',
      'cancel',
      'install:candidate-123'
    ]);
  });

  testWidgets('failed preference saves restore confirmed channel and switch',
      (tester) async {
    fixture.emit(updateSnapshot(AppUpdatePhase.idle));
    final l10n = await pumpUpdates(tester, fixture);
    await tester.tap(find.byType(DropdownButton<UpdateChannel>));
    await tester.pumpAndSettle();
    await tester.tap(find.text(l10n.buildChannelNightly).last);
    await tester.pumpAndSettle();
    expect(fixture.settings.saves.single.updateChannel, UpdateChannel.nightly);
    fixture.settings.failSave('Disk full');
    await tester.pumpAndSettle();
    expect(
        tester
            .widget<DropdownButton<UpdateChannel>>(
                find.byType(DropdownButton<UpdateChannel>))
            .value,
        UpdateChannel.stable);
    expect(find.text(l10n.settingsSaveError('Disk full')), findsOneWidget);
    await tester.tap(find.byType(Switch));
    fixture.settings.failSave('Disk full');
    await tester.pumpAndSettle();
    expect(tester.widget<Switch>(find.byType(Switch)).value, isFalse);
  });

  testWidgets(
      'unsupported install explains reason and keeps notes and release link',
      (tester) async {
    fixture.emit(
        updateSnapshot(AppUpdatePhase.ready, unavailable: 'Development build'));
    final l10n = await pumpUpdates(tester, fixture);
    expect(find.text('Development build'), findsOneWidget);
    expect(find.text(l10n.updatesReleaseLink), findsOneWidget);
    expect(
        tester
            .widget<FilledButton>(
                find.widgetWithText(FilledButton, l10n.updatesInstall))
            .onPressed,
        isNull);
    fixture.emit(updateSnapshot(AppUpdatePhase.available,
        unavailable: 'Development build'));
    await tester.pump();
    expect(
        tester
            .widget<FilledButton>(
                find.widgetWithText(FilledButton, l10n.updatesDownload))
            .onPressed,
        isNotNull);
  });

  testWidgets(
      'integrity failure can be recovered by checking and downloading again',
      (tester) async {
    fixture.emit(updateSnapshot(AppUpdatePhase.ready));
    final l10n = await pumpUpdates(tester, fixture);
    await tester.ensureVisible(find.text(l10n.updatesInstall));
    await tester.tap(find.text(l10n.updatesInstall));
    fixture.emit(updateSnapshot(AppUpdatePhase.preparing));
    await tester.pump();
    fixture.emit(updateSnapshot(AppUpdatePhase.ready,
        errorKind: AppUpdateErrorKind.integrity, error: 'Checksum mismatch'));
    await tester.pump();
    expect(find.text(l10n.updatesIntegrityError), findsOneWidget);
    await tester.ensureVisible(find.text(l10n.updatesCheckAgain));
    await tester.tap(find.text(l10n.updatesCheckAgain));
    await tester.pump();
    expect(
        tester
            .widget<OutlinedButton>(
                find.widgetWithText(OutlinedButton, l10n.updatesCheckAgain))
            .onPressed,
        isNull);
    fixture.emit(updateSnapshot(AppUpdatePhase.checking));
    await tester.pump();
    fixture.emit(updateSnapshot(AppUpdatePhase.available).copyWith(
      release: () => updateRelease.copyWith(candidateId: 'fresh-candidate'),
    ));
    await tester.pump();
    expect(find.text(l10n.updatesIntegrityError), findsNothing);
    await tester.ensureVisible(find.text(l10n.updatesDownload));
    await tester.tap(find.text(l10n.updatesDownload));
    expect(fixture.commands, [
      'snapshot',
      'install:candidate-123',
      'check',
      'download:fresh-candidate',
    ]);
  });

  testWidgets('all error categories have a summary and selectable details',
      (tester) async {
    final l10n = await pumpUpdates(tester, fixture);
    for (final kind in AppUpdateErrorKind.values) {
      fixture.emit(updateSnapshot(AppUpdatePhase.ready,
          errorKind: kind, error: 'Technical details'));
      await tester.pump();
      final summary = switch (kind) {
        AppUpdateErrorKind.network => l10n.updatesNetworkError,
        AppUpdateErrorKind.noRelease => l10n.updatesNoReleaseError,
        AppUpdateErrorKind.incompleteRelease =>
          l10n.updatesIncompleteReleaseError,
        AppUpdateErrorKind.invalidMetadata => l10n.updatesInvalidMetadataError,
        AppUpdateErrorKind.noPackage => l10n.updatesNoPackageError,
        AppUpdateErrorKind.integrity => l10n.updatesIntegrityError,
        AppUpdateErrorKind.installation => l10n.updatesInstallationError,
        AppUpdateErrorKind.invalidRequest => l10n.updatesInvalidRequestError,
        AppUpdateErrorKind.recoveryRequired => l10n.updatesRecoveryError,
      };
      expect(find.text(summary), findsOneWidget);
      expect(find.text(l10n.updatesInstall), findsOneWidget);
    }
    await tester.ensureVisible(find.text(l10n.updatesErrorDetails));
    await tester.tap(find.text(l10n.updatesErrorDetails));
    await tester.pumpAndSettle();
    expect(find.widgetWithText(SelectableText, 'Technical details'),
        findsOneWidget);
  });

  testWidgets('Markdown links open externally and failures show a snackbar',
      (tester) async {
    final calls = <MethodCall>[];
    const channel = MethodChannel('plugins.flutter.io/url_launcher');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return false;
    });
    addTearDown(() => TestDefaultBinaryMessengerBinding
        .instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null));
    fixture.emit(updateSnapshot(AppUpdatePhase.available));
    final l10n = await pumpUpdates(tester, fixture);
    await tester.tap(find.text(l10n.updatesReleaseNotes));
    await tester.pumpAndSettle();
    tester.widget<MarkdownBody>(find.byType(MarkdownBody)).onTapLink!(
        'Details', 'https://example.com/notes', '');
    await tester.pumpAndSettle();
    expect(calls.single.arguments['url'], 'https://example.com/notes');
    expect(calls.single.arguments['useWebView'], isFalse);
    expect(find.text(l10n.couldNotOpenUrl('https://example.com/notes')),
        findsOneWidget);
  });

  for (final locale in ['en', 'ru']) {
    testWidgets('$locale banner navigates and dismisses without overflow',
        (tester) async {
      final app = AppState();
      addTearDown(app.dispose);
      fixture.emit(updateSnapshot(AppUpdatePhase.available));
      final l10n = await pumpUpdates(tester, fixture,
          app: app, banner: true, locale: locale, width: 480, textScale: 1.5);
      await tester.tap(find.text(l10n.updatesView));
      expect(app.takeNavigationRequest(), 'about');
      await tester.tap(find.byTooltip(l10n.updatesDismiss));
      await tester.pumpAndSettle();
      expect(find.byType(MaterialBanner), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }
}
