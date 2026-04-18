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

  Future<void> _handleFileDrop(
      List<DropItem> files, bool isDeviceConnected) async {
    if (files.isEmpty) return;

    // Remaining items are for sideload/backup workflows
    final others = files.toList();
    if (others.isEmpty) return;

    if (!mounted) return;
    if (!isDeviceConnected) {
      SideloadUtils.showErrorToast(
          context, AppLocalizations.of(context).connectDeviceToInstall);
      return;
    }

    // First pass: validate all items and collect info
    final installItems = <({String path, bool isDirectory})>[];
    final backupItems = <String>[];
    int totalInstallSize = 0;

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
        backupItems.add(path);
      } else {
        installItems.add((path: path, isDirectory: isDirectory));
        totalInstallSize += SideloadUtils.calculateSize(path, isDirectory);
      }
    }

    if (installItems.isNotEmpty) {
      if (!mounted) return;
      final proceed =
          await SideloadUtils.confirmIfLowSpace(context, totalInstallSize);
      if (!proceed) return;
    }

    for (final backup in backupItems) {
      SideloadUtils.restoreBackup(backup);
    }
    for (final item in installItems) {
      SideloadUtils.installApp(item.path, item.isDirectory);
    }
  }

  Widget _buildDropSection({
    required BuildContext context,
    required IconData icon,
    required Color color,
    required String title,
    required String subtitle,
    bool isSecondary = false,
  }) {
    final theme = Theme.of(context);
    return ConstrainedBox(
      constraints: BoxConstraints(maxWidth: isSecondary ? 240 : 280),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(
            icon,
            size: isSecondary ? 40 : 56,
            color: isSecondary ? color.withValues(alpha: 0.7) : color,
          ),
          SizedBox(height: isSecondary ? 8 : 12),
          Text(
            title,
            textAlign: TextAlign.center,
            style: (isSecondary
                    ? theme.textTheme.titleSmall
                    : theme.textTheme.titleMedium)
                ?.copyWith(
              color: isSecondary ? color.withValues(alpha: 0.8) : color,
            ),
          ),
          SizedBox(height: isSecondary ? 4 : 6),
          Text(
            subtitle,
            textAlign: TextAlign.center,
            style: isSecondary
                ? theme.textTheme.bodySmall
                : theme.textTheme.bodyMedium,
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        final l10n = AppLocalizations.of(context);
        final theme = Theme.of(context);
        final colorScheme = theme.colorScheme;
        final isConnected = deviceState.isConnected;
        final mainDropSection = isConnected
            ? _buildDropSection(
                context: context,
                icon: Icons.file_download,
                color: colorScheme.primary,
                title: l10n.dragDropDropToInstall,
                subtitle: l10n.dragDropHintConnected,
              )
            : _buildDropSection(
                context: context,
                icon: Icons.phonelink_erase,
                color: colorScheme.error,
                title: l10n.dragDropNoDevice,
                subtitle: l10n.dragDropHintDisconnected,
              );
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
                      color: colorScheme.surface.withValues(alpha: 0.8),
                      child: Center(child: mainDropSection),
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
