import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/dialogs/active_tasks_close_dialog.dart';

void main() {
  Future<void> pumpDialogLauncher(
    WidgetTester tester, {
    required Future<void> Function() prepareShutdown,
    required ValueChanged<bool> onResult,
  }) async {
    await tester.pumpWidget(
      MaterialApp(
        localizationsDelegates: const [
          AppLocalizations.delegate,
          GlobalMaterialLocalizations.delegate,
          GlobalWidgetsLocalizations.delegate,
          GlobalCupertinoLocalizations.delegate,
        ],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(
          builder: (context) => TextButton(
            onPressed: () async {
              final result = await showActiveTasksCloseDialog(
                context: context,
                activeTaskCount: 2,
                prepareShutdown: prepareShutdown,
              );
              onResult(result);
            },
            child: const Text('Open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
  }

  testWidgets('cancel leaves shutdown unrequested', (tester) async {
    var shutdownRequests = 0;
    bool? result;
    await pumpDialogLauncher(
      tester,
      prepareShutdown: () async {
        shutdownRequests++;
      },
      onResult: (value) => result = value,
    );

    await tester.tap(find.byKey(const ValueKey('activeTasksCloseCancel')));
    await tester.pumpAndSettle();

    expect(shutdownRequests, 0);
    expect(result, isFalse);
    expect(find.byType(AlertDialog), findsNothing);
  });

  testWidgets('confirm requests shutdown once and shows progress',
      (tester) async {
    final shutdown = Completer<void>();
    var shutdownRequests = 0;
    bool? result;
    await pumpDialogLauncher(
      tester,
      prepareShutdown: () {
        shutdownRequests++;
        return shutdown.future;
      },
      onResult: (value) => result = value,
    );

    await tester.tap(find.byKey(const ValueKey('activeTasksCloseConfirm')));
    await tester.pump();

    expect(shutdownRequests, 1);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    expect(find.byKey(const ValueKey('activeTasksCloseConfirm')), findsNothing);
    expect(result, isNull);

    shutdown.complete();
    await tester.pumpAndSettle();

    expect(result, isTrue);
    expect(find.byType(AlertDialog), findsNothing);
  });

  testWidgets('dialog cannot be dismissed while shutdown is pending',
      (tester) async {
    final shutdown = Completer<void>();
    await pumpDialogLauncher(
      tester,
      prepareShutdown: () => shutdown.future,
      onResult: (_) {},
    );

    await tester.tap(find.byKey(const ValueKey('activeTasksCloseConfirm')));
    await tester.pump();
    await tester.binding.handlePopRoute();
    await tester.pump();

    expect(find.byType(AlertDialog), findsOneWidget);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);

    shutdown.complete();
    await tester.pumpAndSettle();
  });
}
