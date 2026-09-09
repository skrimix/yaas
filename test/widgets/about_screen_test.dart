import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:provider/provider.dart';
import 'package:yaas/providers/app_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/screens/about_screen.dart';

void main() {
  for (final locale in ['en', 'ru']) {
    for (final channel in ['development', 'nightly', 'stable']) {
      testWidgets('About identifies $channel builds in $locale',
          (tester) async {
        PackageInfo.setMockInitialValues(
          appName: 'YAAS',
          packageName: 'yaas',
          version: '1.2.3',
          buildNumber: '4',
          buildSignature: '',
        );
        final state = AppState();
        addTearDown(state.dispose);
        await tester.pumpWidget(
          ChangeNotifierProvider.value(
            value: state,
            child: MaterialApp(
              locale: Locale(locale),
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              home: const Scaffold(body: AboutScreen()),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text('YAAS 1.2.3+4'), findsOneWidget);

        state.setCoreVersionInfo(AppVersionInfo(
          appVersion: '1.2.3',
          buildNumber: '4',
          releaseChannel: channel,
          runNumber: channel == 'development' ? null : '123',
          runAttempt: channel == 'development' ? null : '2',
          coreVersion: '0.1.0',
          profile: 'release',
          rustcVersion: 'rustc test',
          builtTimeUtc: 'test',
          gitCommitHashShort: 'abcdef0',
          gitDirty: false,
        ));
        await tester.pumpAndSettle();
        final l10n =
            AppLocalizations.of(tester.element(find.byType(AboutScreen)));
        final label = switch (channel) {
          'stable' => l10n.buildChannelStable,
          'nightly' => l10n.buildChannelNightly,
          _ => l10n.buildChannelDevelopment,
        };
        final expectedChannel = channel == 'development'
            ? label
            : '$label · ${l10n.ciBuildIdentity('123', '2')}';
        expect(find.text(expectedChannel), findsOneWidget);
        expect(find.text('YAAS 1.2.3+4'), findsOneWidget);
        expect(find.text('Core v0.1.0'), findsNothing);
        expect(find.text('abcdef0'), findsOneWidget);
        expect(find.text(l10n.aboutCoreDetails), findsOneWidget);
        expect(find.text(l10n.aboutBuiltAt('test')), findsOneWidget);
        expect(find.text(l10n.aboutBuildProfile('release')), findsOneWidget);
        expect(find.text(l10n.aboutCompiler('rustc test')), findsOneWidget);
        expect(find.text('Core v0.1.0'), findsNothing);
        expect(tester.takeException(), isNull);
      });
    }
  }
}
