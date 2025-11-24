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
        final configDropSection = _buildDropSection(
          context: context,
          icon: Icons.cloud_download,
          color: colorScheme.primary,
          title: l10n.dragDropDownloaderConfigTitle,
          subtitle: l10n.dragDropDownloaderConfigHint,
          isSecondary: true,
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
                      child: Stack(
                        children: [
                          Center(child: mainDropSection),
                          Center(
                            child: Transform.translate(
                              offset: const Offset(308, 0),
                              child: configDropSection,
                            ),
                          ),
                        ],
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
