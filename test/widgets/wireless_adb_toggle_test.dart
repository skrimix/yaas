import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/providers/device_state.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/device/device_actions.dart';

AdbDevice _device({bool wireless = false, bool? enabled}) => AdbDevice(
      product: 'hollywood',
      serial: wireless ? '192.168.1.10:5555' : 'serial',
      trueSerial: 'serial',
      transportId: '1',
      isWireless: wireless,
      wirelessAdbEnabled: enabled,
      batteryLevel: 85,
      controllers: const HeadsetControllersInfo(),
      spaceInfo: SpaceInfo(
        total: Uint64.fromBigInt(BigInt.from(100)),
        available: Uint64.fromBigInt(BigInt.from(50)),
      ),
      installedPackages: const [],
      guardianPaused: false,
      proximityDisabled: false,
      storageConnected: false,
    );

void main() {
  testWidgets('Wireless ADB reflects the mode on USB and wireless connections',
      (tester) async {
    final events = StreamController<RustSignalPack<DeviceChangedEvent>>();
    final state = DeviceState(deviceEvents: events.stream);
    addTearDown(() async {
      state.dispose();
      await events.close();
    });
    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider.value(value: state),
          ChangeNotifierProvider(create: (_) => SettingsState()),
        ],
        child: MaterialApp(
          localizationsDelegates: const [
            AppLocalizations.delegate,
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          supportedLocales: AppLocalizations.supportedLocales,
          home: const Scaffold(body: DeviceActionsCard()),
        ),
      ),
    );

    final toggle = find.descendant(
      of: find.ancestor(
        of: find.text('Wireless ADB'),
        matching: find.byType(Row),
      ),
      matching: find.byType(Switch),
    );
    expect(toggle, findsNothing);

    Future<void> update(AdbDevice? device) async {
      events.add(RustSignalPack(
        DeviceChangedEvent(device: device),
        Uint8List(0),
      ));
      await tester.pump();
      await tester.pump();
    }

    await update(_device(enabled: false));
    expect(tester.widget<Switch>(toggle).value, isFalse);
    expect(tester.widget<Switch>(toggle).onChanged, isNotNull);

    await update(_device(enabled: true));
    expect(tester.widget<Switch>(toggle).value, isTrue);
    expect(tester.widget<Switch>(toggle).onChanged, isNotNull);

    await update(_device(wireless: true, enabled: true));
    expect(tester.widget<Switch>(toggle).value, isTrue);
    expect(tester.widget<Switch>(toggle).onChanged, isNotNull);

    await update(_device(wireless: true));
    expect(tester.widget<Switch>(toggle).value, isTrue);

    await update(_device());
    expect(tester.widget<Switch>(toggle).onChanged, isNull);

    await update(null);
    expect(toggle, findsNothing);
  });
}
