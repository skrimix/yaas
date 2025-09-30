import 'dart:ui';

import 'package:desktop_window/desktop_window.dart';
import 'package:flutter/material.dart';
import 'package:dynamic_color/dynamic_color.dart';
import 'utils/theme_utils.dart' as app_theme;
import 'package:provider/provider.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:video_player_media_kit/video_player_media_kit.dart';
import 'src/l10n/app_localizations.dart';
import 'package:rinf/rinf.dart';
import 'package:toastification/toastification.dart'; // TODO: find an alternative
import 'src/bindings/bindings.dart' as messages;
import 'widgets/screens/error_screen.dart';
import 'widgets/common/drag_drop_overlay.dart';
import 'providers/device_state.dart';
import 'providers/adb_state.dart';
import 'providers/cloud_apps_state.dart';
import 'providers/task_state.dart';
import 'providers/settings_state.dart';
import 'providers/log_state.dart';
import 'providers/app_state.dart';
import 'navigation.dart';
import 'widgets/common/status_bar.dart';

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
      backgroundColor: null,
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

  // Receive backend build/version info
  messages.AppVersionInfo.rustSignalStream.listen((pack) {
    final info = pack.message;
    final appState = YAASApp.navigatorKey.currentContext?.read<AppState>();
    appState?.setBackendVersionInfo(info);
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
  Color? _linuxKdeAccent;
  bool _triedReadLinuxAccent = false;

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

    // Best-effort read of KDE accent color on Linux
    _maybeLoadLinuxAccent();
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
      child: DynamicColorBuilder(
        builder: (ColorScheme? lightDynamic, ColorScheme? darkDynamic) {
          final settings = context.watch<SettingsState>().settings;
          final seed = app_theme.seedFromKey(settings.seedColorKey);

          // If system color is enabled, prefer dynamic ColorScheme; otherwise use seed
          final ColorScheme lightScheme;
          final ColorScheme darkScheme;
          if (settings.useSystemColor) {
            final fallbackSeed = _linuxKdeAccent ?? seed;
            lightScheme = lightDynamic ??
                ColorScheme.fromSeed(
                    seedColor: fallbackSeed, brightness: Brightness.light);
            darkScheme = darkDynamic ??
                ColorScheme.fromSeed(
                    seedColor: fallbackSeed, brightness: Brightness.dark);
          } else {
            lightScheme = ColorScheme.fromSeed(
                seedColor: seed, brightness: Brightness.light);
            darkScheme = ColorScheme.fromSeed(
                seedColor: seed, brightness: Brightness.dark);
          }

          ThemeMode themeMode;
          switch (settings.themePreference) {
            case messages.ThemePreference.auto:
              themeMode = ThemeMode.system;
              break;
            case messages.ThemePreference.light:
              themeMode = ThemeMode.light;
              break;
            case messages.ThemePreference.dark:
              themeMode = ThemeMode.dark;
              break;
          }

          return MaterialApp(
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
            themeMode: themeMode,
            theme: ThemeData(colorScheme: lightScheme, useMaterial3: true),
            darkTheme: ThemeData(colorScheme: darkScheme, useMaterial3: true),
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
          );
        },
      ),
    );
  }

  Future<void> _maybeLoadLinuxAccent() async {
    if (_triedReadLinuxAccent) return;
    _triedReadLinuxAccent = true;
    final color = await app_theme.readKdeAccent();
    if (!mounted) return;
    if (color != null) {
      setState(() => _linuxKdeAccent = color);
    }
  }
}

class SinglePage extends StatefulWidget {
  const SinglePage({super.key});

  @override
  State<SinglePage> createState() => _SinglePageState();
}

class _SinglePageState extends State<SinglePage> {
  var pageIndex = 0;
  var _startupApplied = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final settingsState = context.watch<SettingsState>();
    final pageDefinitions = AppPageRegistry.pages;
    final destinations = pageDefinitions
        .map((page) => page.toNavigationDestination(l10n))
        .toList();

    if (!_startupApplied && settingsState.hasLoaded) {
      final initialIndex = _indexForStartupPage(
        settingsState.settings.startupPageKey,
        pageDefinitions,
      );

      if (initialIndex != null && initialIndex != pageIndex) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!mounted) return;
          setState(() {
            pageIndex = initialIndex;
          });
        });
      }
      _startupApplied = true;
    }

    final labelType =
        switch (settingsState.settings.navigationRailLabelVisibility) {
      messages.NavigationRailLabelVisibility.all => NavigationRailLabelType.all,
      messages.NavigationRailLabelVisibility.selected =>
        NavigationRailLabelType.selected,
    };
    return Scaffold(
        body: Row(
      children: [
        SafeArea(
            child: NavigationRail(
          backgroundColor: Theme.of(context).colorScheme.surfaceContainerLow,
          labelType: labelType,
          destinations: destinations,
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
                    child: pageDefinitions[pageIndex].buildContent(),
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

  int? _indexForStartupPage(
    String pageKey,
    List<AppPageDefinition> definitions,
  ) {
    for (var i = 0; i < definitions.length; i++) {
      if (definitions[i].key == pageKey) {
        return i;
      }
    }
    return null;
  }
}
