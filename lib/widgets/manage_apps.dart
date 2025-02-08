import 'package:flutter/material.dart';
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
  final List<String> _sections = [
    'VR Apps',
    'Other Apps',
    'System & Hidden Apps'
  ];

  List<InstalledPackage> _getFilteredApps(List<InstalledPackage>? packages) {
    if (packages == null) return [];

    return packages.where((app) {
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
  }

  String _formatSize(AppSize size) {
    final totalBytes =
        size.app.toInt() + size.data.toInt() + size.cache.toInt();
    return FileSize.fromBytes(totalBytes).toString(
      unit: Unit.auto(
        size: totalBytes,
        baseType: BaseType.metric,
      ),
      decimals: 2,
    );
  }

  void _showAppDetailsDialog(BuildContext context, InstalledPackage app) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(app.label),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Package Name: ${app.packageName}'),
            Text('Version: ${app.versionName}'),
            Text('Version Code: ${app.versionCode}'),
            const SizedBox(height: 8),
            const Text('Storage Details:',
                style: TextStyle(fontWeight: FontWeight.bold)),
            Text(
                'App: ${FileSize.fromBytes(app.size.app.toInt()).toString(decimals: 2)}'),
            Text(
                'Data: ${FileSize.fromBytes(app.size.data.toInt()).toString(decimals: 2)}'),
            Text(
                'Cache: ${FileSize.fromBytes(app.size.cache.toInt()).toString(decimals: 2)}'),
            Text('Total: ${_formatSize(app.size)}'),
          ],
        ),
        actions: [
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
        return Card(
          margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
          child: ListTile(
            onTap: () => _showAppDetailsDialog(context, app),
            title: Text(app.label),
            subtitle: Text(
              '${app.packageName} • ${app.versionName} (${app.versionCode}) • ${_formatSize(app.size)}',
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                    color: Theme.of(context)
                        .textTheme
                        .bodyMedium
                        ?.color
                        ?.withValues(alpha: 0.6),
                  ),
            ),
            contentPadding:
                const EdgeInsets.symmetric(horizontal: 16.0, vertical: 4.0),
            trailing: _selectedIndex == 2
                ? null
                : Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
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

                // Animated content area
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
