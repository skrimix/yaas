import 'dart:io';

import 'package:flutter/material.dart';
import 'package:proper_filesize/proper_filesize.dart' as filesize;
import 'package:provider/provider.dart';
import '../../utils/sideload_utils.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';
import '../../providers/cloud_apps_state.dart';
import '../../providers/app_state.dart';

const _listPadding = EdgeInsets.only(bottom: 24);
const _cardMargin = EdgeInsets.symmetric(horizontal: 16, vertical: 2);
const _cardPadding = EdgeInsets.symmetric(horizontal: 16, vertical: 4);

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

  @override
  void initState() {
    super.initState();
    _loadDownloads();
    DownloadsChanged.rustSignalStream.listen((_) {
      if (mounted) _loadDownloads();
    });
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
    });
    GetDownloadsRequest().sendSignalToRust();
  }

  Future<void> _refresh() async {
    await _loadDownloads();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
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
                              padding: _listPadding,
                              itemCount: _entries.length,
                              itemBuilder: (context, index) => _DownloadTile(
                                entry: _entries[index],
                                newestDownloadedForPackage:
                                    _latestDownloadedByPackage[
                                        _entries[index].packageName ?? ''],
                                onInstall: () => SideloadUtils.installApp(
                                    _entries[index].path, true),
                                onOpenFolder: () =>
                                    _openFolder(_entries[index].path),
                                onDelete: () =>
                                    _confirmAndDelete(_entries[index]),
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
    final subtitle = _buildSubtitle(context, entry, l10n);
    return Card(
      margin: _cardMargin,
      child: ListTile(
        title: Text(entry.name),
        subtitle: Text(subtitle),
        contentPadding: _cardPadding,
        trailing: SizedBox(
          height: 40,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              _DownloadedNewerBadge(
                entry: entry,
                newestDownloadedForPackage: newestDownloadedForPackage,
              ),
              const SizedBox(width: 8),
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
          ),
        ),
      ),
    );
  }

  String _buildSubtitle(
      BuildContext context, DownloadEntry entry, AppLocalizations l10n) {
    final tsMillis = entry.timestamp.toInt();
    final dt = tsMillis == 0
        ? null
        : DateTime.fromMillisecondsSinceEpoch(tsMillis, isUtc: true).toLocal();
    final tsStr = dt == null
        ? l10n.unknownTime
        : '${dt.year.toString().padLeft(4, '0')}-${dt.month.toString().padLeft(2, '0')}-${dt.day.toString().padLeft(2, '0')} '
            '${dt.hour.toString().padLeft(2, '0')}:${dt.minute.toString().padLeft(2, '0')}:${dt.second.toString().padLeft(2, '0')}';

    final sizeStr =
        filesize.FileSize.fromBytes(entry.totalSize.toInt()).toString(
      unit: filesize.Unit.auto(
        size: entry.totalSize.toInt(),
        baseType: filesize.BaseType.metric,
      ),
      decimals: 2,
    );

    final pkg = entry.packageName ?? '';
    final ver = entry.versionCode?.toString() ?? '';
    final meta = pkg.isEmpty
        ? ''
        : ver.isEmpty
            ? pkg
            : '$pkg • v$ver';

    return meta.isEmpty ? '$tsStr • $sizeStr' : '$meta • $tsStr • $sizeStr';
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
      // Find the newest cloud version for this package (handle duplicates)
      int? cloudCode;
      for (final a in cloud.apps) {
        if (a.packageName == pkg) {
          cloudCode = cloudCode == null
              ? a.versionCode
              : (a.versionCode > cloudCode ? a.versionCode : cloudCode);
        }
      }
      if (cloudCode == null) return const SizedBox.shrink();

      // Compare against the newest downloaded version for this package
      final int downloadedCode = newestDownloadedForPackage ?? code;
      if (cloudCode <= downloadedCode) return const SizedBox.shrink();

      final theme = Theme.of(context);
      final scheme = theme.colorScheme;
      final l10n = AppLocalizations.of(context);

      return Tooltip(
        message: l10n.downloadedStatusToolTip,
        waitDuration: const Duration(milliseconds: 300),
        child: InkWell(
          borderRadius: BorderRadius.circular(999),
          onTap: () {
            final appState = context.read<AppState>();
            appState.setDownloadSearchQuery(pkg);
            appState.requestNavigationTo('download');
          },
          child: Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              decoration: BoxDecoration(
                color: Colors.transparent,
                borderRadius: BorderRadius.circular(999),
                border:
                    Border.all(color: scheme.secondary.withValues(alpha: 0.7)),
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
      );
    });
  }
}
