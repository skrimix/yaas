import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:rinf/rinf.dart';
import 'package:yaas/providers/cloud_apps_state.dart';
import 'package:yaas/src/bindings/bindings.dart';

RustSignalPack<T> pack<T>(T value) => RustSignalPack(value, Uint8List(0));

void main() {
  late StreamController<RustSignalPack<CloudAppsChangedEvent>> catalog;
  late StreamController<RustSignalPack<DownloaderAvailabilityChanged>>
      availability;
  late CloudAppsState state;

  setUp(() {
    catalog = StreamController();
    availability = StreamController();
    state = CloudAppsState(
      catalogEvents: catalog.stream,
      availabilityEvents: availability.stream,
      mediaEvents: const Stream.empty(),
    );
  });

  tearDown(() async {
    state.dispose();
    await catalog.close();
    await availability.close();
  });

  Future<void> seed() async {
    catalog.add(pack(CloudAppsChangedEvent(
      isLoading: false,
      apps: [
        CloudApp(
          appName: 'App',
          fullName: 'App v1',
          packageName: 'com.example.app',
          truePackageName: 'com.example.app',
          versionCode: 1,
          lastUpdated: '',
          size: Uint64.fromBigInt(BigInt.from(100)),
        )
      ],
      donationBlacklist: const ['com.example.blocked'],
    )));
    await Future<void>.delayed(Duration.zero);
  }

  test('retains apps, indexes, and blacklist after a refresh error', () async {
    await seed();
    catalog.add(pack(const CloudAppsChangedEvent(isLoading: true)));
    await Future<void>.delayed(Duration.zero);
    expect(state.isLoading, isTrue);
    expect(state.apps, hasLength(1));
    catalog.add(
        pack(const CloudAppsChangedEvent(isLoading: false, error: 'Offline')));
    await Future<void>.delayed(Duration.zero);
    expect(state.error, 'Offline');
    expect(state.apps.single.fullName, 'App v1');
    expect(state.newestVersionCodeForPackage('com.example.app'), 1);
    expect(state.isDonationBlacklisted('com.example.blocked'), isTrue);
    expect(state.isLoading, isFalse);
  });

  test('clears apps and blacklist together when the remote changes', () async {
    await seed();
    catalog.add(pack(const CloudAppsChangedEvent(
        isLoading: true, apps: [], donationBlacklist: [])));
    await Future<void>.delayed(Duration.zero);
    expect(state.apps, isEmpty);
    expect(state.donationBlacklist, isEmpty);
    expect(state.newestVersionCodeForPackage('com.example.app'), isNull);
  });

  test('clears the catalog when switching sources', () async {
    await seed();
    availability.add(pack(const DownloaderAvailabilityChanged(
      available: false,
      initializing: true,
      isDonationConfigured: false,
      needsSetup: false,
      capabilities: RepoCapabilities(
        supportsRemoteSelection: false,
        supportsBandwidthLimit: false,
        supportsDownloadModeSelection: true,
        supportsDonationUpload: false,
      ),
    )));
    await Future<void>.delayed(Duration.zero);
    expect(state.apps, isEmpty);
    expect(state.donationBlacklist, isEmpty);
    expect(state.error, isNull);
    expect(state.isLoading, isFalse);
  });
}
