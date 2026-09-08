import 'dart:io';

import 'package:flutter/material.dart';

import '../../src/bindings/bindings.dart';
import 'package:intl/intl.dart';
import 'package:provider/provider.dart';
import '../../utils/sideload_utils.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';
import '../../utils/utils.dart';

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
    final theme = Theme.of(context);
    final locale = Localizations.localeOf(context).toString();
    final timestamp = entry.timestamp.toInt();
    final date = timestamp == 0
        ? null
        : DateTime.fromMillisecondsSinceEpoch(timestamp, isUtc: true).toLocal();
    final dateLabel = date == null
        ? l10n.unknownTime
        : DateFormat.yMMMd(locale).add_Hm().format(date);
    final fullDate = date == null
        ? l10n.unknownTime
        : formatDateTime(context, date) ??
            DateFormat.yMMMd(locale).add_Hm().format(date);
    final sizeLabel = formatSize(entry.totalSize.toInt(), 2);
    final parts = <String>[];
    if (entry.hasApk) parts.add(l10n.partAPK);
    if (entry.hasPrivateData) parts.add(l10n.partPrivate);
    if (entry.hasSharedData) parts.add(l10n.partShared);
    if (entry.hasObb) parts.add(l10n.partOBB);
    final metadata = parts.isEmpty ? l10n.noPartsDetected : parts.join(', ');
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
                  const Icon(Icons.archive_outlined),
                  const SizedBox(width: 16),
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
                    SizedBox(width: 210 * textScale, child: dateText),
                    const SizedBox(width: 16),
                    SizedBox(width: 88 * textScale, child: sizeText),
                    const SizedBox(width: 24),
                  ],
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
    );
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
