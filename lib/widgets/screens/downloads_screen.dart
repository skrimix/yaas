import 'dart:io';

import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import 'package:provider/provider.dart';
import '../../utils/sideload_utils.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';
import '../../providers/cloud_apps_state.dart';
import '../../providers/app_state.dart';
import '../../utils/utils.dart';

const _listPadding = EdgeInsets.only(bottom: 24);
const _cardMargin = EdgeInsets.symmetric(horizontal: 16, vertical: 2);
const _cardPadding = EdgeInsets.symmetric(horizontal: 16, vertical: 4);

enum DownloadsSortOption {
  name,
  date,
  size,
}

class DownloadsScreen extends StatefulWidget {
  const DownloadsScreen({super.key});

  @override
  State<DownloadsScreen> createState() => _DownloadsScreenState();
}

class _DownloadsScreenState extends State<DownloadsScreen> {
  List<DownloadEntry> _entries = const [];
  bool _loading = false;
  String? _error;
  Map<String, int> _latestDownloadedByPackage = const {};
  DownloadsSortOption _sortOption = DownloadsSortOption.date;
  bool _sortAscending = false;
  bool _initialized = false;
  final _scrollController = ScrollController();
  double _lastScrollOffset = 0.0;

  @override
  void initState() {
    super.initState();
    _scrollController.addListener(() {
      if (_scrollController.hasClients) {
        _lastScrollOffset = _scrollController.position.pixels;
      }
    });
    _loadDownloads();
    DownloadsChanged.rustSignalStream.listen((_) {
      if (mounted) _loadDownloads();
    });
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_initialized) return;
    final appState = context.read<AppState>();
    _sortOption = switch (appState.downloadsSortKey) {
      'name' => DownloadsSortOption.name,
      'size' => DownloadsSortOption.size,
      _ => DownloadsSortOption.date,
    };
    _sortAscending = appState.downloadsSortAscending;
    _initialized = true;
  }

  Future<void> _loadDownloads() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    GetDownloadsResponse.rustSignalStream.take(1).listen((event) {
      final msg = event.message;
      if (!mounted) return;
      setState(() {
        _loading = false;
        _error = msg.error;
        _entries = msg.entries;
        // Precompute newest downloaded version per package (for update checking)
        final latest = <String, int>{};
        for (final e in _entries) {
          final pkg = e.packageName;
          final code = e.versionCode;
          if (pkg == null || pkg.isEmpty || code == null) continue;
          final prev = latest[pkg];
          if (prev == null || code > prev) {
            latest[pkg] = code;
          }
        }
        _latestDownloadedByPackage = latest;
      });
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!_scrollController.hasClients) return;
        final max = _scrollController.position.maxScrollExtent;
        _scrollController.jumpTo(_lastScrollOffset.clamp(0.0, max));
      });
    });
    GetDownloadsRequest().sendSignalToRust();
  }

  Future<void> _refresh() async {
    await _loadDownloads();
  }

  Future<void> _installEntry(DownloadEntry entry) async {
    final size = entry.totalSize.toInt();
    final proceed = await SideloadUtils.confirmIfLowSpace(context, size);
    if (!proceed) return;
    SideloadUtils.installApp(entry.path, true);
  }

  List<DownloadEntry> _sortedEntries() {
    final entries = [..._entries];
    entries.sort((a, b) {
      final nameA = a.name.toLowerCase();
      final nameB = b.name.toLowerCase();
      int result;

      switch (_sortOption) {
        case DownloadsSortOption.name:
          result = nameA.compareTo(nameB);
          break;
        case DownloadsSortOption.date:
          result = a.timestamp.toInt().compareTo(b.timestamp.toInt());
          if (result == 0) {
            result = nameA.compareTo(nameB);
          }
          break;
        case DownloadsSortOption.size:
          result = a.totalSize.toInt().compareTo(b.totalSize.toInt());
          if (result == 0) {
            result = nameA.compareTo(nameB);
          }
          break;
      }

      return _sortAscending ? result : -result;
    });
    return entries;
  }

  Widget _buildSortButton() {
    final l10n = AppLocalizations.of(context);

    bool isSelected(String key, bool ascending) {
      return _sortOption.name == key && _sortAscending == ascending;
    }

    PopupMenuItem<(String, bool)> buildItem(
      String key,
      bool ascending,
      String label,
    ) {
      return PopupMenuItem(
        value: (key, ascending),
        child: Row(
          children: [
            Icon(isSelected(key, ascending)
                ? Icons.radio_button_checked
                : Icons.radio_button_unchecked),
            const SizedBox(width: 8),
            Text(label),
          ],
        ),
      );
    }

    return PopupMenuButton<(String, bool)>(
      tooltip: l10n.sortBy,
      icon: const Icon(Icons.sort),
      initialValue: (_sortOption.name, _sortAscending),
      itemBuilder: (context) => [
        PopupMenuItem(
          enabled: false,
          child: Text(l10n.sortBy),
        ),
        buildItem('name', true, l10n.sortNameAsc),
        buildItem('name', false, l10n.sortNameDesc),
        buildItem('date', true, l10n.sortDateOldest),
        buildItem('date', false, l10n.sortDateNewest),
        buildItem('size', true, l10n.sortSizeSmallest),
        buildItem('size', false, l10n.sortSizeLargest),
      ],
      onSelected: (value) {
        final (key, ascending) = value;
        setState(() {
          _sortOption = DownloadsSortOption.values.byName(key);
          _sortAscending = ascending;
        });
        context.read<AppState>().setDownloadsSort(key, ascending);
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final entries = _sortedEntries();
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(16.0),
              child: Row(
                children: [
                  Text(
                    l10n.downloadsTitle,
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const Spacer(),
                  IconButton(
                    tooltip: l10n.deleteAllDownloads,
                    onPressed: _confirmDeleteAllDownloads,
                    icon: const Icon(Icons.delete_sweep),
                  ),
                  IconButton(
                    tooltip: l10n.openDownloadsFolder,
                    onPressed: _openDownloadsRoot,
                    icon: const Icon(Icons.folder_open),
                  ),
                  _buildSortButton(),
                  IconButton(
                    tooltip: l10n.refresh,
                    onPressed: _refresh,
                    icon: const Icon(Icons.refresh),
                  ),
                ],
              ),
            ),
            Expanded(
              child: _loading
                  ? const Center(child: CircularProgressIndicator())
                  : _error != null
                      ? Center(child: Text(_error!))
                      : _entries.isEmpty
                          ? Center(child: Text(l10n.noDownloadsFound))
                          : ListView.builder(
                              controller: _scrollController,
                              padding: _listPadding,
                              itemCount: entries.length,
                              itemBuilder: (context, index) => _DownloadTile(
                                entry: entries[index],
                                newestDownloadedForPackage:
                                    _latestDownloadedByPackage[
                                        entries[index].packageName ?? ''],
                                onInstall: () => _installEntry(entries[index]),
                                onOpenFolder: () =>
                                    _openFolder(entries[index].path),
                                onDelete: () =>
                                    _confirmAndDelete(entries[index]),
                              ),
                            ),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _openFolder(String folderPath) async {
    try {
      if (Platform.isLinux) {
        await Process.run('xdg-open', [folderPath]);
      } else if (Platform.isMacOS) {
        await Process.run('open', [folderPath]);
      } else if (Platform.isWindows) {
        await Process.run('explorer', [folderPath]);
      } else {
        if (mounted) {
          final l10n = AppLocalizations.of(context);
          SideloadUtils.showInfoToast(
            context,
            l10n.unsupportedPlatform,
            l10n.folderPathCopied,
          );
        }
      }
    } catch (e) {
      if (mounted) {
        SideloadUtils.showErrorToast(
          context,
          AppLocalizations.of(context).unableToOpenFolder(folderPath),
        );
      }
    }
  }

  void _openDownloadsRoot() {
    GetDownloadsDirectoryResponse.rustSignalStream
        .take(1)
        .listen((event) async {
      final path = event.message.path;
      await _openFolder(path);
    });
    GetDownloadsDirectoryRequest().sendSignalToRust();
  }

  void _deleteAllDownloads() {
    DeleteAllDownloadsResponse.rustSignalStream.take(1).listen((event) {
      final msg = event.message;
      if (!mounted) return;
      if (msg.error != null) {
        SideloadUtils.showErrorToast(context, msg.error!);
      } else {
        final l10n = AppLocalizations.of(context);
        final text = l10n.deleteAllDownloadsResult(
            msg.removed.toString(), msg.skipped.toString());
        SideloadUtils.showInfoToast(context, l10n.deleteAllDownloads, text);
        _loadDownloads();
      }
    });
    DeleteAllDownloadsRequest().sendSignalToRust();
  }

  Future<void> _confirmDeleteAllDownloads() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(AppLocalizations.of(context).deleteAllDownloadsTitle),
        content: Text(AppLocalizations.of(context).deleteAllDownloadsConfirm),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(AppLocalizations.of(context).commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(AppLocalizations.of(context).delete),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      _deleteAllDownloads();
    }
  }

  Future<void> _confirmAndDelete(DownloadEntry entry) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(AppLocalizations.of(context).deleteDownloadTitle),
        content: Text(
            AppLocalizations.of(context).deleteDownloadConfirm(entry.name)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(AppLocalizations.of(context).commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(AppLocalizations.of(context).delete),
          ),
        ],
      ),
    );

    if (confirmed != true) return;

    DeleteDownloadResponse.rustSignalStream.take(1).listen((event) {
      final msg = event.message;
      if (!mounted) return;
      if (msg.error != null) {
        SideloadUtils.showErrorToast(context, msg.error!);
      } else {
        SideloadUtils.showInfoToast(context,
            AppLocalizations.of(context).downloadDeletedTitle, entry.name);
        _loadDownloads();
      }
    });
    DeleteDownloadRequest(path: entry.path).sendSignalToRust();
  }
}

class _DownloadTile extends StatelessWidget {
  final DownloadEntry entry;
  final int? newestDownloadedForPackage;
  final VoidCallback onInstall;
  final VoidCallback onOpenFolder;
  final VoidCallback onDelete;

  const _DownloadTile({
    required this.entry,
    required this.newestDownloadedForPackage,
    required this.onInstall,
    required this.onOpenFolder,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final locale = Localizations.localeOf(context).toString();
    final timestamp = entry.timestamp.toInt();
    final date = timestamp == 0
        ? null
        : DateTime.fromMillisecondsSinceEpoch(timestamp, isUtc: true).toLocal();
    final dateLabel =
        date == null ? l10n.unknownTime : DateFormat.yMMMd(locale).format(date);
    final fullDate = date == null
        ? l10n.unknownTime
        : formatDateTime(context, date) ??
            DateFormat.yMMMd(locale).add_Hm().format(date);
    final sizeLabel = formatSize(entry.totalSize.toInt(), 2);
    final package = entry.packageName ?? '';
    final version = entry.versionCode;
    final metadata = package.isEmpty
        ? ''
        : version == null
            ? package
            : '$package • v$version';
    final metadataStyle = theme.textTheme.bodyMedium?.copyWith(
      color: theme.colorScheme.onSurfaceVariant,
    );
    final dateText = Tooltip(
      message: fullDate,
      child: Text(dateLabel, style: metadataStyle),
    );
    final sizeText = Text(
      sizeLabel,
      textAlign: TextAlign.end,
      style: metadataStyle?.copyWith(
        fontFeatures: const [FontFeature.tabularFigures()],
      ),
    );
    final textScale = MediaQuery.textScalerOf(context).scale(14) / 14;

    return Card(
      margin: _cardMargin,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final showColumns = constraints.maxWidth >= 1000 * textScale;
          return Padding(
            padding: _cardPadding,
            child: ConstrainedBox(
              constraints: const BoxConstraints(minHeight: 64),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Tooltip(
                          message: entry.name,
                          child: Text(
                            entry.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.titleMedium,
                          ),
                        ),
                        if (metadata.isNotEmpty)
                          Tooltip(
                            message: metadata,
                            child: Text(
                              metadata,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: metadataStyle,
                            ),
                          ),
                        if (!showColumns)
                          Padding(
                            padding: const EdgeInsets.only(top: 4),
                            child: Wrap(
                              spacing: 16,
                              runSpacing: 4,
                              children: [dateText, sizeText],
                            ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 24),
                  if (showColumns) ...[
                    SizedBox(width: 140 * textScale, child: dateText),
                    const SizedBox(width: 16),
                    SizedBox(width: 88 * textScale, child: sizeText),
                    const SizedBox(width: 24),
                  ],
                  SizedBox(
                    width: 160 * textScale,
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.end,
                      children: [
                        _InstalledDownloadBadge(entry: entry),
                        _DownloadedNewerBadge(
                          entry: entry,
                          newestDownloadedForPackage:
                              newestDownloadedForPackage,
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 16),
                  _buildActions(context, l10n),
                ],
              ),
            ),
          );
        },
      ),
    );
  }

  Widget _buildActions(BuildContext context, AppLocalizations l10n) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        IconButton(
          tooltip: l10n.delete,
          icon: const Icon(Icons.delete_outline),
          onPressed: onDelete,
        ),
        const SizedBox(width: 8),
        IconButton(
          tooltip: l10n.openFolderTooltip,
          icon: const Icon(Icons.folder_open),
          onPressed: onOpenFolder,
        ),
        const SizedBox(width: 8),
        Consumer<DeviceState>(
          builder: (context, deviceState, _) {
            if (!deviceState.isConnected) {
              return Tooltip(
                message: l10n.connectDeviceToInstall,
                child: FilledButton.icon(
                  onPressed: null,
                  icon: const Icon(Icons.install_mobile),
                  label: Text(l10n.install),
                ),
              );
            }
            return FilledButton.icon(
              onPressed: onInstall,
              icon: const Icon(Icons.install_mobile),
              label: Text(l10n.install),
            );
          },
        ),
      ],
    );
  }
}

class _InstalledDownloadBadge extends StatelessWidget {
  const _InstalledDownloadBadge({required this.entry});

  final DownloadEntry entry;

  @override
  Widget build(BuildContext context) {
    final pkg = entry.packageName;
    if (pkg == null || pkg.isEmpty) {
      return const SizedBox.shrink();
    }

    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        final installed = deviceState.findInstalled(pkg);
        if (installed == null) {
          return const SizedBox.shrink();
        }

        final theme = Theme.of(context);
        final scheme = theme.colorScheme;
        final l10n = AppLocalizations.of(context);

        final int installedCode = installed.versionCode.toInt();
        final int? downloadCode = entry.versionCode;

        late final String label;
        late final IconData icon;
        late final Color fg;
        late final Color border;

        if (downloadCode != null) {
          if (downloadCode > installedCode) {
            label = l10n.downloadedStatusInstalledOlder;
            icon = Icons.arrow_upward_rounded;
            fg = scheme.primary;
            border = scheme.primary;
          } else if (downloadCode < installedCode) {
            label = l10n.downloadedStatusInstalledNewer;
            icon = Icons.arrow_downward_rounded;
            fg = scheme.secondary;
            border = scheme.secondary;
          } else {
            label = l10n.downloadedStatusInstalled;
            icon = Icons.check_rounded;
            fg = theme.colorScheme.onSurfaceVariant;
            border = theme.colorScheme.outline;
          }
        } else {
          label = l10n.cloudStatusInstalled;
          icon = Icons.check_rounded;
          fg = theme.colorScheme.onSurfaceVariant;
          border = theme.colorScheme.outline;
        }

        return Padding(
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
        );
      },
    );
  }
}

class _DownloadedNewerBadge extends StatelessWidget {
  const _DownloadedNewerBadge(
      {required this.entry, required this.newestDownloadedForPackage});

  final DownloadEntry entry;
  final int? newestDownloadedForPackage;

  @override
  Widget build(BuildContext context) {
    final pkg = entry.packageName;
    final code = entry.versionCode;
    if (pkg == null || pkg.isEmpty || code == null) {
      return const SizedBox.shrink();
    }

    return Consumer<CloudAppsState>(builder: (context, cloud, _) {
      final int? cloudCode = cloud.newestVersionCodeForPackage(pkg);
      if (cloudCode == null) return const SizedBox.shrink();

      final int downloadedCode = newestDownloadedForPackage ?? code;
      if (cloudCode <= downloadedCode) return const SizedBox.shrink();

      final theme = Theme.of(context);
      final scheme = theme.colorScheme;
      final l10n = AppLocalizations.of(context);

      return Tooltip(
        message: l10n.downloadedStatusToolTip,
        waitDuration: const Duration(milliseconds: 300),
        child: Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Material(
            color: Colors.transparent,
            borderRadius: BorderRadius.circular(999),
            clipBehavior: Clip.antiAlias,
            child: InkWell(
              borderRadius: BorderRadius.circular(999),
              onTap: () {
                final appState = context.read<AppState>();
                appState.setDownloadSearchQuery(pkg);
                appState.requestNavigationTo('download');
              },
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(999),
                  border: Border.all(
                      color: scheme.secondary.withValues(alpha: 0.7)),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.arrow_upward_rounded,
                        size: 14, color: scheme.secondary),
                    const SizedBox(width: 6),
                    Text(
                      l10n.downloadedStatusNewerVersion,
                      style: theme.textTheme.labelSmall
                          ?.copyWith(color: scheme.secondary),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
    });
  }
}
