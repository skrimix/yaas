import 'package:flutter/material.dart';
import '../../src/l10n/app_localizations.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart' as filesize;
import '../../providers/device_state.dart';
import '../../providers/cloud_apps_state.dart';
import '../../providers/app_state.dart';
import '../../src/bindings/bindings.dart';
import '../../utils/utils.dart';
import '../common/animated_adb_button.dart';
import '../common/no_device_connected_indicator.dart';
import '../dialogs/animated_uninstall_dialog.dart';
import '../dialogs/backup_options_dialog.dart';

class ManageAppsScreen extends StatefulWidget {
  const ManageAppsScreen({super.key});

  @override
  State<ManageAppsScreen> createState() => _ManageAppsScreenState();
}

enum AppCategory {
  vr,
  other,
  system,
}

// Some of these aren't technically system apps, but they belong to the OS on Quest devices
const hiddenPrefixes = [
  'com.oculus.',
  'com.meta.',
  'com.facebook.',
];

class _ManageAppsScreenState extends State<ManageAppsScreen> {
  AppCategory _selectedCategory = AppCategory.vr;
  static const _animationDuration = Duration(milliseconds: 200);
  static const _cardPadding =
      EdgeInsets.symmetric(horizontal: 16.0, vertical: 4.0);
  static const _listPadding = EdgeInsets.only(bottom: 24);
  static const _segmentPadding =
      EdgeInsets.symmetric(horizontal: 16, vertical: 8);
  static const _buttonPadding =
      EdgeInsets.symmetric(horizontal: 12, vertical: 8);

  final bool _sortAscending = true;
  final ValueNotifier<bool> _isShiftPressedNotifier =
      ValueNotifier<bool>(false);

  // Cache filtered apps to avoid recalculating on every build
  List<InstalledPackage>? _cachedVrApps;
  List<InstalledPackage>? _cachedOtherApps;
  List<InstalledPackage>? _cachedSystemApps;
  List<InstalledPackage>? _installedPackages;
  bool _initialized = false;
  final Map<AppCategory, ScrollController> _scrollControllers = {
    for (final c in AppCategory.values) c: ScrollController(),
  };
  final Map<AppCategory, VoidCallback> _scrollListeners = {};

  VoidCallback _makeScrollListener(
      AppCategory category, ScrollController controller) {
    return () {
      if (controller.hasClients) {
        final idx = AppCategory.values.indexOf(category);
        context
            .read<AppState>()
            .setManageScrollOffset(idx, controller.position.pixels);
      }
    };
  }

  @override
  void initState() {
    super.initState();
    HardwareKeyboard.instance.addHandler(_handleKeyEvent);
    // Persist scroll positions per category
    for (final entry in _scrollControllers.entries) {
      final category = entry.key;
      final controller = entry.value;
      final listener = _makeScrollListener(category, controller);
      controller.addListener(listener);
      _scrollListeners[category] = listener;
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_initialized) return;
    final appState = context.read<AppState>();
    final idx = appState.manageAppsCategoryIndex;
    if (idx >= 0 && idx < AppCategory.values.length) {
      _selectedCategory = AppCategory.values[idx];
    }
    // Restore scroll offset for current category after layout
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final controller = _scrollControllers[_selectedCategory]!;
      if (controller.hasClients) {
        final target = appState.getManageScrollOffset(idx);
        final max = controller.position.maxScrollExtent;
        controller.jumpTo(target.clamp(0.0, max));
      }
    });
    _initialized = true;
  }

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_handleKeyEvent);
    _isShiftPressedNotifier.dispose();
    for (final c in AppCategory.values) {
      final controller = _scrollControllers[c]!;
      final listener = _scrollListeners[c];
      if (listener != null) {
        controller.removeListener(listener);
      }
      controller.dispose();
    }
    super.dispose();
  }

  bool _handleKeyEvent(KeyEvent event) {
    final isShiftPressed = HardwareKeyboard.instance.isShiftPressed;
    if (_isShiftPressedNotifier.value != isShiftPressed) {
      _isShiftPressedNotifier.value = isShiftPressed;
      setState(() {});
    }
    return false; // Pass the event through
  }

  void _updateCachedLists(List<InstalledPackage>? packages) {
    if (_installedPackages == packages) return;
    _installedPackages = packages;
    _cachedVrApps = _getFilteredApps(packages, AppCategory.vr);
    _cachedOtherApps = _getFilteredApps(packages, AppCategory.other);
    _cachedSystemApps = _getFilteredApps(packages, AppCategory.system);
  }

  List<InstalledPackage> _sortApps(List<InstalledPackage> apps) {
    apps.sort((a, b) {
      final appNameA =
          (a.label.isNotEmpty ? a.label : a.packageName).toLowerCase();
      final appNameB =
          (b.label.isNotEmpty ? b.label : b.packageName).toLowerCase();
      int result = appNameA.compareTo(appNameB);
      return _sortAscending ? result : -result;
    });
    return apps;
  }

  List<InstalledPackage> _getFilteredApps(
    List<InstalledPackage>? packages,
    AppCategory category,
  ) {
    if (packages == null) return [];

    var filtered = packages.where((app) {
      final isForceHidden =
          hiddenPrefixes.any((prefix) => app.packageName.startsWith(prefix));
      switch (category) {
        case AppCategory.vr: // VR Apps
          return !isForceHidden && app.vr && !app.system && app.launchable;
        case AppCategory.other: // Other Apps
          return !isForceHidden && !app.vr && !app.system && app.launchable;
        case AppCategory.system: // System & Hidden Apps
          return isForceHidden || app.system || !app.launchable;
      }
    }).toList();

    return _sortApps(filtered);
  }

  String _formatSize(int bytes) {
    return filesize.FileSize.fromBytes(bytes).toString(
      unit: filesize.Unit.auto(
        size: bytes,
        baseType: filesize.BaseType.metric,
      ),
      decimals: 2,
    );
  }

  String _formatAppSize(AppSize size) {
    final totalBytes =
        size.app.toInt() + size.data.toInt() + size.cache.toInt();
    return _formatSize(totalBytes);
  }

  Widget _buildCopyableText(String text, bool showTooltip) {
    return showTooltip
        ? Tooltip(
            message: AppLocalizations.of(context).clickToCopy,
            waitDuration: const Duration(milliseconds: 300),
            child: MouseRegion(
                cursor: SystemMouseCursors.click,
                child: GestureDetector(
                  onTap: () => {
                    copyToClipboard(context, text, description: text),
                  },
                  child: Text(text),
                )),
          )
        : InkWell(
            onTap: () => copyToClipboard(context, text, description: text),
            child: Text(text),
          );
  }

  String _formatAppDetails(InstalledPackage app) {
    final l10n = AppLocalizations.of(context);
    return '${l10n.detailsPackageName} ${app.packageName}\n'
        '${l10n.detailsVersion} ${app.versionName}\n'
        '${l10n.detailsVersionCode} ${app.versionCode}\n'
        '${l10n.detailsIsVr} ${app.vr ? l10n.commonYes : l10n.commonNo}\n'
        '${l10n.detailsIsLaunchable} ${app.launchable ? l10n.commonYes : l10n.commonNo}\n'
        '${l10n.detailsIsSystem} ${app.system ? l10n.commonYes : l10n.commonNo}\n'
        '${l10n.detailsStorageUsage}\n'
        '${l10n.detailsApp} ${_formatSize(app.size.app.toInt())}\n'
        '${l10n.detailsData} ${_formatSize(app.size.data.toInt())}\n'
        '${l10n.detailsCache} ${_formatSize(app.size.cache.toInt())}\n'
        '${l10n.detailsTotal} ${_formatAppSize(app.size)}';
  }

  Widget _buildDetailsRow(String label, String value, bool copyable) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      spacing: 12,
      children: [
        Text(label),
        copyable ? _buildCopyableText(value, true) : Text(value),
      ],
    );
  }

  void _showAppDetailsDialog(BuildContext context, InstalledPackage app) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: _buildCopyableText(app.label, true),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ...() {
              final l10n = AppLocalizations.of(context);
              return [
                _buildDetailsRow(
                    l10n.detailsPackageName, app.packageName, true),
                _buildDetailsRow(l10n.detailsVersion, app.versionName, true),
                _buildDetailsRow(
                    l10n.detailsVersionCode, app.versionCode.toString(), true),
                _buildDetailsRow(l10n.detailsIsVr,
                    app.vr ? l10n.commonYes : l10n.commonNo, false),
                _buildDetailsRow(l10n.detailsIsLaunchable,
                    app.launchable ? l10n.commonYes : l10n.commonNo, false),
                _buildDetailsRow(l10n.detailsIsSystem,
                    app.system ? l10n.commonYes : l10n.commonNo, false),
              ];
            }(),
            const SizedBox(height: 16),
            Text(AppLocalizations.of(context).detailsStorageUsage,
                style: const TextStyle(fontWeight: FontWeight.bold)),
            const SizedBox(height: 4),
            ...() {
              final l10n = AppLocalizations.of(context);
              return [
                _buildDetailsRow(
                    l10n.detailsApp, _formatSize(app.size.app.toInt()), false),
                _buildDetailsRow(l10n.detailsData,
                    _formatSize(app.size.data.toInt()), false),
                _buildDetailsRow(l10n.detailsCache,
                    _formatSize(app.size.cache.toInt()), false),
              ];
            }(),
            const Divider(height: 4),
            _buildDetailsRow(AppLocalizations.of(context).detailsTotal,
                _formatAppSize(app.size), false),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => {
              copyToClipboard(context, _formatAppDetails(app)),
            },
            child: Text(AppLocalizations.of(context).commonCopy),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(AppLocalizations.of(context).commonClose),
          ),
        ],
      ),
    );
  }

  void _showUninstallDialog(BuildContext context, InstalledPackage app) async {
    showDialog(
      context: context,
      builder: (context) => AnimatedUninstallDialog(app: app),
    );
  }

  List<CloudApp> _findMatchingCloudApps(
      InstalledPackage app, List<CloudApp> cloudApps) {
    return cloudApps
        .where((cloudApp) => cloudApp.packageName == app.packageName)
        .toList();
  }

  bool _isNewerVersion(InstalledPackage installedApp, CloudApp cloudApp) {
    return cloudApp.versionCode > installedApp.versionCode.toInt();
  }

  void _showUpdateDialog(BuildContext context, InstalledPackage app,
      List<CloudApp> matchingCloudApps) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(AppLocalizations.of(context).availableVersions),
        content: SizedBox(
          width: 500,
          height: 300,
          child: ValueListenableBuilder<bool>(
              valueListenable: _isShiftPressedNotifier,
              builder: (context, isShiftPressed, _) {
                final l10n = AppLocalizations.of(context);
                return ListView.builder(
                  itemCount: matchingCloudApps.length,
                  itemBuilder: (context, index) {
                    final cloudApp = matchingCloudApps[index];
                    final isNewer = _isNewerVersion(app, cloudApp);
                    final isSameVersion =
                        cloudApp.versionCode == app.versionCode.toInt();
                    final isOlder =
                        cloudApp.versionCode < app.versionCode.toInt();

                    final bool canInstall = isNewer ||
                        (isSameVersion && isShiftPressed) ||
                        (isOlder && isShiftPressed);

                    String tooltipText;
                    if (isNewer) {
                      tooltipText = l10n.installNewerVersion;
                    } else if (isSameVersion) {
                      tooltipText = isShiftPressed
                          ? l10n.reinstallThisVersion
                          : l10n.holdShiftToReinstall;
                    } else {
                      // Older version
                      tooltipText = isShiftPressed
                          ? l10n.downgradeToThisVersion
                          : l10n.holdShiftToDowngrade;
                    }

                    return ListTile(
                      title: Text(cloudApp.fullName),
                      subtitle: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(_formatInstalledToCloudVersion(app, cloudApp)),
                          Text(
                            isNewer
                                ? l10n.newerVersion
                                : isSameVersion
                                    ? l10n.sameVersion
                                    : l10n.olderVersion,
                            style: TextStyle(
                              color: isNewer
                                  ? Colors.green
                                  : isSameVersion
                                      ? Colors.blue
                                      : Colors.red,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ],
                      ),
                      trailing: Tooltip(
                        message: tooltipText,
                        child: Builder(builder: (context) {
                          final theme = Theme.of(context);
                          final bool isDowngradeAction = isOlder && canInstall;
                          final button = FilledButton(
                            style: isDowngradeAction
                                ? ButtonStyle(
                                    backgroundColor: WidgetStatePropertyAll(
                                        theme.colorScheme.error),
                                    foregroundColor: WidgetStatePropertyAll(
                                        theme.colorScheme.onError),
                                  )
                                : null,
                            onPressed: canInstall
                                ? () async {
                                    if (isOlder) {
                                      final confirmed = await _confirmDowngrade(
                                        context,
                                        app,
                                        cloudApp,
                                      );
                                      if (!confirmed) return;
                                    }
                                    if (!context.mounted) return;
                                    Navigator.of(context).pop();
                                    _installCloudApp(cloudApp.fullName);
                                  }
                                : null,
                            child: Text(isNewer
                                ? l10n.update
                                : isOlder
                                    ? l10n.install
                                    : l10n.install),
                          );
                          return button;
                        }),
                      ),
                    );
                  },
                );
              }),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(AppLocalizations.of(context).commonCancel),
          ),
        ],
      ),
    );
  }

  void _installCloudApp(String appFullName) {
    TaskRequest(
      taskType: TaskType.downloadInstall,
      params: TaskParams(cloudAppFullName: appFullName),
    ).sendSignalToRust();
  }

  Widget _buildUpdateButton(BuildContext context, InstalledPackage app) {
    return Consumer<CloudAppsState>(
      builder: (context, cloudAppsState, _) {
        final l10n = AppLocalizations.of(context);
        if (cloudAppsState.apps.isEmpty) {
          return IconButton(
            icon: const Icon(Icons.update),
            tooltip: l10n.checkForUpdates,
            onPressed: null,
          );
        }

        final matchingCloudApps =
            _findMatchingCloudApps(app, cloudAppsState.apps);

        if (matchingCloudApps.isEmpty) {
          return IconButton(
            icon: const Icon(Icons.update),
            tooltip: l10n.noMatchingCloudApp,
            onPressed: null,
          );
        }

        final hasNewerVersion =
            matchingCloudApps.any((cloudApp) => _isNewerVersion(app, cloudApp));

        // Prioritize newer versions
        matchingCloudApps
            .sort((a, b) => b.versionCode.compareTo(a.versionCode));

        final newestCloudApp = matchingCloudApps.first;

        return ValueListenableBuilder<bool>(
            valueListenable: _isShiftPressedNotifier,
            builder: (context, isShiftPressed, _) {
              final l10n = AppLocalizations.of(context);
              if (matchingCloudApps.length == 1) {
                // Single match
                final isSameVersion =
                    newestCloudApp.versionCode == app.versionCode.toInt();
                final isOlder =
                    newestCloudApp.versionCode < app.versionCode.toInt();

                final bool enable = hasNewerVersion ||
                    (isSameVersion && isShiftPressed) ||
                    (isOlder && isShiftPressed);

                String tooltip;
                if (hasNewerVersion) {
                  tooltip = l10n.updateFromTo(
                      '${app.versionCode}', '${newestCloudApp.versionCode}');
                } else if (isSameVersion) {
                  tooltip = isShiftPressed
                      ? l10n.reinstallThisVersion
                      : l10n.holdShiftToReinstall;
                } else {
                  // Older
                  tooltip = isShiftPressed
                      ? l10n.downgradeToThisVersion
                      : l10n.holdShiftToDowngrade;
                }

                final Color? iconColor = hasNewerVersion
                    ? Colors.green
                    : (isOlder && isShiftPressed)
                        ? Colors.red
                        : null;

                return IconButton(
                  icon: Icon(
                    hasNewerVersion ? Icons.system_update : Icons.update,
                    color: iconColor,
                  ),
                  tooltip: tooltip,
                  onPressed: enable
                      ? () async {
                          if (isOlder) {
                            final confirmed = await _confirmDowngrade(
                              context,
                              app,
                              newestCloudApp,
                            );
                            if (!confirmed) return;
                          }
                          _installCloudApp(newestCloudApp.fullName);
                        }
                      : null,
                );
              } else {
                // Multiple matches
                return IconButton(
                  icon: Icon(
                    hasNewerVersion ? Icons.system_update_alt : Icons.update,
                    color: hasNewerVersion || isShiftPressed
                        ? Colors.lightBlue
                        : null,
                  ),
                  tooltip: hasNewerVersion || isShiftPressed
                      ? l10n.availableVersions
                      : l10n.holdShiftToViewVersions,
                  onPressed: hasNewerVersion || isShiftPressed
                      ? () => _showUpdateDialog(context, app, matchingCloudApps)
                      : null,
                );
              }
            });
      },
    );
  }

  // Try to extract something like "v545+1.28.0_4124311467" from CloudApp.fullName
  // Returns null if not found.
  String? _tryParseVersionFromFullName(String fullName) {
    final regex =
        RegExp(r"v(\d+)(?:\+([A-Za-z0-9._-]+))?", caseSensitive: false);
    final match = regex.firstMatch(fullName);
    if (match == null) return null;
    final code = match.group(1);
    final name = match.group(2);
    if (code == null) return null;
    return name == null ? 'v$code' : '$name ($code)';
  }

  String _formatInstalledToCloudVersion(
      InstalledPackage installed, CloudApp cloud) {
    final parsed = _tryParseVersionFromFullName(cloud.fullName);
    final target = parsed ?? 'v${cloud.versionCode}';
    if (installed.versionCode.toInt() == cloud.versionCode) {
      return '${installed.versionName} (${installed.versionCode})';
    }
    return '${installed.versionName} (${installed.versionCode}) → $target';
  }

  Future<bool> _confirmDowngrade(
    BuildContext context,
    InstalledPackage installed,
    CloudApp target,
  ) async {
    final l10n = AppLocalizations.of(context);
    final res = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.downgradeAppTitle),
        content: Text(l10n.downgradeConfirmMessage('${target.versionCode}')),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            style: ButtonStyle(
              backgroundColor:
                  WidgetStatePropertyAll(Theme.of(context).colorScheme.error),
              foregroundColor:
                  WidgetStatePropertyAll(Theme.of(context).colorScheme.onError),
            ),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(l10n.commonConfirm),
          ),
        ],
      ),
    );
    return res ?? false;
  }

  Widget _buildAppList(List<InstalledPackage> apps) {
    if (apps.isEmpty) {
      final l10n = AppLocalizations.of(context);
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(16.0),
          child: Text(l10n.noAppsInCategory),
        ),
      );
    }

    return ListView.builder(
      controller: _scrollControllers[_selectedCategory],
      padding: _listPadding,
      itemCount: apps.length,
      itemBuilder: (context, index) {
        final app = apps[index];
        final appName = app.label.isNotEmpty ? app.label : app.packageName;
        final theme = Theme.of(context);
        final textTheme = theme.textTheme;

        final l10n = AppLocalizations.of(context);
        return Card(
          margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
          child: ListTile(
            title: Text(appName),
            subtitle: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '${app.packageName} • ${app.versionName} (${app.versionCode})',
                  style: textTheme.bodyMedium?.copyWith(
                    color: textTheme.bodyMedium?.color?.withValues(alpha: 0.6),
                  ),
                ),
                Text(
                  _formatAppSize(app.size),
                  style: textTheme.bodySmall?.copyWith(
                    color: textTheme.bodySmall?.color?.withValues(alpha: 0.6),
                  ),
                ),
              ],
            ),
            contentPadding: _cardPadding,
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                IconButton(
                  icon: const Icon(Icons.info_outline),
                  tooltip: l10n.appDetails,
                  onPressed: () => _showAppDetailsDialog(context, app),
                ),
                if (_selectedCategory != AppCategory.system) ...[
                  _buildUpdateButton(context, app),
                  AnimatedAdbButton(
                    icon: Icons.play_arrow,
                    tooltip: l10n.launch,
                    commandType: AdbCommandType.launchApp,
                    commandKey: app.packageName,
                    onPressed: () {
                      AdbRequest(
                              command:
                                  AdbCommandLaunchApp(value: app.packageName),
                              commandKey: app.packageName)
                          .sendSignalToRust();
                    },
                  ),
                  AnimatedAdbButton(
                    icon: Icons.close,
                    tooltip: l10n.forceStop,
                    commandType: AdbCommandType.forceStopApp,
                    commandKey: app.packageName,
                    onPressed: () {
                      AdbRequest(
                              command: AdbCommandForceStopApp(
                                  value: app.packageName),
                              commandKey: app.packageName)
                          .sendSignalToRust();
                    },
                  ),
                  Tooltip(
                    message: l10n.backupApp,
                    child: IconButton(
                      icon: const Icon(Icons.archive_outlined),
                      tooltip: l10n.backup,
                      onPressed: () {
                        showDialog(
                          context: context,
                          builder: (context) => BackupOptionsDialog(app: app),
                        );
                      },
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.delete_outline),
                    tooltip: l10n.uninstall,
                    onPressed: () => _showUninstallDialog(context, app),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        final l10n = AppLocalizations.of(context);
        if (!deviceState.isConnected) {
          return const NoDeviceConnectedIndicator();
        }

        _updateCachedLists(deviceState.device?.installedPackages);

        final currentApps = switch (_selectedCategory) {
              AppCategory.vr => _cachedVrApps,
              AppCategory.other => _cachedOtherApps,
              AppCategory.system => _cachedSystemApps,
            } ??
            [];

        return Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                // TODO: Add refresh button
                Padding(
                  padding: _segmentPadding,
                  child: SegmentedButton<AppCategory>(
                    segments: [
                      ButtonSegment(
                        value: AppCategory.vr,
                        label: Text(
                            l10n.segmentVrApps(_cachedVrApps?.length ?? 0)),
                      ),
                      ButtonSegment(
                        value: AppCategory.other,
                        label: Text(l10n
                            .segmentOtherApps(_cachedOtherApps?.length ?? 0)),
                      ),
                      ButtonSegment(
                        value: AppCategory.system,
                        label: Text(l10n
                            .segmentSystemApps(_cachedSystemApps?.length ?? 0)),
                      ),
                    ],
                    selected: {_selectedCategory},
                    onSelectionChanged: (Set<AppCategory> newSelection) {
                      setState(() {
                        _selectedCategory = newSelection.first;
                      });
                      // Persist selected category
                      final idx = AppCategory.values.indexOf(_selectedCategory);
                      final appState = context.read<AppState>();
                      appState.setManageAppsCategoryIndex(idx);
                      // Restore saved scroll for the newly selected category
                      WidgetsBinding.instance.addPostFrameCallback((_) {
                        final controller =
                            _scrollControllers[_selectedCategory]!;
                        if (controller.hasClients) {
                          final target = appState.getManageScrollOffset(idx);
                          final max = controller.position.maxScrollExtent;
                          controller.jumpTo(target.clamp(0.0, max));
                        }
                      });
                    },
                    style: ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      padding: WidgetStateProperty.all<EdgeInsets>(
                        _buttonPadding,
                      ),
                    ),
                  ),
                ),
                Expanded(
                  child: AnimatedSwitcher(
                    duration: _animationDuration,
                    transitionBuilder: (child, animation) {
                      return FadeTransition(
                        opacity: animation,
                        child: child,
                      );
                    },
                    child: Container(
                      key: ValueKey<AppCategory>(_selectedCategory),
                      child: _buildAppList(currentApps),
                    ),
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
