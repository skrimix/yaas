import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/providers/adb_state.dart';
import 'package:yaas/providers/device_state.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/device/device_actions.dart';
import 'package:yaas/widgets/screens/home_screen.dart';

Uint64 _uint64(int value) => Uint64.fromBigInt(BigInt.from(value));

AdbDevice _device() => AdbDevice(
      name: 'Oculus Quest 2',
      product: 'hollywood',
      serial: '1WMHH830M61203',
      trueSerial: '1WMHH830M61203',
      transportId: '1',
      isWireless: false,
      batteryLevel: 41,
      isCharging: true,
      controllers: const HeadsetControllersInfo(
        left: ControllerInfo(
            batteryLevel: 10, status: ControllerStatusInactive()),
        right:
            ControllerInfo(batteryLevel: 90, status: ControllerStatusActive()),
      ),
      spaceInfo: SpaceInfo(
          total: _uint64(128000000000), available: _uint64(28610000000)),
      installedPackages: const [],
      guardianPaused: false,
      proximityDisabled: true,
      storageConnected: false,
      usbSpeed: '5 Gbps',
    );

Future<StreamController<RustSignalPack<DeviceChangedEvent>>> _pumpHome(
  WidgetTester tester, {
  Size size = const Size(1200, 720),
  Locale locale = const Locale('en'),
  double textScale = 1,
  Brightness brightness = Brightness.dark,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  final events = StreamController<RustSignalPack<DeviceChangedEvent>>();
  final state = DeviceState(deviceEvents: events.stream);
  addTearDown(() async {
    state.dispose();
    await events.close();
  });
  await tester.pumpWidget(MultiProvider(
    providers: [
      ChangeNotifierProvider.value(value: state),
      ChangeNotifierProvider(create: (_) => AdbStateProvider()),
      ChangeNotifierProvider(create: (_) => SettingsState()),
    ],
    child: MaterialApp(
      locale: locale,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
            seedColor: Colors.deepPurple, brightness: brightness),
      ),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context)
            .copyWith(textScaler: TextScaler.linear(textScale)),
        child: child!,
      ),
      home: const Scaffold(body: HomeScreen()),
    ),
  ));
  events
      .add(RustSignalPack(DeviceChangedEvent(device: _device()), Uint8List(0)));
  await tester.pumpAndSettle();
  return events;
}

void main() {
  testWidgets('uses desktop width for the overview and controls',
      (tester) async {
    await _pumpHome(tester);
    expect(tester.takeException(), isNull);
    expect(find.text('41%'), findsOneWidget);
    expect(find.text('28.6 GB'), findsOneWidget);
    final device = tester.getRect(find.text('Oculus Quest 2'));
    final controls = tester.getRect(find.byType(DeviceActionsCard));
    expect(controls.left, greaterThan(device.right));
    expect(controls.right, greaterThan(1100));
  });

  for (final width in [800.0, 360.0]) {
    testWidgets('stacks and scrolls at $width with larger Russian text',
        (tester) async {
      await _pumpHome(tester,
          size: Size(width, 600),
          locale: const Locale('ru'),
          textScale: 1.3,
          brightness: Brightness.light);
      expect(tester.takeException(), isNull);
      final device = tester.getRect(find.text('Oculus Quest 2'));
      final controls = tester.getRect(find.byType(DeviceActionsCard));
      expect(controls.top, greaterThan(device.bottom));
      await tester.ensureVisible(find.text('Свободное место'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.text('Свободное место').hitTestable(), findsOneWidget);
    });
  }

  testWidgets('handles unavailable readings and disconnection', (tester) async {
    final events = await _pumpHome(tester);
    events.add(RustSignalPack(
        DeviceChangedEvent(
            device: _device().copyWith(
          product: 'unrecognized-headset',
          controllers: const HeadsetControllersInfo(),
          spaceInfo: SpaceInfo(total: _uint64(0), available: _uint64(0)),
        )),
        Uint8List(0)));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    expect(find.text('—'), findsNWidgets(3));
    expect(find.text('Not connected'), findsNWidgets(2));
    expect(find.text('0%'), findsNothing);

    events.add(RustSignalPack(const DeviceChangedEvent(), Uint8List(0)));
    await tester.pumpAndSettle();
    expect(find.text('No device connected'), findsOneWidget);
    expect(find.byType(DeviceActionsCard), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
