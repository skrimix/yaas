import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart';
import '../../providers/settings_state.dart';
import '../../src/l10n/app_localizations.dart';
import '../../utils/utils.dart';
import '../common/context_menu_region.dart';
import 'cloud_app_details_dialog.dart';

class CachedAppData {
  final CloudApp app;
  final String formattedSize;
  final String formattedDate;
  // Precomputed lowercase strings
  final String fullNameLower;
  final String packageNameLower;

  const CachedAppData({
    required this.app,
    required this.formattedSize,
    required this.formattedDate,
    required this.fullNameLower,
    required this.packageNameLower,
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
  final Function(String, String) onDownload;
  final Function(String, String) onInstall;
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
  final Function(String, String) onDownload;
  final Function(String, String) onInstall;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;
    final l10n = AppLocalizations.of(context);

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
      child: SizedBox(
        child: ContextMenuRegion(
          menuChildren: [
            MenuItemButton(
              child: Text(l10n.copyFullName),
              onPressed: () {
                copyToClipboard(
                  context,
                  cachedApp.app.fullName,
                  description: cachedApp.app.fullName,
                );
              },
            ),
            MenuItemButton(
              child: Text(l10n.copyPackageName),
              onPressed: () {
                copyToClipboard(
                  context,
                  cachedApp.app.packageName,
                  description: cachedApp.app.packageName,
                );
              },
            ),
          ],
          onPrimaryTap:
              showCheckbox ? () => onSelectionChanged(!isSelected) : null,
          child: ListTile(
            leading: showCheckbox
                ? Checkbox(
                    value: isSelected,
                    onChanged: (value) => onSelectionChanged(value ?? false),
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
                    color: textTheme.bodyMedium?.color?.withValues(alpha: 0.6),
                  ),
                ),
                Text(
                  l10n.sizeAndDate(
                    cachedApp.formattedSize,
                    cachedApp.formattedDate,
                  ),
                  style: textTheme.bodySmall?.copyWith(
                    color: textTheme.bodySmall?.color?.withValues(alpha: 0.6),
                  ),
                ),
              ],
            ),
            contentPadding: CloudAppList.cardPadding,
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                _InstalledStatusBadge(app: cachedApp.app),
                const SizedBox(width: 8),
                Consumer<SettingsState>(
                  builder: (context, settings, _) {
                    final original = cachedApp.app.truePackageName;
                    final fav = settings.isFavorite(original);
                    return IconButton(
                      icon: Icon(
                        fav ? Icons.star_rounded : Icons.star_outline_rounded,
                        color:
                            fav ? Theme.of(context).colorScheme.tertiary : null,
                      ),
                      tooltip:
                          fav ? l10n.removeFromFavorites : l10n.addToFavorites,
                      onPressed: () =>
                          settings.toggleFavorite(original, value: !fav),
                    );
                  },
                ),
                const SizedBox(width: 8),
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
                    onDownload(
                        cachedApp.app.fullName, cachedApp.app.truePackageName);
                  },
                ),
                const SizedBox(width: 8),
                Consumer<DeviceState>(
                  builder: (context, deviceState, _) {
                    return Tooltip(
                      message: deviceState.isConnected
                          ? l10n.downloadAndInstall
                          : l10n.downloadAndInstallNotConnected,
                      child: OutlinedButton.icon(
                        icon: const Icon(Icons.install_mobile),
                        label: Text(l10n.install),
                        onPressed: deviceState.isConnected
                            ? () => _handleInstall(context)
                            : null,
                      ),
                    );
                  },
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _handleInstall(BuildContext context) async {
    final deviceState = context.read<DeviceState>();
    final installed = deviceState.findInstalled(cachedApp.app.packageName);
    if (installed != null &&
        installed.versionCode.toInt() > cachedApp.app.versionCode) {
      final confirmed = await _confirmDowngrade(
        context,
        installed,
        cachedApp.app,
      );
      if (!confirmed) return;
    }
    onInstall(cachedApp.app.fullName, cachedApp.app.truePackageName);
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
}

class _InstalledStatusBadge extends StatelessWidget {
  const _InstalledStatusBadge({required this.app});

  final CloudApp app;

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(builder: (context, deviceState, _) {
      final installed = deviceState.findInstalled(app.packageName);
      if (installed == null) {
        return const SizedBox.shrink();
      }

      final theme = Theme.of(context);
      final scheme = theme.colorScheme;
      final l10n = AppLocalizations.of(context);

      final int installedCode = installed.versionCode.toInt();
      final int cloudCode = app.versionCode;

      late final String label;
      late final IconData icon;
      late final Color fg;
      late final Color border;

      if (cloudCode > installedCode) {
        // Cloud is newer than installed: update available
        label = l10n.cloudStatusNewerVersion;
        icon = Icons.arrow_upward_rounded;
        fg = scheme.primary;
        border = scheme.primary;
      } else if (cloudCode < installedCode) {
        // Installed is newer than cloud
        label = l10n.cloudStatusOlderVersion;
        icon = Icons.arrow_downward_rounded;
        fg = scheme.secondary;
        border = scheme.secondary;
      } else {
        // Same version
        label = l10n.cloudStatusInstalled;
        icon = Icons.check_rounded;
        fg = theme.colorScheme.onSurfaceVariant;
        border = theme.colorScheme.outline;
      }

      final tooltip = l10n.cloudStatusTooltip(
        '${installed.versionCode}',
        '${app.versionCode}',
      );

      return Tooltip(
        message: tooltip,
        waitDuration: const Duration(milliseconds: 300),
        child: Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
            decoration: BoxDecoration(
              color: Colors.transparent,
              borderRadius: BorderRadius.circular(999),
              border: Border.all(color: border.withValues(alpha: 0.7)),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 14, color: fg),
                const SizedBox(width: 6),
                Text(
                  label,
                  style: Theme.of(context)
                      .textTheme
                      .labelSmall
                      ?.copyWith(color: fg),
                ),
              ],
            ),
          ),
        ),
      );
    });
  }
}
