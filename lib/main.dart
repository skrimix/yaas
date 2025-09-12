import 'dart:ui';

import 'package:desktop_window/desktop_window.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:video_player_media_kit/video_player_media_kit.dart';
import 'src/l10n/app_localizations.dart';
import 'package:rinf/rinf.dart';
import 'package:toastification/toastification.dart'; // TODO: find an alternative
import 'src/bindings/bindings.dart' as messages;
import 'widgets/screens/home.dart';
import 'widgets/common/status_bar.dart';
import 'widgets/app_management/manage_apps.dart';
import 'widgets/app_management/local_sideload.dart';
import 'widgets/screens/error_screen.dart';
import 'widgets/screens/settings_screen.dart';
import 'widgets/screens/logs_screen.dart';
import 'widgets/screens/backups_screen.dart';
import 'widgets/screens/downloads_screen.dart';
import 'widgets/common/drag_drop_overlay.dart';
import 'providers/device_state.dart';
import 'providers/adb_state.dart';
import 'providers/cloud_apps_state.dart';
import 'providers/task_state.dart';
import 'providers/settings_state.dart';
import 'providers/log_state.dart';
import 'widgets/app_management/download_apps.dart';
import 'providers/app_state.dart';

final colorScheme = ColorScheme.fromSeed(
  seedColor: Colors.deepPurple,
  brightness: Brightness.dark,
);

void main() async {
  await initializeRust(messages.assignRustSignal);

  VideoPlayerMediaKit.ensureInitialized(
    macOS: true,
    windows: true,
    linux: true,
  );

  runApp(
    MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => DeviceState()),
        ChangeNotifierProvider(create: (_) => AdbStateProvider()),
        ChangeNotifierProvider(create: (_) => AppState()),
        ChangeNotifierProvider(create: (_) => CloudAppsState()),
        ChangeNotifierProvider(create: (_) => TaskState()),
        ChangeNotifierProvider(create: (_) => SettingsState()),
        ChangeNotifierProvider(create: (_) => LogState()),
      ],
      child: const YAASApp(),
    ),
  );
  await DesktopWindow.setMinWindowSize(const Size(800, 600));

  messages.Toast.rustSignalStream.listen((message) {
    final toast = message.message;
    toastification.show(
      type: toast.error ? ToastificationType.error : ToastificationType.success,
      title: Text(toast.title),
      description: Text(toast.description),
      autoCloseDuration: Duration(milliseconds: toast.duration ?? 3000),
      style: ToastificationStyle.flat,
      backgroundColor: colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  });

  messages.RustPanic.rustSignalStream.listen((panic) {
    final appState = YAASApp.navigatorKey.currentContext?.read<AppState>();
    if (appState != null) {
      appState.setPanicMessage(panic.message.message);
      finalizeRust(); // Rust side is in an undefined state, shut it down
    }
  });
}

class YAASApp extends StatefulWidget {
  static final navigatorKey = GlobalKey<NavigatorState>();

  const YAASApp({super.key});

  @override
  State<YAASApp> createState() => _YAASAppState();
}

class _YAASAppState extends State<YAASApp> {
  late final AppLifecycleListener _listener;

  @override
  void initState() {
    super.initState();
    _listener = AppLifecycleListener(
      onExitRequested: () async {
        finalizeRust();
        // TODO: Cooperative shutdown
        return AppExitResponse.exit;
      },
    );

    // Kick off the initial load of some providers
    WidgetsBinding.instance.addPostFrameCallback((_) {
      context.read<CloudAppsState>().load();
      context.read<SettingsState>().load();
    });
  }

  @override
  void dispose() {
    _listener.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ToastificationWrapper(
      config: ToastificationConfig(
        // Increase the bottom margin for the toast notifications
        marginBuilder: (context, alignment) {
          final y = alignment.resolve(Directionality.of(context)).y;

          return switch (y) {
            <= -0.5 => const EdgeInsets.only(top: 12),
            >= 0.5 => const EdgeInsets.only(bottom: 16),
            _ => EdgeInsets.zero,
          };
        },
      ),
      child: MaterialApp(
        navigatorKey: YAASApp.navigatorKey,
        onGenerateTitle: (context) => AppLocalizations.of(context).appTitle,
        locale: context.watch<SettingsState>().locale,
        localizationsDelegates: const [
          AppLocalizations.delegate,
          GlobalMaterialLocalizations.delegate,
          GlobalWidgetsLocalizations.delegate,
          GlobalCupertinoLocalizations.delegate,
        ],
        supportedLocales: AppLocalizations.supportedLocales,
        theme: ThemeData(
          colorScheme: colorScheme,
          useMaterial3: true,
        ),
        home: Consumer<AppState>(
          builder: (context, appState, child) {
            if (appState.panicMessage != null) {
              return ErrorScreen(message: appState.panicMessage!);
            }
            return DragDropOverlay(
              child: const SinglePage(),
            );
          },
        ),
      ),
    );
  }
}

class Destination {
  final IconData icon;
  final String label;
  final Widget content;

  Destination({required this.icon, required this.label, required this.content});

  NavigationRailDestination get navigationDestination =>
      NavigationRailDestination(icon: Icon(icon), label: Text(label));
}

class SinglePage extends StatefulWidget {
  const SinglePage({super.key});

  @override
  State<SinglePage> createState() => _SinglePageState();
}

class _SinglePageState extends State<SinglePage> {
  var pageIndex = 0;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final destinations = [
      Destination(icon: Icons.home, label: l10n.navHome, content: const Home()),
      Destination(
          icon: Icons.apps, label: l10n.navManage, content: const ManageApps()),
      Destination(
          icon: Icons.get_app,
          label: l10n.navDownload,
          content: const DownloadApps()),
      Destination(
          icon: Icons.download_done_outlined,
          label: l10n.navDownloads,
          content: const DownloadsScreen()),
      Destination(
          icon: Icons.arrow_circle_down,
          label: l10n.navSideload,
          content: const LocalSideload()),
      Destination(
          icon: Icons.backup_outlined,
          label: l10n.navBackups,
          content: const BackupsScreen()),
      Destination(
          icon: Icons.settings,
          label: l10n.navSettings,
          content: const SettingsScreen()),
      Destination(
          icon: Icons.terminal,
          label: l10n.navLogs,
          content: const LogsScreen()),
      Destination(
          icon: Icons.info, label: l10n.navAbout, content: Text(l10n.navAbout)),
    ];
    return Scaffold(
        body: Row(
      children: [
        SafeArea(
            child: NavigationRail(
          backgroundColor: Theme.of(context).colorScheme.surfaceContainerLow,
          labelType: NavigationRailLabelType.selected,
          destinations: _buildNavDestinations(destinations),
          selectedIndex: pageIndex,
          onDestinationSelected: (index) => setState(() => pageIndex = index),
        )),
        Expanded(
          child: Column(
            children: [
              Expanded(
                child: AnimatedSwitcher(
                  duration: const Duration(milliseconds: 100),
                  child: SizedBox.expand(
                    key: ValueKey(pageIndex),
                    child: destinations[pageIndex].content,
                  ),
                ),
              ),
              const StatusBar(),
            ],
          ),
        ),
      ],
    ));
  }

  List<NavigationRailDestination> _buildNavDestinations(
      List<Destination> destinations) {
    return destinations
        .map((destination) => destination.navigationDestination)
        .toList();
  }
}
