import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:toastification/toastification.dart';
import '../providers/device_state.dart';
import '../src/bindings/bindings.dart';

class CachedAppData {
  final CloudApp app;
  final String formattedSize;
  final String formattedDate;

  const CachedAppData({
    required this.app,
    required this.formattedSize,
    required this.formattedDate,
  });
}

class CloudAppList extends StatelessWidget {
  static const cardPadding =
      EdgeInsets.symmetric(horizontal: 16.0, vertical: 4.0);
  static const listPadding = EdgeInsets.only(bottom: 24);

  final List<CachedAppData> apps;
  final bool showCheckboxes;
  final Set<String> selectedFullNames;
  final ValueChanged<String>? onSelectionChanged;
  final ScrollController scrollController;
  final Function(String) onDownload;
  final Function(String) onInstall;
  final bool isSearching;

  const CloudAppList({
    super.key,
    required this.apps,
    required this.showCheckboxes,
    required this.selectedFullNames,
    required this.scrollController,
    required this.isSearching,
    this.onSelectionChanged,
    required this.onDownload,
    required this.onInstall,
  });

  @override
  Widget build(BuildContext context) {
    if (apps.isEmpty) {
      return Center(
        child: Padding(
          padding: EdgeInsets.all(16.0),
          child: Text(isSearching ? 'No apps found' : 'No apps available'),
        ),
      );
    }

    // BUG: scrolling slowly leaks memory until navigated away and back
    return ListView.builder(
      controller: scrollController,
      padding: listPadding,
      itemCount: apps.length,
      prototypeItem: CloudAppListItem(
        cachedApp: apps.first,
        isSelected: selectedFullNames.contains(apps.first.app.fullName),
        onSelectionChanged: (selected) =>
            onSelectionChanged?.call(apps.first.app.fullName),
        showCheckbox: showCheckboxes,
        onDownload: onDownload,
        onInstall: onInstall,
      ),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      itemBuilder: (context, index) {
        final cachedApp = apps[index];
        return CloudAppListItem(
          cachedApp: cachedApp,
          isSelected: selectedFullNames.contains(cachedApp.app.fullName),
          onSelectionChanged: (selected) =>
              onSelectionChanged?.call(cachedApp.app.fullName),
          showCheckbox: showCheckboxes,
          onDownload: onDownload,
          onInstall: onInstall,
        );
      },
    );
  }
}

class CloudAppListItem extends StatelessWidget {
  const CloudAppListItem({
    super.key,
    required this.cachedApp,
    required this.isSelected,
    required this.onSelectionChanged,
    required this.showCheckbox,
    required this.onDownload,
    required this.onInstall,
  });

  final CachedAppData cachedApp;
  final bool isSelected;
  final ValueChanged<bool> onSelectionChanged;
  final bool showCheckbox;
  final Function(String) onDownload;
  final Function(String) onInstall;

  void _copyToClipboard(BuildContext context, String text) {
    Clipboard.setData(ClipboardData(text: text));
    toastification.show(
      type: ToastificationType.success,
      style: ToastificationStyle.flat,
      title: Text('Copied to clipboard'),
      description: Text(text),
      autoCloseDuration: const Duration(seconds: 2),
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
      child: SizedBox(
        child: MenuAnchor(
          menuChildren: [
            MenuItemButton(
              child: const Text('Copy full name'),
              onPressed: () {
                _copyToClipboard(context, cachedApp.app.fullName);
              },
            ),
            MenuItemButton(
              child: const Text('Copy package name'),
              onPressed: () {
                _copyToClipboard(context, cachedApp.app.packageName);
              },
            ),
          ],
          builder: (context, controller, child) {
            return GestureDetector(
              onSecondaryTapUp: (details) {
                controller.open(position: details.localPosition);
              },
              onLongPress: () {
                controller.open();
              },
              onTapUp: (_) {
                if (showCheckbox) {
                  onSelectionChanged(!isSelected);
                }
                controller.close();
              },
              child: ListTile(
                leading: showCheckbox
                    ? Checkbox(
                        value: isSelected,
                        onChanged: (value) =>
                            onSelectionChanged(value ?? false),
                      )
                    : null,
                title: Text(
                  cachedApp.app.fullName,
                  softWrap: false,
                  overflow: TextOverflow.ellipsis,
                ),
                subtitle: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      cachedApp.app.packageName,
                      style: textTheme.bodyMedium?.copyWith(
                        color:
                            textTheme.bodyMedium?.color?.withValues(alpha: 0.6),
                      ),
                    ),
                    Text(
                      'Size: ${cachedApp.formattedSize} • Last Updated: ${cachedApp.formattedDate}',
                      style: textTheme.bodySmall?.copyWith(
                        color:
                            textTheme.bodySmall?.color?.withValues(alpha: 0.6),
                      ),
                    ),
                  ],
                ),
                contentPadding: CloudAppList.cardPadding,
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    IconButton(
                      icon: const Icon(Icons.download),
                      tooltip: 'Download to computer',
                      onPressed: () {
                        onDownload(cachedApp.app.fullName);
                      },
                    ),
                    const SizedBox(width: 8),
                    Consumer<DeviceState>(
                      builder: (context, deviceState, _) {
                        return IconButton(
                          icon: const Icon(Icons.install_mobile),
                          tooltip: deviceState.isConnected
                              ? 'Install on device'
                              : 'Install on device (not connected)',
                          onPressed: deviceState.isConnected
                              ? () {
                                  onInstall(cachedApp.app.fullName);
                                }
                              : null,
                        );
                      },
                    ),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    );
  }
}
