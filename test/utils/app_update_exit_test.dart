import 'dart:async';
import 'dart:ui';
import 'package:flutter_test/flutter_test.dart';
import 'package:yaas/utils/app_update_exit.dart';

void main() {
  test('candidate stays attached until exit resolves, duplicates are ignored',
      () async {
    final exit = Completer<AppExitResponse>();
    var requests = 0;
    var cancellations = 0;
    final handler = AppUpdateExit(
      requestExit: () {
        requests++;
        return exit.future;
      },
      cancelUpdate: () => cancellations++,
      isExiting: () => false,
    );
    final pending = handler.request('candidate');
    expect(handler.candidateId, 'candidate');
    await handler.request('duplicate');
    expect(requests, 1);
    expect(cancellations, 0);
    exit.complete(AppExitResponse.exit);
    await pending;
    expect(handler.candidateId, isNull);
    expect(cancellations, 0);
  });

  test('cancelling the existing exit confirmation disarms installation',
      () async {
    var cancellations = 0;
    final handler = AppUpdateExit(
      requestExit: () async => AppExitResponse.cancel,
      cancelUpdate: () => cancellations++,
      isExiting: () => false,
    );
    await handler.request('candidate');
    expect(cancellations, 1);
    expect(handler.candidateId, isNull);
  });

  test('a failed exit request leaves the application running', () async {
    var cancellations = 0;
    final handler = AppUpdateExit(
      requestExit: () async => throw StateError('exit failed'),
      cancelUpdate: () => cancellations++,
      isExiting: () => false,
    );
    await handler.request('candidate');
    expect(cancellations, 1);
    expect(handler.candidateId, isNull);
  });

  test('an ordinary exit already in progress cancels the update handoff',
      () async {
    var cancellations = 0;
    final handler = AppUpdateExit(
      requestExit: () async =>
          throw StateError('must not request a second exit'),
      cancelUpdate: () => cancellations++,
      isExiting: () => true,
    );
    await handler.request('candidate');
    expect(cancellations, 1);
    expect(handler.candidateId, isNull);
  });
}
