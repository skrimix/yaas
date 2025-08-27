import 'dart:ui';

import 'package:desktop_window/desktop_window.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import 'package:toastification/toastification.dart'; // TODO: find an alternative
import './src/bindings/bindings.dart' as messages;
import 'widgets/home.dart';
import 'widgets/status_bar.dart';
import 'widgets/manage_apps.dart';
import 'widgets/local_sideload.dart';
import 'widgets/error_screen.dart';
import 'widgets/settings_screen.dart';
import 'widgets/logs_screen.dart';
import 'widgets/drag_drop_overlay.dart';
import 'providers/device_state.dart';
import 'providers/adb_state.dart';
import 'providers/cloud_apps_state.dart';
import 'providers/task_state.dart';
import 'providers/settings_state.dart';
import 'providers/log_state.dart';
import 'widgets/download_apps.dart';

final colorScheme = ColorScheme.fromSeed(
  seedColor: Colors.deepPurple,
  brightness: Brightness.dark,
);

class AppState extends ChangeNotifier {
  String? _panicMessage;
  String? get panicMessage => _panicMessage;

  void setPanicMessage(String message) {
    _panicMessage = message;
    notifyListeners();
  }
}

void main() async {
  await initializeRust(messages.assignRustSignal);

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
      child: const RqlApp(),
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
    final appState = RqlApp.navigatorKey.currentContext?.read<AppState>();
    if (appState != null) {
      appState.setPanicMessage(panic.message.message);
      finalizeRust(); // Rust side is in an undefined state, shut it down
    }
  });
}

class RqlApp extends StatefulWidget {
  static final navigatorKey = GlobalKey<NavigatorState>();

  const RqlApp({super.key});

  @override
  State<RqlApp> createState() => _RqlAppState();
}

class _RqlAppState extends State<RqlApp> {
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
        navigatorKey: RqlApp.navigatorKey,
        title: 'RQL',
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
  final _destinations = [
    Destination(icon: Icons.home, label: 'Home', content: const Home()),
    Destination(icon: Icons.apps, label: 'Manage', content: const ManageApps()),
    Destination(
        icon: Icons.get_app, label: 'Download', content: const DownloadApps()),
    Destination(
        icon: Icons.arrow_circle_down,
        label: 'Sideload',
        content: const LocalSideload()),
    Destination(
        icon: Icons.settings,
        label: 'Settings',
        content: const SettingsScreen()),
    Destination(
        icon: Icons.terminal, label: 'Logs', content: const LogsScreen()),
    Destination(icon: Icons.info, label: 'About', content: Text('About')),
  ];

  var pageIndex = 0;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
        body: Row(
      children: [
        SafeArea(
            child: NavigationRail(
          backgroundColor: Theme.of(context).colorScheme.surfaceContainerLow,
          labelType: NavigationRailLabelType.selected,
          destinations: _buildNavDestinations(),
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
                    child: _destinations[pageIndex].content,
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

  List<NavigationRailDestination> _buildNavDestinations() {
    return _destinations
        .map((destination) => destination.navigationDestination)
        .toList();
  }
}
