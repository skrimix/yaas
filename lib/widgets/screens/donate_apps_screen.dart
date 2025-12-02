import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../../providers/settings_state.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';
import '../../providers/cloud_apps_state.dart';
import '../../providers/app_state.dart';
import '../../src/bindings/bindings.dart';
import '../../utils/utils.dart';
import '../common/context_menu_region.dart';
import '../common/no_device_connected_indicator.dart';

const _cardPadding = EdgeInsets.symmetric(horizontal: 16.0, vertical: 4.0);
const _listPadding = EdgeInsets.only(bottom: 24);

class DonateAppsScreen extends StatefulWidget {
  const DonateAppsScreen({super.key});

  @override
  State<DonateAppsScreen> createState() => _DonateAppsScreenState();
}

enum FilterReason {
  blacklisted,
  renamed,
  systemUnwanted,
  alreadyExists,
}

enum DonationStatus {
  newApp,
  newerVersion,
}

class _FilteredApp {
  final InstalledPackage app;
  final List<FilterReason> filterReasons;
  final DonationStatus? status;

  _FilteredApp({
    required this.app,
    required this.filterReasons,
    this.status,
  });

  bool get isFiltered => filterReasons.isNotEmpty;
}

class _DonateAppsScreenState extends State<DonateAppsScreen> {
  /// Regex pattern to detect renamed packages.
  // Source: native/hub/src/models/cloud_app.rs normalize_package_name()
  // TODO: share this pattern with Rust backend
  static final _renamePattern = RegExp(r'(^mr\.)|(^mrf\.)|(\.mrf\.)|(\.jjb)');

  /// Check if package name contains rename markers
  bool _isRenamed(String packageName) {
    return _renamePattern.hasMatch(packageName);
  }

  /// Check if package is system/unwanted
  bool _isSystemUnwanted(String packageName) {
    return packageName.startsWith('com.oculus.') ||
        packageName.startsWith('com.meta.') ||
        packageName.contains('.environment.');
  }

  List<_FilteredApp> _getFilteredApps(
    List<InstalledPackage>? packages,
    Set<String> blacklist,
    CloudAppsState cloudAppsState,
  ) {
    if (packages == null) return [];

    // Only show user apps (non-system, launchable)
    final userApps =
        packages.where((app) => !app.system && app.launchable).toList();

    // Build list with filter reasons and status
    final filteredApps = userApps.map((app) {
      final reasons = <FilterReason>[];
      DonationStatus? status;

      if (blacklist.contains(app.packageName)) {
        reasons.add(FilterReason.blacklisted);
      }

      if (_isRenamed(app.packageName)) {
        reasons.add(FilterReason.renamed);
      }

      if (_isSystemUnwanted(app.packageName)) {
        reasons.add(FilterReason.systemUnwanted);
      }

      // Check if newer or same version exists in cloud
      final newestCloudVersion =
          cloudAppsState.newestVersionCodeForPackage(app.packageName);
      if (newestCloudVersion != null) {
        if (newestCloudVersion >= app.versionCode.toInt()) {
          reasons.add(FilterReason.alreadyExists);
        } else {
          // Installed version is newer than cloud
          status = DonationStatus.newerVersion;
        }
      } else {
        // Not in cloud at all
        status = DonationStatus.newApp;
      }

      return _FilteredApp(app: app, filterReasons: reasons, status: status);
    }).toList();

    // Sort: non-filtered first, then by app name
    filteredApps.sort((a, b) {
      if (a.isFiltered != b.isFiltered) {
        return a.isFiltered ? 1 : -1;
      }
      final nameA = (a.app.label.isNotEmpty ? a.app.label : a.app.packageName)
          .toLowerCase();
      final nameB = (b.app.label.isNotEmpty ? b.app.label : b.app.packageName)
          .toLowerCase();
      return nameA.compareTo(nameB);
    });

    return filteredApps;
  }

  String _getFilterReasonText(FilterReason reason, AppLocalizations l10n) {
    return switch (reason) {
      FilterReason.blacklisted => l10n.donateFilterReasonBlacklisted,
      FilterReason.renamed => l10n.donateFilterReasonRenamed,
      FilterReason.systemUnwanted => l10n.donateFilterReasonSystemUnwanted,
      FilterReason.alreadyExists => l10n.donateFilterReasonAlreadyExists,
    };
  }

  String _getStatusText(DonationStatus status, AppLocalizations l10n) {
    return switch (status) {
      DonationStatus.newApp => l10n.donateStatusNewApp,
      DonationStatus.newerVersion => l10n.donateStatusNewerVersion,
    };
  }

  void _donateApp(InstalledPackage app) {
    final displayName = app.label.isNotEmpty ? app.label : null;
    TaskRequest(
      task: TaskDonateApp(
        packageName: app.packageName,
        displayName: displayName,
      ),
    ).sendSignalToRust();
  }

  Widget _buildAppList(
      List<_FilteredApp> apps, AppLocalizations l10n, bool showFiltered) {
    final visibleApps =
        showFiltered ? apps : apps.where((a) => !a.isFiltered).toList();

    if (visibleApps.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(16.0),
          child: Text(
            showFiltered
                ? l10n.donateNoAppsAvailable
                : l10n.donateNoAppsWithFilters,
          ),
        ),
      );
    }

    return ListView.builder(
      padding: _listPadding,
      itemCount: visibleApps.length,
      itemBuilder: (context, index) {
        final filteredApp = visibleApps[index];
        final app = filteredApp.app;
        final appName = app.label.isNotEmpty ? app.label : app.packageName;
        final theme = Theme.of(context);
        final textTheme = theme.textTheme;
        final appSize = formatSize(app.size.app.toInt(), 2);

        return Card(
          margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
          child: SizedBox(
            child: ContextMenuRegion(
              menuChildren: [
                MenuItemButton(
                  child: Text(l10n.copyDisplayName),
                  onPressed: () {
                    copyToClipboard(context, appName, description: appName);
                  },
                ),
                MenuItemButton(
                  child: Text(l10n.copyPackageName),
                  onPressed: () {
                    copyToClipboard(context, app.packageName,
                        description: app.packageName);
                  },
                ),
              ],
              child: ListTile(
                title: Text(appName),
                subtitle: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      '${app.packageName} • ${app.versionName} (${app.versionCode})',
                      style: textTheme.bodyMedium?.copyWith(
                        color:
                            textTheme.bodyMedium?.color?.withValues(alpha: 0.6),
                      ),
                    ),
                    Text(
                      appSize,
                      style: textTheme.bodySmall?.copyWith(
                        color:
                            textTheme.bodySmall?.color?.withValues(alpha: 0.6),
                      ),
                    ),
                    if (showFiltered &&
                        (filteredApp.isFiltered ||
                            filteredApp.status != null)) ...[
                      const SizedBox(height: 4),
                      Wrap(
                        spacing: 4,
                        runSpacing: 4,
                        children: [
                          // Show filter reason chips for filtered apps
                          if (filteredApp.isFiltered)
                            ...filteredApp.filterReasons.map((reason) {
                              return Chip(
                                label: Text(
                                  _getFilterReasonText(reason, l10n),
                                  style: textTheme.bodySmall,
                                ),
                                visualDensity: VisualDensity.compact,
                                padding: EdgeInsets.zero,
                                materialTapTargetSize:
                                    MaterialTapTargetSize.shrinkWrap,
                              );
                            }),
                          // Show status chip for non-filtered apps
                          if (!filteredApp.isFiltered &&
                              filteredApp.status != null)
                            Chip(
                              label: Text(
                                _getStatusText(filteredApp.status!, l10n),
                                style: textTheme.bodySmall,
                              ),
                              backgroundColor:
                                  filteredApp.status == DonationStatus.newApp
                                      ? Colors.green.withValues(alpha: 0.2)
                                      : Colors.blue.withValues(alpha: 0.2),
                              visualDensity: VisualDensity.compact,
                              padding: EdgeInsets.zero,
                              materialTapTargetSize:
                                  MaterialTapTargetSize.shrinkWrap,
                            ),
                        ],
                      ),
                    ],
                    // Show status chip for non-filtered apps when not showing filtered
                    if (!showFiltered && filteredApp.status != null) ...[
                      const SizedBox(height: 4),
                      Chip(
                        label: Text(
                          _getStatusText(filteredApp.status!, l10n),
                          style: textTheme.bodySmall,
                        ),
                        backgroundColor:
                            filteredApp.status == DonationStatus.newApp
                                ? Colors.green.withValues(alpha: 0.2)
                                : Colors.blue.withValues(alpha: 0.2),
                        visualDensity: VisualDensity.compact,
                        padding: EdgeInsets.zero,
                        materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                      ),
                    ],
                  ],
                ),
                contentPadding: _cardPadding,
                trailing: FilledButton.icon(
                  icon: const Icon(Icons.upload),
                  label: Text(l10n.donateDonateButton),
                  onPressed: filteredApp.isFiltered && !showFiltered
                      ? null
                      : () => _donateApp(app),
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer4<DeviceState, CloudAppsState, AppState, SettingsState>(
      builder:
          (context, deviceState, cloudAppsState, appState, settingsState, _) {
        final l10n = AppLocalizations.of(context);

        if (!deviceState.isConnected) {
          return const NoDeviceConnectedIndicator();
        }

        Widget? placeholder;

        if (!settingsState.isDownloaderAvailable) {
          placeholder = Text(l10n.donateDownloaderNotAvailable);
        } else if (cloudAppsState.isLoading || cloudAppsState.apps.isEmpty) {
          placeholder = Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              const CircularProgressIndicator(),
              const SizedBox(height: 16),
              Text(l10n.donateLoadingCloudApps),
            ],
          );
        }

        if (placeholder != null) {
          return Scaffold(
            body: SafeArea(
              child: Center(
                child: placeholder,
              ),
            ),
          );
        }

        final filteredApps = _getFilteredApps(
          deviceState.device?.installedPackages,
          cloudAppsState.donationBlacklist,
          cloudAppsState,
        );

        final showFiltered = appState.donateShowFiltered;

        return Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.all(16.0),
                  child: Row(
                    children: [
                      Expanded(
                        child: Text(
                          l10n.donateAppsDescription,
                          style: Theme.of(context).textTheme.bodyLarge,
                        ),
                      ),
                      const SizedBox(width: 16),
                      FilterChip(
                          label: Text(
                            showFiltered
                                ? l10n.donateHideFiltered
                                : l10n.donateShowFiltered,
                          ),
                          selected: showFiltered,
                          showCheckmark: false,
                          onSelected: (value) {
                            appState.setDonateShowFiltered(value);
                          },
                          avatar: const Icon(Icons.visibility_off, size: 18)),
                    ],
                  ),
                ),
                Expanded(
                  child: _buildAppList(filteredApps, l10n, showFiltered),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
