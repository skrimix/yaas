import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import './messages/all.dart';
import 'widgets/home.dart';
import 'widgets/status_bar.dart';
import 'widgets/manage_apps.dart';
import 'providers/device_state.dart';

void main() async {
  await initializeRust(assignRustSignal);
  runApp(
    ChangeNotifierProvider(
      create: (_) => DeviceState(),
      child: const RqlApp(),
    ),
  );
}

class RqlApp extends StatefulWidget {
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
        finalizeRust(); // Shut down the `tokio` Rust runtime.
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
    return MaterialApp(
      title: 'RQL',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
            seedColor: Colors.deepPurple, brightness: Brightness.dark),
        useMaterial3: true,
      ),
      home: const SinglePage(),
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
