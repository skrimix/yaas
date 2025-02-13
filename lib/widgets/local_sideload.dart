import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:provider/provider.dart';
import 'package:toastification/toastification.dart';
import '../providers/device_state.dart';
import '../messages/all.dart';
import 'dart:io';

class LocalSideload extends StatefulWidget {
  const LocalSideload({super.key});

  @override
  State<LocalSideload> createState() => _LocalSideloadState();
}

class _LocalSideloadState extends State<LocalSideload> {
  final _pathController = TextEditingController();
  bool _isDirectory = false;

  @override
  void initState() {
    super.initState();
    _pathController.addListener(() {
      setState(() {});
    });
  }

  @override
  void dispose() {
    _pathController.dispose();
    super.dispose();
  }

  Future<void> _pickPath() async {
    // TODO: remember last path
    String? path;
    if (_isDirectory) {
      path = await FilePicker.platform.getDirectoryPath();
    } else {
      final result = await FilePicker.platform.pickFiles(
        type: FileType.custom,
        allowedExtensions: ['apk'],
      );
      path = result?.files.single.path;
    }

    if (path != null) {
      _pathController.text = path;
    }
  }

  bool _isValidApkFile(String path) {
    final file = File(path);
    return file.existsSync() && file.path.toLowerCase().endsWith('.apk');
  }

  bool _isDirectoryValid(String path) {
    final dir = Directory(path);
    if (!dir.existsSync()) return false;

    try {
      return dir.listSync(recursive: true).any((entity) =>
          entity is File &&
          (entity.path.toLowerCase().endsWith('.apk') ||
              entity.path.toLowerCase() == 'install.txt'));
    } catch (e) {
      return false;
    }
  }

  void _install() {
    final path = _pathController.text;
    if (path.isEmpty) return;

    // Validate paths before proceeding
    if (_isDirectory) {
      if (!_isDirectoryValid(path)) {
        toastification.show(
          type: ToastificationType.error,
          style: ToastificationStyle.flat,
          title: const Text('Error'),
          description: const Text('Selected path is not a valid app directory'),
          autoCloseDuration: const Duration(seconds: 3),
          backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
          borderSide: BorderSide.none,
          alignment: Alignment.bottomRight,
        );
        return;
      }
    } else {
      if (!_isValidApkFile(path)) {
        toastification.show(
          type: ToastificationType.error,
          style: ToastificationStyle.flat,
          title: const Text('Error'),
          description: const Text('Selected path is not a valid APK file'),
          autoCloseDuration: const Duration(seconds: 3),
          backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
          borderSide: BorderSide.none,
          alignment: Alignment.bottomRight,
        );
        return;
      }
    }

    final command = _isDirectory
        ? AdbCommand.ADB_COMMAND_SIDELOAD_APP
        : AdbCommand.ADB_COMMAND_INSTALL_APK;

    final parameters = _isDirectory
        ? AdbRequest(command: command, appPath: path)
        : AdbRequest(command: command, apkPath: path);

    parameters.sendSignalToRust();
    _pathController.clear();
    toastification.show(
      type: ToastificationType.success,
      style: ToastificationStyle.flat,
      title: const Text('Installation Started'),
      description: Text(path),
      autoCloseDuration: const Duration(seconds: 3),
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        if (!deviceState.isConnected) {
          return const Center(
            child: Text(
              'No device connected',
              style: TextStyle(fontSize: 18),
            ),
          );
        }

        return Scaffold(
          body: SafeArea(
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 600),
                child: Card(
                  child: Padding(
                    padding: const EdgeInsets.all(24.0),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(
                          'Local Sideload',
                          style: Theme.of(context).textTheme.headlineMedium,
                          textAlign: TextAlign.center,
                        ),
                        const SizedBox(height: 24),
                        SegmentedButton<bool>(
                          segments: const [
                            ButtonSegment(
                              value: false,
                              label: Text('Single APK'),
                              icon: Icon(Icons.file_present),
                            ),
                            ButtonSegment(
                              value: true,
                              label: Text('App Directory'),
                              icon: Icon(Icons.folder),
                            ),
                          ],
                          selected: {_isDirectory},
                          onSelectionChanged: (selection) {
                            setState(() {
                              _isDirectory = selection.first;
                              _pathController.clear();
                            });
                          },
                        ),
                        const SizedBox(height: 24),
                        Row(
                          children: [
                            Expanded(
                              child: TextField(
                                controller: _pathController,
                                decoration: InputDecoration(
                                  labelText: _isDirectory
                                      ? 'App Directory Path'
                                      : 'APK File Path',
                                  hintText: _isDirectory
                                      ? 'Select or enter app directory path'
                                      : 'Select or enter APK file path',
                                  border: const OutlineInputBorder(),
                                ),
                              ),
                            ),
                            const SizedBox(width: 16),
                            IconButton.filledTonal(
                              onPressed: _pickPath,
                              icon: const Icon(Icons.folder_open),
                              tooltip: 'Browse',
                            ),
                          ],
                        ),
                        if (_isDirectory) ...[
                          const SizedBox(height: 16),
                          Text(
                            'Note: The app directory should contain an APK file and optionally an OBB data directory, or an install.txt file.',
                            style:
                                Theme.of(context).textTheme.bodySmall?.copyWith(
                                      fontStyle: FontStyle.italic,
                                    ),
                            textAlign: TextAlign.center,
                          ),
                        ],
                        const SizedBox(height: 16),
                        FilledButton.icon(
                          onPressed:
                              _pathController.text.isEmpty ? null : _install,
                          icon: const Icon(Icons.upload),
                          label: Text(
                              _isDirectory ? 'Sideload App' : 'Install APK'),
                        ),
                        const SizedBox(height: 16),
                        Text(
                          'Pro tip: You can also drag and drop APK files or app directories anywhere in the app to install them.',
                          style: Theme.of(context)
                              .textTheme
                              .bodySmall
                              ?.copyWith(
                                fontStyle: FontStyle.italic,
                                color: Theme.of(context).colorScheme.primary,
                              ),
                          textAlign: TextAlign.center,
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
  }
}
