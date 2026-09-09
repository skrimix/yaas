import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_cache_manager/flutter_cache_manager.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:system_date_time_format/system_date_time_format.dart';
import 'package:yaas/main.dart';
import 'package:yaas/providers/adb_state.dart';
import 'package:yaas/providers/app_state.dart';
import 'package:yaas/providers/app_update_state.dart';
import 'package:yaas/providers/cloud_apps_state.dart';
import 'package:yaas/providers/device_state.dart';
import 'package:yaas/providers/settings_state.dart';
import 'package:yaas/providers/task_state.dart';
import 'package:yaas/src/bindings/bindings.dart';
import 'package:yaas/src/l10n/app_localizations.dart';
import 'package:yaas/widgets/cloud_apps/cloud_app_details_dialog.dart';
import 'package:yaas/widgets/cloud_apps/cloud_app_list.dart';
import 'package:yaas/widgets/dialogs/task_list_dialog.dart';

import 'fixtures.dart' as fixtures;
import 'window_frame.dart';

const _size = Size(1264, 711);
const _mediaUrl = 'https://screenshots.invalid/';
const _captureKey = ValueKey('screenshot');

void _emit(String signal, Uint8List message) {
  assignRustSignal[signal]!(message, Uint8List(0));
}

Future<void> _loadFonts() async {
  var directory = File(Platform.resolvedExecutable).parent;
  while (!Directory('${directory.path}/bin/cache/artifacts/material_fonts')
      .existsSync()) {
    if (directory.parent.path == directory.path) {
      throw StateError('Cannot locate the Flutter SDK fonts.');
    }
    directory = directory.parent;
  }
  final fonts = '${directory.path}/bin/cache/artifacts/material_fonts';
  final roboto = FontLoader('Roboto');
  for (final weight in ['Regular', 'Medium', 'Bold']) {
    roboto.addFont(File('$fonts/Roboto-$weight.ttf').readAsBytes().then(
          (bytes) => ByteData.sublistView(bytes),
        ));
  }
  await roboto.load();
  await (FontLoader('MaterialIcons')
        ..addFont(File('$fonts/MaterialIcons-Regular.otf').readAsBytes().then(
              (bytes) => ByteData.sublistView(bytes),
            )))
      .load();
}

// Draw a local cover for the fictional app.
Future<void> _prepareCover(Directory cache) async {
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder);
  const bounds = Rect.fromLTWH(0, 0, 900, 540);
  canvas.drawRect(
    bounds,
    Paint()
      ..shader = const LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [Color(0xff182044), Color(0xff613c80)],
      ).createShader(bounds),
  );
  for (var i = 0; i < 55; i++) {
    canvas.drawCircle(
      Offset((i * 137 % 900).toDouble(), (i * 83 % 540).toDouble()),
      i % 4 == 0 ? 2 : 1,
      Paint()..color = const Color(0xffb8c7f5),
    );
  }
  canvas.drawCircle(
    const Offset(680, 215),
    108,
    Paint()
      ..shader = const LinearGradient(
        colors: [Color(0xffffdcb2), Color(0xffe89197), Color(0xffb769b5)],
      ).createShader(const Rect.fromLTWH(572, 107, 216, 216)),
  );
  canvas.save();
  canvas.translate(680, 215);
  canvas.rotate(-0.4);
  canvas.drawOval(
    const Rect.fromLTWH(-170, -42, 340, 84),
    Paint()
      ..color = const Color(0xffe8c6f5)
      ..style = PaintingStyle.stroke
      ..strokeWidth = 10,
  );
  canvas.restore();
  void text(
      String label, Offset at, double size, FontWeight weight, Color color) {
    final painter = TextPainter(
      text: TextSpan(
        text: label,
        style: TextStyle(
            fontFamily: 'Roboto',
            fontSize: size,
            fontWeight: weight,
            color: color),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    painter.paint(canvas, at);
    painter.dispose();
  }

  text('ORBIT', const Offset(60, 225), 78, FontWeight.bold, Colors.white);
  text('WORKSHOP', const Offset(65, 310), 36, FontWeight.w500,
      const Color(0xffe8c6f5));
  text('A little universe. Made by you.', const Offset(65, 440), 25,
      FontWeight.normal, const Color(0xffd5d9ef));
  final picture = recorder.endRecording();
  final image = await picture.toImage(900, 540);
  try {
    final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
    await File('${cache.path}/orbit.png')
        .writeAsBytes(bytes!.buffer.asUint8List());
  } finally {
    image.dispose();
    picture.dispose();
  }
  final repo =
      JsonCacheInfoRepository.withFile(File('${cache.path}/cache_info.json'));
  await repo.open();
  await repo.insert(CacheObject(
    '${_mediaUrl}thumbnails/org.example.orbit.jpg',
    relativePath: 'orbit.png',
    validTill: DateTime.now().add(const Duration(days: 30)),
  ));
  await repo.close();
}

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('render the six documentation screenshots', (tester) async {
    const destination = String.fromEnvironment('SCREENSHOT_DIR');
    expect(destination, isNotEmpty,
        reason:
            'Use dart run tool/screenshots.dart to install the test transport.');
    tester.view.physicalSize = _size;
    tester.view.devicePixelRatio = 1;
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    debugDisableShadows = false;
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
      debugDefaultTargetPlatformOverride = null;
      debugDisableShadows = true;
    });
    binding.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('system_date_time_format'),
      (call) async => call.method == 'getTimeFormat' ? 'HH:mm' : 'yyyy-MM-dd',
    );
    addTearDown(() => binding.defaultBinaryMessenger.setMockMethodCallHandler(
        const MethodChannel('system_date_time_format'), null));

    await tester.runAsync(() async {
      final output = await Directory(destination).create(recursive: true);
      final cache =
          await Directory.systemTemp.createTemp('yaas-screenshot-media-');
      binding.defaultBinaryMessenger.setMockMethodCallHandler(
        const MethodChannel('plugins.flutter.io/path_provider'),
        (_) async => cache.path,
      );
      addTearDown(() => binding.defaultBinaryMessenger.setMockMethodCallHandler(
          const MethodChannel('plugins.flutter.io/path_provider'), null));
      addTearDown(() async {
        PaintingBinding.instance.imageCache.clear();
        PaintingBinding.instance.imageCache.clearLiveImages();
        await cache.delete(recursive: true);
      });
      await _loadFonts();
      await _prepareCover(cache);
      final frame = await WindowFrame.load();
      addTearDown(frame.dispose);
      final settings = SettingsState();
      final app = AppState();
      final tasks = fixtures.ScreenshotTasks();
      await tester.pumpWidget(MultiProvider(
        providers: [
          ChangeNotifierProvider(create: (_) => DeviceState(), lazy: false),
          ChangeNotifierProvider(
              create: (_) => AdbStateProvider(), lazy: false),
          ChangeNotifierProvider(create: (_) => CloudAppsState(), lazy: false),
          ChangeNotifierProvider<SettingsState>(create: (_) => settings),
          ChangeNotifierProvider(
            create: (_) => AppUpdateState(
              settings: settings,
              events: const Stream.empty(),
              requestSnapshot: () {},
            ),
          ),
          ChangeNotifierProvider<AppState>(create: (_) => app),
          ChangeNotifierProvider<TaskState>(create: (_) => tasks),
        ],
        child: RepaintBoundary(
          key: _captureKey,
          child: SDTFScope(
            child: MaterialApp(
              debugShowCheckedModeBanner: false,
              locale: const Locale('en'),
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              theme: ThemeData(
                useMaterial3: true,
                colorScheme: ColorScheme.fromSeed(
                    seedColor: Colors.deepPurple, brightness: Brightness.dark),
              ),
              home: const SinglePage(),
            ),
          ),
        ),
      ));

      _emit('DeviceChangedEvent',
          DeviceChangedEvent(device: fixtures.device).bincodeSerialize());
      _emit('AdbState', const AdbStateDeviceConnected().bincodeSerialize());
      _emit(
          'SettingsChangedEvent',
          SettingsChangedEvent(
              settings: settings.settings.copyWith(
            localeCode: 'en',
            favoritePackages: ['org.example.orbit', 'org.example.aurora'],
          )).bincodeSerialize());
      _emit(
          'CloudAppsChangedEvent',
          CloudAppsChangedEvent(isLoading: false, apps: fixtures.apps)
              .bincodeSerialize());
      // Let the catalog arrive before availability can trigger an automatic load.
      await tester.pump();
      _emit(
          'DownloaderAvailabilityChanged',
          const DownloaderAvailabilityChanged(
            available: true,
            initializing: false,
            isDonationConfigured: false,
            needsSetup: false,
            capabilities: RepoCapabilities(
                supportsRemoteSelection: false,
                supportsBandwidthLimit: true,
                supportsDownloadModeSelection: true,
                supportsDonationUpload: false),
          ).bincodeSerialize());
      _emit(
          'MediaConfigChanged',
          MediaConfigChanged(mediaBaseUrl: _mediaUrl, cacheDir: cache.path)
              .bincodeSerialize());

      Future<void> settle() async {
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 600));
        await tester.pump(const Duration(milliseconds: 600));
        expect(tester.takeException(), isNull);
      }

      Future<void> capture(String name) async {
        await settle();
        final boundary =
            tester.renderObject<RenderRepaintBoundary>(find.byKey(_captureKey));
        final image = await boundary.toImage();
        try {
          final framed = await frame.render(image);
          try {
            final bytes =
                await framed.toByteData(format: ui.ImageByteFormat.png);
            await File('${output.path}/$name.png')
                .writeAsBytes(bytes!.buffer.asUint8List());
          } finally {
            framed.dispose();
          }
        } finally {
          image.dispose();
        }
      }

      await settle();
      // Asset decoding runs on real I/O even though frame time is controlled.
      await precacheImage(const AssetImage('assets/png/headset/eureka.png'),
          tester.element(find.byType(SinglePage)));
      expect(find.text('Meta Quest 3'), findsOneWidget);
      await capture('1_home');

      app.requestNavigationTo('manage');
      await settle();
      expect(find.text('Canvas Studio'), findsWidgets);
      await capture('2_app_management');

      app.requestNavigationTo('download');
      await settle();
      expect(find.byType(CloudAppList), findsOneWidget);
      await capture('3_download_apps');

      final orbit = find.byWidgetPredicate((widget) =>
          widget is CloudAppListItem &&
          widget.cachedApp.app.packageName == 'org.example.orbit');
      await tester.tap(find.descendant(
          of: orbit, matching: find.byIcon(Icons.info_outline)));
      await tester.pump();
      _emit('AppDetailsResponse', fixtures.details.bincodeSerialize());
      await tester.pump();
      _emit('AppReviewsResponse', fixtures.reviews.bincodeSerialize());
      await settle();
      final imageFinder = find.descendant(
          of: find.byType(CloudAppDetailsDialog),
          matching: find.byType(RawImage));
      for (var attempt = 0; attempt < 100; attempt++) {
        await Future<void>.delayed(const Duration(milliseconds: 20));
        await tester.pump(const Duration(milliseconds: 20));
        if (imageFinder.evaluate().isNotEmpty &&
            tester.widget<RawImage>(imageFinder.first).image != null) {
          break;
        }
      }
      expect(imageFinder, findsOneWidget,
          reason: 'The offline cover must load.');
      expect(tester.widget<RawImage>(imageFinder).image, isNotNull);
      expect(find.text('A lovely place to create'), findsOneWidget);
      await capture('4_app_details');
      Navigator.of(tester.element(find.byType(CloudAppDetailsDialog))).pop();
      await settle();

      tasks.reveal();
      await settle();
      final l10n = AppLocalizations.of(tester.element(find.byType(SinglePage)));
      await tester.tap(find.text(l10n.activeTasks(3)));
      await settle();
      expect(find.byType(TaskListDialog), findsOneWidget);
      await capture('5_tasks_list');
      Navigator.of(tester.element(find.byType(TaskListDialog))).pop();
      await settle();

      app.requestNavigationTo('downloads');
      await tester.pump();
      _emit('GetDownloadsResponse',
          GetDownloadsResponse(entries: fixtures.downloads).bincodeSerialize());
      await settle();
      expect(find.text(fixtures.downloads.first.name), findsOneWidget);
      await capture('6_downloads_list');
      await tester.pumpWidget(const SizedBox.shrink());
    });
    debugDefaultTargetPlatformOverride = null;
    debugDisableShadows = true;
  });
}
