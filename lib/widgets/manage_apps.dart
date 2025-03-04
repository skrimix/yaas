import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:toastification/toastification.dart';
import '../providers/device_state.dart';
import '../messages/all.dart';

class ManageApps extends StatefulWidget {
  const ManageApps({super.key});

  @override
  State<ManageApps> createState() => _ManageAppsState();
}

enum AppCategory {
  vr,
  other,
  system,
}

// Some of these aren't technically system apps, but they belong to the OS
const hiddenPrefixes = [
  'com.oculus.',
  'com.meta.',
  'com.facebook.',
];

class _ManageAppsState extends State<ManageApps> {
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

  // Cache filtered apps to avoid recalculating on every build
  List<InstalledPackage>? _cachedVrApps;
  List<InstalledPackage>? _cachedOtherApps;
  List<InstalledPackage>? _cachedSystemApps;
  List<InstalledPackage>? _installedPackages;

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
    return FileSize.fromBytes(bytes).toString(
      unit: Unit.auto(
        size: bytes,
        baseType: BaseType.metric,
      ),
      decimals: 2,
    );
  }

  String _formatAppSize(AppSize size) {
    final totalBytes =
        size.app.toInt() + size.data.toInt() + size.cache.toInt();
    return _formatSize(totalBytes);
  }

  void _copyToClipboard(String text, bool showText) {
    Clipboard.setData(ClipboardData(text: text));
    toastification.show(
      type: ToastificationType.success,
      style: ToastificationStyle.flat,
      title: Text('Copied to clipboard'),
      description: showText ? Text(text) : null,
      autoCloseDuration: const Duration(seconds: 2),
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  }

  Widget _buildCopyableText(String text, bool showTooltip) {
    return showTooltip
        ? Tooltip(
            message: 'Click to copy',
            waitDuration: const Duration(milliseconds: 300),
            child: MouseRegion(
                cursor: SystemMouseCursors.click,
                child: GestureDetector(
                  onTap: () => {
                    _copyToClipboard(text, true),
                  },
                  child: Text(text),
                )),
          )
        : MouseRegion(
            cursor: SystemMouseCursors.click,
            child: GestureDetector(
              onTap: () => _copyToClipboard(text, true),
              child: Text(text),
            ),
          );
  }

  String _formatAppDetails(InstalledPackage app) {
    return 'App Name: ${app.label}\n'
        'Package Name: ${app.packageName}\n'
        'Version: ${app.versionName}\n'
        'Version Code: ${app.versionCode}\n'
        'Is VR: ${app.vr ? 'Yes' : 'No'}\n'
        'Is Launchable: ${app.launchable ? 'Yes' : 'No'}\n'
        'Is System: ${app.system ? 'Yes' : 'No'}\n'
        'Storage Usage:\n'
        'App: ${_formatSize(app.size.app.toInt())}\n'
        'Data: ${_formatSize(app.size.data.toInt())}\n'
        'Cache: ${_formatSize(app.size.cache.toInt())}\n'
        'Total: ${_formatAppSize(app.size)}';
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
    final totalSize =
        app.size.app.toInt() + app.size.data.toInt() + app.size.cache.toInt();
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: _buildCopyableText(app.label, true),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _buildDetailsRow('Package Name:', app.packageName, true),
            _buildDetailsRow('Version:', app.versionName, true),
            _buildDetailsRow('Version Code:', app.versionCode.toString(), true),
            _buildDetailsRow('Is VR:', app.vr ? 'Yes' : 'No', false),
            _buildDetailsRow(
                'Is Launchable:', app.launchable ? 'Yes' : 'No', false),
            _buildDetailsRow('Is System:', app.system ? 'Yes' : 'No', false),
            const SizedBox(height: 16),
            const Text('Storage Usage:',
                style: TextStyle(fontWeight: FontWeight.bold)),
            const SizedBox(height: 4),
            _buildDetailsRow('App:', _formatSize(app.size.app.toInt()), false),
            _buildDetailsRow(
                'Data:', _formatSize(app.size.data.toInt()), false),
            _buildDetailsRow(
                'Cache:', _formatSize(app.size.cache.toInt()), false),
            const Divider(height: 4),
            _buildDetailsRow('Total:', _formatAppSize(app.size), false),
            if (totalSize == 0 && !app.system) ...[
              const SizedBox(height: 8),
              const Text(
                'Note: If this app was recently installed, the OS might not be reporting its size yet.',
                style: TextStyle(
                  fontStyle: FontStyle.italic,
                  // fontSize: 12,
                ),
              ),
            ],
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => {
              _copyToClipboard(_formatAppDetails(app), false),
            },
            child: const Text('Copy'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  void _showUninstallDialog(BuildContext context, InstalledPackage app) async {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Uninstall App'),
        content: Text(
            'Are you sure you want to uninstall "${app.label}"?\n\nThis will permanently delete the app and all its data.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () {
              Navigator.of(context).pop();
              // AdbRequest(
              //         command: AdbCommand.ADB_COMMAND_UNINSTALL_PACKAGE,
              //         packageName: app.packageName)
              //     .sendSignalToRust();
            },
            child: const Text('Uninstall'),
          ),
        ],
      ),
    );
  }

  Widget _buildAppList(List<InstalledPackage> apps) {
    if (apps.isEmpty) {
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(16.0),
          child: Text('No apps in this category'),
        ),
      );
    }

    return ListView.builder(
      padding: _listPadding,
      itemCount: apps.length,
      itemBuilder: (context, index) {
        final app = apps[index];
        final appName = app.label.isNotEmpty ? app.label : app.packageName;
        final theme = Theme.of(context);
        final textTheme = theme.textTheme;

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
                  tooltip: 'App Details',
                  onPressed: () => _showAppDetailsDialog(context, app),
                ),
                if (_selectedCategory != AppCategory.system) ...[
                  IconButton(
                    icon: const Icon(Icons.play_arrow),
                    tooltip: 'Launch',
                    onPressed: () async {
                      AdbRequest(
                              command: AdbCommand.ADB_COMMAND_LAUNCH_APP,
                              packageName: app.packageName)
                          .sendSignalToRust();
                    },
                  ),
                  IconButton(
                    icon: const Icon(Icons.close),
                    tooltip: 'Force Stop',
                    onPressed: () async {
                      AdbRequest(
                              command: AdbCommand.ADB_COMMAND_FORCE_STOP_APP,
                              packageName: app.packageName)
                          .sendSignalToRust();
                    },
                  ),
                  IconButton(
                    icon: const Icon(Icons.delete_outline),
                    tooltip: 'Uninstall',
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
        if (!deviceState.isConnected) {
          return const Center(
            child: Text(
              'No device connected',
              style: TextStyle(fontSize: 18),
            ),
          );
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
                        label: Text('VR Apps (${_cachedVrApps?.length ?? 0})'),
                      ),
                      ButtonSegment(
                        value: AppCategory.other,
                        label: Text(
                            'Other Apps (${_cachedOtherApps?.length ?? 0})'),
                      ),
                      ButtonSegment(
                        value: AppCategory.system,
                        label: Text(
                            'System & Hidden Apps (${_cachedSystemApps?.length ?? 0})'),
                      ),
                    ],
                    selected: {_selectedCategory},
                    onSelectionChanged: (Set<AppCategory> newSelection) {
                      setState(() {
                        _selectedCategory = newSelection.first;
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
