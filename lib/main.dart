import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import 'package:toastification/toastification.dart';
import './messages/all.dart';
import 'widgets/home.dart';
import 'widgets/status_bar.dart';
import 'widgets/manage_apps.dart';
import 'widgets/error_screen.dart';
import 'providers/device_state.dart';

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
  await initializeRust(assignRustSignal);

  runApp(
    MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => DeviceState()),
        ChangeNotifierProvider(create: (_) => AppState()),
      ],
      child: const RqlApp(),
    ),
  );

  AdbResponse.rustSignalStream.listen((response) {
    final type = response.message.success
        ? ToastificationType.success
        : ToastificationType.error;

    String title;
    switch (response.message.command) {
      case AdbCommand.ADB_COMMAND_LAUNCH_APP:
        title = response.message.success ? 'App Launched' : 'Launch Failed';
        break;
      case AdbCommand.ADB_COMMAND_FORCE_STOP_APP:
        title = response.message.success ? 'App Stopped' : 'Stop Failed';
        break;
      case AdbCommand.ADB_COMMAND_INSTALL_APK:
        title = response.message.success
            ? 'Installation Complete'
            : 'Installation Failed';
        break;
      case AdbCommand.ADB_COMMAND_UNINSTALL_PACKAGE:
        title = response.message.success
            ? 'Uninstallation Complete'
            : 'Uninstallation Failed';
        break;
      default:
        title = response.message.success ? 'Success' : 'Error';
    }

    toastification.show(
      type: type,
      style: ToastificationStyle.flat,
      title: Text(title),
      description: Text(response.message.message),
      autoCloseDuration: const Duration(seconds: 3),
      backgroundColor: colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  });

  RustPanic.rustSignalStream.listen((panic) {
    final appState = RqlApp.navigatorKey.currentContext?.read<AppState>();
    if (appState != null) {
      appState.setPanicMessage(panic.message.message);
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
        return AppExitResponse.exit;
      },
    );
  }

  @override
  void dispose() {
    _listener.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ToastificationWrapper(
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
            return const SinglePage();
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
        icon: Icons.get_app, label: 'Download', content: Text('Download Apps')),
    Destination(
        icon: Icons.arrow_circle_down,
        label: 'Sideload',
        content: Text('Local Sideload')),
    Destination(
        icon: Icons.settings, label: 'Settings', content: Text('Settings')),
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
