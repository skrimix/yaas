import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart';
import '../providers/device_state.dart';
import '../messages/all.dart';

class ManageApps extends StatefulWidget {
  const ManageApps({super.key});

  @override
  State<ManageApps> createState() => _ManageAppsState();
}

class _ManageAppsState extends State<ManageApps> {
  int _selectedIndex = 0;
  final bool _sortAscending =
      true; // TODO: Add a toggle to sort ascending/descending?
  final List<String> _sections = [
    'VR Apps',
    'Other Apps',
    'System & Hidden Apps'
  ];

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

  List<InstalledPackage> _getFilteredApps(List<InstalledPackage>? packages) {
    if (packages == null) return [];

    var filtered = packages.where((app) {
      switch (_selectedIndex) {
        case 0: // VR Apps
          return app.vr && !app.system && app.launchable;
        case 1: // Other Apps
          return !app.vr && !app.system && app.launchable;
        case 2: // System & Hidden Apps
          return app.system || !app.launchable;
        default:
          return false;
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
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
          duration: const Duration(seconds: 1),
          content: Text(
              showText ? 'Copied to clipboard: $text' : 'Copied to clipboard')),
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
      padding: const EdgeInsets.only(bottom: 24),
      itemCount: apps.length,
      itemBuilder: (context, index) {
        final app = apps[index];
        final appName = app.label.isNotEmpty ? app.label : app.packageName;
        return Card(
          margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
          child: ListTile(
            title: Text(appName),
            subtitle: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '${app.packageName} • ${app.versionName} (${app.versionCode})',
                  style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                        color: Theme.of(context)
                            .textTheme
                            .bodyMedium
                            ?.color
                            ?.withValues(alpha: 0.6),
                      ),
                ),
                Text(
                  _formatAppSize(app.size),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: Theme.of(context)
                            .textTheme
                            .bodySmall
                            ?.color
                            ?.withValues(alpha: 0.6),
                      ),
                ),
              ],
            ),
            contentPadding:
                const EdgeInsets.symmetric(horizontal: 16.0, vertical: 4.0),
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                IconButton(
                  icon: const Icon(Icons.info_outline),
                  tooltip: 'App Details',
                  onPressed: () => _showAppDetailsDialog(context, app),
                ),
                if (_selectedIndex != 2) ...[
                  IconButton(
                    icon: const Icon(Icons.play_arrow),
                    tooltip: 'Launch',
                    onPressed: () {
                      // TODO: Implement launch functionality
                    },
                  ),
                  IconButton(
                    icon: const Icon(Icons.close),
                    tooltip: 'Force Stop',
                    onPressed: () {
                      // TODO: Implement force stop functionality
                    },
                  ),
                  IconButton(
                    icon: const Icon(Icons.delete_outline),
                    tooltip: 'Uninstall',
                    onPressed: () {
                      // TODO: Implement uninstall functionality
                    },
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

        final apps = _getFilteredApps(deviceState.device?.installedPackages);

        return Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                Padding(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                  child: SegmentedButton<int>(
                    segments: _sections
                        .asMap()
                        .entries
                        .map((e) => ButtonSegment(
                              value: e.key,
                              label: Text(
                                  '${e.value} (${_getFilteredApps(deviceState.device?.installedPackages).length})'),
                            ))
                        .toList(),
                    selected: {_selectedIndex},
                    onSelectionChanged: (Set<int> newSelection) {
                      setState(() {
                        _selectedIndex = newSelection.first;
                      });
                    },
                    style: ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      padding: WidgetStateProperty.all<EdgeInsets>(
                        const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                      ),
                    ),
                  ),
                ),
                Expanded(
                  child: AnimatedSwitcher(
                    duration: const Duration(milliseconds: 200),
                    transitionBuilder: (child, animation) {
                      return FadeTransition(
                        opacity: animation,
                        child: child,
                      );
                    },
                    child: Container(
                      key: ValueKey<int>(_selectedIndex),
                      child: _buildAppList(apps),
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
