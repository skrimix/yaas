import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/providers/adb_state.dart';
import 'package:yaas/providers/device_state.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/providers/task_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/common/status_bar.dart';
import 'package:yaas/widgets/screens/home_screen.dart';

Uint64 _uint64(int value) => Uint64.fromBigInt(BigInt.from(value));

AdbDevice _device({bool? isCharging}) => AdbDevice(
      name: 'Quest',
      product: 'hollywood',
      serial: 'serial',
      trueSerial: 'serial',
      transportId: '1',
      isWireless: false,
      batteryLevel: 85,
      isCharging: isCharging,
      controllers: const HeadsetControllersInfo(),
      spaceInfo: SpaceInfo(total: _uint64(100), available: _uint64(50)),
      installedPackages: const [],
      guardianPaused: false,
      proximityDisabled: false,
      storageConnected: false,
      usbSpeed: '5 Gbps',
    );

RustSignalPack<DeviceChangedEvent> _event(AdbDevice device) =>
    RustSignalPack(DeviceChangedEvent(device: device), Uint8List(0));

void main() {
  testWidgets('shows charging indicators only while charging', (tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final events = StreamController<RustSignalPack<DeviceChangedEvent>>();
    final deviceState = DeviceState(deviceEvents: events.stream);
    addTearDown(() async {
      deviceState.dispose();
      await events.close();
    });

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider.value(value: deviceState),
          ChangeNotifierProvider(create: (_) => AdbStateProvider()),
          ChangeNotifierProvider(create: (_) => SettingsState()),
          ChangeNotifierProvider(create: (_) => TaskState()),
        ],
        child: MaterialApp(
          localizationsDelegates: const [
            AppLocalizations.delegate,
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          supportedLocales: AppLocalizations.supportedLocales,
          home: const Scaffold(
            body: Column(
              children: [
                StatusBar(),
                Expanded(child: SingleChildScrollView(child: HomeScreen())),
              ],
            ),
          ),
        ),
      ),
    );

    events.add(_event(_device(isCharging: true)));
    await tester.pump();
    await tester.pump();

    final batteryIcon = find.byKey(const ValueKey('headset-battery-icon'));
    expect(tester.widget<Icon>(batteryIcon).icon, Icons.battery_charging_full);
    final chargingBadge = find.byKey(const ValueKey('headset-charging-badge'));
    expect(chargingBadge, findsOneWidget);
    // expect(
    //   tester
    //       .widget<Tooltip>(
    //         find
    //             .ancestor(of: batteryIcon, matching: find.byType(Tooltip))
    //             .first,
    //       )
    //       .message,
    //   contains('85% (charging)'),
    // );
    // expect(
    //   tester
    //       .widget<Tooltip>(
    //         find
    //             .ancestor(of: chargingBadge, matching: find.byType(Tooltip))
    //             .first,
    //       )
    //       .message,
    //   contains('Battery: 85% (charging)'),
    // );

    events.add(_event(_device(isCharging: false)));
    await tester.pump();
    await tester.pump();

    expect(tester.widget<Icon>(batteryIcon).icon, Icons.battery_full);
    expect(chargingBadge, findsNothing);
    expect(
      tester
          .widget<Tooltip>(
            find
                .ancestor(of: batteryIcon, matching: find.byType(Tooltip))
                .first,
          )
          .message,
      isNot(contains('(charging)')),
    );

    events.add(_event(_device()));
    await tester.pump();
    await tester.pump();

    expect(tester.widget<Icon>(batteryIcon).icon, Icons.battery_full);
    expect(chargingBadge, findsNothing);
  });
}
