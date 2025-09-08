import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../../utils/utils.dart';
import 'cloud_app_details_dialog.dart';

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
    final l10n = AppLocalizations.of(context);
    if (apps.isEmpty) {
      return Center(
        child: Padding(
          padding: EdgeInsets.all(16.0),
          child: Text(isSearching ? l10n.noAppsFound : l10n.noAppsAvailable),
        ),
      );
    }

    // BUG: scrolling leaks memory slowly until navigated away and back
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

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;
    final l10n = AppLocalizations.of(context);

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
      child: SizedBox(
        child: MenuAnchor(
          menuChildren: [
            MenuItemButton(
              child: Text(l10n.copyFullName),
              onPressed: () {
                copyToClipboard(context, cachedApp.app.fullName,
                    description: cachedApp.app.fullName);
              },
            ),
            MenuItemButton(
              child: Text(l10n.copyPackageName),
              onPressed: () {
                copyToClipboard(context, cachedApp.app.packageName,
                    description: cachedApp.app.packageName);
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
                      l10n.sizeAndDate(
                          cachedApp.formattedSize, cachedApp.formattedDate),
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
                      icon: const Icon(Icons.info_outline),
                      tooltip: l10n.appDetails,
                      onPressed: () {
                        showDialog(
                          context: context,
                          builder: (context) => CloudAppDetailsDialog(
                            cachedApp: cachedApp,
                            onDownload: onDownload,
                            onInstall: onInstall,
                          ),
                        );
                      },
                    ),
                    const SizedBox(width: 8),
                    IconButton(
                      icon: const Icon(Icons.download),
                      tooltip: l10n.downloadToComputer,
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
                              ? l10n.downloadAndInstall
                              : l10n.downloadAndInstallNotConnected,
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
