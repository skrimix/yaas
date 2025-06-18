import 'dart:io';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:desktop_drop/desktop_drop.dart';
import '../providers/device_state.dart';
import '../utils/sideload_utils.dart';

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
      SideloadUtils.showErrorToast(context, 'Connect a device to install apps');
      return;
    }

    // TODO: handle multiple items
    final path = files.first.path;
    final isDirectory = FileSystemEntity.isDirectorySync(path);

    // Validate paths before proceeding
    final isValid = isDirectory
        ? SideloadUtils.isDirectoryValid(path)
        : SideloadUtils.isValidApkFile(path);

    if (!isValid) {
      final errorMessage = isDirectory
          ? 'Selected path is not a valid app directory'
          : 'Selected path is not a valid APK file';

      SideloadUtils.showErrorToast(context, errorMessage);
      return;
    }

    SideloadUtils.installApp(path, isDirectory);

    SideloadUtils.showInfoToast(
      context,
      'Installation Started',
      'Installing ${isDirectory ? 'app from directory' : 'APK file'}',
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
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
                                  ? 'Drop to Install'
                                  : 'No Device Connected',
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
                                  ? 'Drop APK file or app directory to sideload'
                                  : 'Connect a device to enable drag and drop installation',
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
