import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/widgets/common/animated_refresh_button.dart';

RustSignalPack<AdbCommandCompletedEvent> _completion({
  required AdbCommandKind type,
  required String key,
  required bool success,
}) =>
    RustSignalPack(
      AdbCommandCompletedEvent(
        commandType: type,
        commandKey: key,
        success: success,
      ),
      Uint8List(0),
    );

void main() {
  testWidgets('completes only for the matching refresh command',
      (tester) async {
    final completions =
        StreamController<RustSignalPack<AdbCommandCompletedEvent>>();
    addTearDown(completions.close);
    var requests = 0;

    await tester.pumpWidget(
      MaterialApp(
        home: AnimatedRefreshButton(
          tooltip: 'Refresh',
          command: const AdbCommandRefreshPackages(),
          commandType: AdbCommandKind.refreshPackages,
          commandKey: 'packages',
          completionEvents: completions.stream,
          requestSender: (_) => requests++,
        ),
      ),
    );

    await tester.tap(find.byType(IconButton));
    await tester.pump();
    expect(requests, 1);
    expect(find.byKey(const Key('spinning')), findsOneWidget);

    completions.add(_completion(
      type: AdbCommandKind.refreshDevice,
      key: 'packages',
      success: true,
    ));
    await tester.pump();
    expect(find.byKey(const Key('spinning')), findsOneWidget);

    completions.add(_completion(
      type: AdbCommandKind.refreshPackages,
      key: 'packages',
      success: true,
    ));
    await tester.pump();
    expect(find.byKey(const Key('checkmark')), findsOneWidget);
  });
}
