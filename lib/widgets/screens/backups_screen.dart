import 'dart:io';

import 'package:flutter/material.dart';

import '../../src/bindings/bindings.dart';
import 'package:proper_filesize/proper_filesize.dart' as filesize;
import 'package:provider/provider.dart';
import '../../utils/sideload_utils.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';

const _listPadding = EdgeInsets.only(bottom: 24);
const _cardMargin = EdgeInsets.symmetric(horizontal: 16, vertical: 2);
const _cardPadding = EdgeInsets.symmetric(horizontal: 16, vertical: 4);

class BackupsScreen extends StatefulWidget {
  const BackupsScreen({super.key});

  @override
  State<BackupsScreen> createState() => _BackupsScreenState();
}

class _BackupsScreenState extends State<BackupsScreen> {
  List<BackupEntry> _entries = const [];
  bool _loading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadBackups();
    BackupsChanged.rustSignalStream.listen((_) {
      if (mounted) _loadBackups();
    });
  }

  Future<void> _refresh() async {
    await _loadBackups();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(16.0),
              child: Row(
                children: [
                  Text(
                    AppLocalizations.of(context).backupsTitle,
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const Spacer(),
                  IconButton(
                    tooltip: AppLocalizations.of(context).openBackupsFolder,
                    onPressed: _openBackupsRoot,
                    icon: const Icon(Icons.folder_open),
                  ),
                  IconButton(
                    tooltip: AppLocalizations.of(context).refresh,
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
                          ? Center(
                              child: Text(
                                  AppLocalizations.of(context).noBackupsFound))
                          : ListView.builder(
                              padding: _listPadding,
                              itemCount: _entries.length,
                              itemBuilder: (context, index) => _BackupTile(
                                entry: _entries[index],
                                onRestore: () => SideloadUtils.restoreBackup(
                                    _entries[index].path),
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

  Future<void> _loadBackups() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    GetBackupsResponse.rustSignalStream.take(1).listen((event) {
      final msg = event.message;
      if (!mounted) return;
      setState(() {
        _loading = false;
        _error = msg.error;
        _entries = msg.entries;
      });
    });
    GetBackupsRequest().sendSignalToRust();
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
}

class _BackupTile extends StatelessWidget {
  final BackupEntry entry;
  final VoidCallback onRestore;
  final VoidCallback onOpenFolder;
  final VoidCallback onDelete;

  const _BackupTile({
    required this.entry,
    required this.onRestore,
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
        leading: const Icon(Icons.archive_outlined),
        title: Text(entry.name),
        subtitle: Text(subtitle),
        contentPadding: _cardPadding,
        trailing: SizedBox(
          height: 40,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.center,
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
                      message: l10n.connectDeviceToRestore,
                      child: FilledButton.icon(
                        onPressed: null,
                        icon: const Icon(Icons.restore),
                        label: Text(l10n.restore),
                      ),
                    );
                  }
                  return FilledButton.icon(
                    onPressed: onRestore,
                    icon: const Icon(Icons.restore),
                    label: Text(l10n.restore),
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
      BuildContext context, BackupEntry entry, AppLocalizations l10n) {
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

    final parts = <String>[];
    if (entry.hasApk) parts.add(l10n.partAPK);
    if (entry.hasPrivateData) parts.add(l10n.partPrivate);
    if (entry.hasSharedData) parts.add(l10n.partShared);
    if (entry.hasObb) parts.add(l10n.partOBB);
    final partsStr = parts.isEmpty ? l10n.noPartsDetected : parts.join(', ');

    return '$tsStr • $partsStr • $sizeStr';
  }
}

extension on _BackupsScreenState {
  Future<void> _confirmAndDelete(BackupEntry entry) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(AppLocalizations.of(context).deleteBackupTitle),
        content:
            Text(AppLocalizations.of(context).deleteBackupConfirm(entry.name)),
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

    DeleteBackupResponse.rustSignalStream.take(1).listen((event) {
      final msg = event.message;
      if (!mounted) return;
      if (msg.error != null) {
        SideloadUtils.showErrorToast(context, msg.error!);
      } else {
        SideloadUtils.showInfoToast(context,
            AppLocalizations.of(context).backupDeletedTitle, entry.name);
        _loadBackups();
      }
    });
    DeleteBackupRequest(path: entry.path).sendSignalToRust();
  }
}

extension _BackupsRoot on _BackupsScreenState {
  void _openBackupsRoot() {
    GetBackupsDirectoryResponse.rustSignalStream
        .take(1)
        .listen((response) async {
      final path = response.message.path;
      await _openFolder(path);
    });
    GetBackupsDirectoryRequest().sendSignalToRust();
  }
}
