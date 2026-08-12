import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/providers/device_state.dart';
import 'package:yaas/src/bindings/bindings.dart';

Uint64 _uint64(int value) => Uint64.fromBigInt(BigInt.from(value));

InstalledPackage _package(String name) => InstalledPackage(
      uid: _uint64(1),
      system: false,
      packageName: name,
      versionCode: _uint64(1),
      versionName: '1.0',
      label: name,
      launchable: true,
      vr: true,
      size: AppSize(app: _uint64(1), data: _uint64(2), cache: _uint64(3)),
      isPackageRenamed: false,
    );

AdbDevice _device({
  int batteryLevel = 50,
  bool? isCharging,
  List<InstalledPackage>? packages,
}) =>
    AdbDevice(
      name: 'Quest',
      product: 'hollywood',
      serial: 'serial',
      trueSerial: 'serial',
      transportId: '1',
      isWireless: false,
      batteryLevel: batteryLevel,
      isCharging: isCharging,
      controllers: const HeadsetControllersInfo(),
      spaceInfo: SpaceInfo(total: _uint64(100), available: _uint64(50)),
      installedPackages: packages ?? [_package('com.example.app')],
      guardianPaused: false,
      proximityDisabled: false,
      storageConnected: false,
      usbSpeed: '5 Gbps',
    );

RustSignalPack<DeviceChangedEvent> _event(AdbDevice? device) =>
    RustSignalPack(DeviceChangedEvent(device: device), Uint8List(0));

void main() {
  test(
      'ignores identical devices and preserves package lookup for other changes',
      () async {
    final events = StreamController<RustSignalPack<DeviceChangedEvent>>();
    final state = DeviceState(deviceEvents: events.stream);
    addTearDown(() async {
      state.dispose();
      await events.close();
    });
    var notifications = 0;
    state.addListener(() => notifications++);

    final initial = _device();
    events.add(_event(initial));
    await Future<void>.delayed(Duration.zero);
    expect(notifications, 1);
    final packageLookup = state.installedByPackage;

    events.add(_event(initial.copyWith()));
    await Future<void>.delayed(Duration.zero);
    expect(notifications, 1);

    events.add(_event(initial.copyWith(batteryLevel: 75)));
    await Future<void>.delayed(Duration.zero);
    expect(notifications, 2);
    expect(identical(state.installedByPackage, packageLookup), isTrue);

    events.add(_event(initial.copyWith(isCharging: () => true)));
    await Future<void>.delayed(Duration.zero);
    expect(notifications, 3);
    expect(state.isCharging, isTrue);
    expect(identical(state.installedByPackage, packageLookup), isTrue);
  });

  test('rebuilds package lookup when packages change', () async {
    final events = StreamController<RustSignalPack<DeviceChangedEvent>>();
    final state = DeviceState(deviceEvents: events.stream);
    addTearDown(() async {
      state.dispose();
      await events.close();
    });

    events.add(_event(_device()));
    await Future<void>.delayed(Duration.zero);
    final packageLookup = state.installedByPackage;

    events.add(_event(_device(packages: [_package('com.example.other')])));
    await Future<void>.delayed(Duration.zero);

    expect(identical(state.installedByPackage, packageLookup), isFalse);
    expect(state.findInstalled('com.example.other'), isNotNull);
  });
}
