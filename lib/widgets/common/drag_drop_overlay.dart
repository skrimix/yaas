import 'dart:io';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:desktop_drop/desktop_drop.dart';
import '../../providers/device_state.dart';
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

  void _handleFileDrop(List<DropItem> files, bool isDeviceConnected) {
    if (files.isEmpty) return;

    if (!isDeviceConnected) {
      SideloadUtils.showErrorToast(
          context, AppLocalizations.of(context).connectDeviceToInstall);
      return;
    }

    for (final file in files) {
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
