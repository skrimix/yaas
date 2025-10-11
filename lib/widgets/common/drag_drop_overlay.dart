import 'dart:io';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:desktop_drop/desktop_drop.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart' as messages;
import '../../utils/sideload_utils.dart';
import '../../src/l10n/app_localizations.dart';

class DragDropOverlay extends StatefulWidget {
  final Widget child;

  const DragDropOverlay({
    super.key,
    required this.child,
  });

  @override
  State<DragDropOverlay> createState() => _DragDropOverlayState();
}

class _DragDropOverlayState extends State<DragDropOverlay> {
  bool _isDragging = false;

  Future<void> _handleFileDrop(
      List<DropItem> files, bool isDeviceConnected) async {
    if (files.isEmpty) return;

    // First, handle any potential downloader config JSONs dropped anywhere in the app
    final Set<String> recognizedConfigPaths = {};
    for (final item in files) {
      final path = item.path;
      try {
        if (await _isMaybeDownloaderConfig(path)) {
          recognizedConfigPaths.add(path);
          messages.InstallDownloaderConfigRequest(sourcePath: path)
              .sendSignalToRust();
        }
      } catch (_) {
        // ignore read errors for detection
      }
    }

    // Remaining items are for sideload/backup workflows
    final others = files
        .where((f) =>
            !_equalsIgnoreCase(_basename(f.path), 'downloader.json') &&
            !recognizedConfigPaths.contains(f.path))
        .toList();
    if (others.isEmpty) return;

    if (!mounted) return;
    if (!isDeviceConnected) {
      SideloadUtils.showErrorToast(
          context, AppLocalizations.of(context).connectDeviceToInstall);
      return;
    }

    for (final file in others) {
      final path = file.path;
      final isDirectory = FileSystemEntity.isDirectorySync(path);
      final isBackup = isDirectory && SideloadUtils.isBackupDirectory(path);

      // Validate paths before proceeding
      final isValid = isBackup
          ? true
          : isDirectory
              ? SideloadUtils.isDirectoryValid(path)
              : SideloadUtils.isValidApkFile(path);

      if (!isValid) {
        if (!mounted) return;
        final l10n = AppLocalizations.of(context);
        final errorMessage =
            isDirectory ? l10n.dragDropInvalidDir : l10n.dragDropInvalidFile;

        SideloadUtils.showErrorToast(context, errorMessage);
        return;
      }

      if (isBackup) {
        SideloadUtils.restoreBackup(path);
      } else {
        SideloadUtils.installApp(path, isDirectory);
      }
    }
  }

  String _basename(String path) {
    final parts = path.split(RegExp(r'[\\/]'));
    return parts.isNotEmpty ? parts.last : path;
  }

  bool _equalsIgnoreCase(String a, String b) =>
      a.toLowerCase() == b.toLowerCase();

  Future<bool> _isMaybeDownloaderConfig(String path) async {
    final name = _basename(path);
    if (!name.toLowerCase().endsWith('.json')) return false;
    final content = await File(path).readAsString();
    if (content.toLowerCase().contains('"rclone_path"')) {
      return true;
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        final l10n = AppLocalizations.of(context);
        return Stack(
          children: [
            widget.child,
            Positioned.fill(
              child: DropTarget(
                onDragEntered: (_) {
                  setState(() => _isDragging = true);
                },
                onDragExited: (_) {
                  setState(() => _isDragging = false);
                },
                onDragDone: (details) {
                  setState(() => _isDragging = false);
                  _handleFileDrop(details.files, deviceState.isConnected);
                },
                child: IgnorePointer(
                  ignoring: !_isDragging,
                  child: AnimatedOpacity(
                    opacity: _isDragging ? 1.0 : 0.0,
                    duration: const Duration(milliseconds: 200),
                    child: Container(
                      color: Theme.of(context)
                          .colorScheme
                          .surface
                          .withValues(alpha: 0.8),
                      child: Center(
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(
                              deviceState.isConnected
                                  ? Icons.file_download
                                  : Icons.phonelink_erase,
                              size: 64,
                              color: deviceState.isConnected
                                  ? Theme.of(context).colorScheme.primary
                                  : Theme.of(context).colorScheme.error,
                            ),
                            const SizedBox(height: 16),
                            Text(
                              deviceState.isConnected
                                  ? l10n.dragDropDropToInstall
                                  : l10n.dragDropNoDevice,
                              style: Theme.of(context)
                                  .textTheme
                                  .headlineMedium
                                  ?.copyWith(
                                    color: deviceState.isConnected
                                        ? Theme.of(context).colorScheme.primary
                                        : Theme.of(context).colorScheme.error,
                                  ),
                            ),
                            const SizedBox(height: 8),
                            Text(
                              deviceState.isConnected
                                  ? l10n.dragDropHintConnected
                                  : l10n.dragDropHintDisconnected,
                              style: Theme.of(context).textTheme.bodyLarge,
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        );
      },
    );
  }
}
