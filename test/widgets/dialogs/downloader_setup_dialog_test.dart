import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/dialogs/downloader_setup_dialog.dart';

const _invalid = InstalledDownloaderConfig(
  id: 'broken',
  displayName: 'broken.json',
  description: '',
  error: 'Failed to parse downloader.json: missing field `id`',
);
const _valid = InstalledDownloaderConfig(
  id: 'healthy',
  displayName: 'Healthy source',
  description: '',
);

class _SettingsState extends SettingsState {
  _SettingsState(this.sources);

  final List<InstalledDownloaderConfig> sources;
  String? selectedId;
  String? removedId;

  @override
  List<InstalledDownloaderConfig> get downloaderSources => sources;

  @override
  void selectDownloaderSource(String configId) {
    selectedId = configId;
  }

  @override
  void removeDownloaderSource(String configId) {
    removedId = configId;
  }
}

Future<AppLocalizations> _pumpDialog(
  WidgetTester tester,
  _SettingsState state, {
  String locale = 'en',
}) async {
  await tester.binding.setSurfaceSize(const Size(1000, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  addTearDown(state.dispose);
  await tester.pumpWidget(
    ChangeNotifierProvider<SettingsState>.value(
      value: state,
      child: MaterialApp(
        locale: Locale(locale),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => const DownloaderSetupDialog(),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Open'));
  await tester.pumpAndSettle();
  return AppLocalizations.of(
      tester.element(find.byType(DownloaderSetupDialog)));
}

void main() {
  for (final locale in ['en', 'ru']) {
    testWidgets('$locale invalid source is removable but cannot be selected',
        (tester) async {
      final state = _SettingsState([_invalid, _valid]);
      final l10n = await _pumpDialog(tester, state, locale: locale);

      expect(find.text(l10n.downloaderSourceInvalid), findsOneWidget);
      expect(find.text(_invalid.error!), findsOneWidget);
      expect(find.byType(Radio<String>), findsOneWidget);
      expect(tester.widget<Radio<String>>(find.byType(Radio<String>)).value,
          _valid.id);

      await tester.tap(find.text(_invalid.displayName));
      await tester.pumpAndSettle();
      expect(state.selectedId, isNull);
      expect(find.byType(DownloaderSetupDialog), findsOneWidget);

      final remove = find
          .byTooltip(l10n.downloaderSourceRemoveTooltip(_invalid.displayName));
      expect(remove, findsOneWidget);
      await tester.tap(remove);
      await tester.pumpAndSettle();
      expect(
          find.text(l10n.downloaderSourceRemoveConfirm(_invalid.displayName)),
          findsOneWidget);
      await tester.tap(find.text(l10n.commonCancel));
      await tester.pumpAndSettle();
      expect(state.removedId, isNull);

      await tester.tap(remove);
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, l10n.remove));
      await tester.pump();
      expect(state.removedId, _invalid.id);
      expect(state.selectedId, isNull);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpAndSettle();
    });
  }

  testWidgets('only invalid sources do not show an empty list', (tester) async {
    final state = _SettingsState([_invalid]);
    final l10n = await _pumpDialog(tester, state);

    expect(find.text(l10n.downloaderSourcesEmpty), findsNothing);
    expect(find.text(_invalid.displayName), findsOneWidget);
    expect(find.byType(Radio<String>), findsNothing);
    expect(
        find.byTooltip(
            l10n.downloaderSourceRemoveTooltip(_invalid.displayName)),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('valid source remains selectable alongside an invalid source',
      (tester) async {
    final state = _SettingsState([_invalid, _valid]);
    await _pumpDialog(tester, state);

    await tester.tap(find.byType(Radio<String>));
    await tester.pumpAndSettle();

    expect(state.selectedId, _valid.id);
    expect(state.removedId, isNull);
    expect(find.byType(DownloaderSetupDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
