import 'dart:io';

import 'package:flutter/material.dart';
import 'package:proper_filesize/proper_filesize.dart' as filesize;
import '../../utils/sideload_utils.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

class DownloadsScreen extends StatefulWidget {
  const DownloadsScreen({super.key});

  @override
  State<DownloadsScreen> createState() => _DownloadsScreenState();
}

class _DownloadsScreenState extends State<DownloadsScreen> {
  List<DownloadEntry> _entries = const [];
  bool _loading = false;
  String? _error;

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
                          : ListView.separated(
                              itemCount: _entries.length,
                              separatorBuilder: (_, __) => const Divider(height: 1),
                              itemBuilder: (context, index) => _DownloadTile(
                                entry: _entries[index],
                                onInstall: () =>
                                    SideloadUtils.installApp(_entries[index].path, true),
                                onOpenFolder: () => _openFolder(_entries[index].path),
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
    GetDownloadsDirectoryResponse.rustSignalStream.take(1).listen((event) async {
      final path = event.message.path;
      await _openFolder(path);
    });
    GetDownloadsDirectoryRequest().sendSignalToRust();
  }
}

class _DownloadTile extends StatelessWidget {
  final DownloadEntry entry;
  final VoidCallback onInstall;
  final VoidCallback onOpenFolder;

  const _DownloadTile({
    required this.entry,
    required this.onInstall,
    required this.onOpenFolder,
  });

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final subtitle = _buildSubtitle(context, entry, l10n);
    return ListTile(
      leading: const Icon(Icons.download_done_outlined),
      title: Text(entry.name),
      subtitle: Text(subtitle),
      trailing: SizedBox(
        height: 40,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            IconButton(
              tooltip: l10n.openFolderTooltip,
              icon: const Icon(Icons.folder_open),
              onPressed: onOpenFolder,
            ),
            const SizedBox(width: 8),
            FilledButton.icon(
              onPressed: onInstall,
              icon: const Icon(Icons.install_mobile),
              label: Text(l10n.install),
            ),
          ],
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

    final sizeStr = filesize.FileSize.fromBytes(entry.totalSize.toInt()).toString(
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

