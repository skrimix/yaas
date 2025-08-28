import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../utils/sideload_utils.dart';

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
    ServicesBinding.instance.keyboard.addHandler(_onKey);
    _pathController.addListener(() {
      setState(() {});
    });
  }

  @override
  void dispose() {
    _pathController.dispose();
    ServicesBinding.instance.keyboard.removeHandler(_onKey);
    super.dispose();
  }

  Future<void> _pickPath() async {
    // TODO: remember last path
    String? path;
    if (_isDirectory) {
      path = await FilePicker.platform
          .getDirectoryPath(dialogTitle: 'Select app directory');
    } else {
      final result = await FilePicker.platform.pickFiles(
        dialogTitle: 'Select APK file',
        type: FileType.custom,
        allowedExtensions: ['apk'],
        // TODO: handle multiple files
        allowMultiple: false,
      );
      path = result?.files.single.path;
    }

    if (path != null) {
      _pathController.text = path;
    }
  }

  bool _install() {
    final path = _pathController.text;
    if (path.isEmpty) return false;

    // Validate paths before proceeding
    final isValid = _isDirectory
        ? SideloadUtils.isDirectoryValid(path)
        : SideloadUtils.isValidApkFile(path);
    if (!isValid) {
      final errorMessage = _isDirectory
          ? 'Selected path is not a valid app directory'
          : 'Selected path is not a valid APK file';
      SideloadUtils.showErrorToast(context, errorMessage);
      return false;
    }

    SideloadUtils.installApp(path, _isDirectory);

    _pathController.clear();
    return true;
  }

  bool _onKey(KeyEvent event) {
    if (event is KeyDownEvent &&
        (event.logicalKey == LogicalKeyboardKey.enter ||
            event.logicalKey == LogicalKeyboardKey.numpadEnter) &&
        _pathController.text.isNotEmpty) {
      _install();
      return true;
    }
    return false;
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
                constraints: const BoxConstraints(maxWidth: 610),
                child: Card(
                  child: Padding(
                    padding: const EdgeInsets.all(24.0),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(
                          'Sideload',
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
                            'The directory should contain an APK file and optionally an OBB data directory, or install.txt file.',
                            style:
                                Theme.of(context).textTheme.bodySmall?.copyWith(
                                      fontStyle: FontStyle.italic,
                                    ),
                            textAlign: TextAlign.center,
                          ),
                        ],
                        const SizedBox(height: 16),
                        _AnimatedSideloadButton(
                          isEnabled: _pathController.text.isNotEmpty,
                          isDirectory: _isDirectory,
                          onPressed: _install,
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

class _AnimatedSideloadButton extends StatefulWidget {
  final bool isEnabled;
  final bool isDirectory;
  final bool Function() onPressed;

  const _AnimatedSideloadButton({
    required this.isEnabled,
    required this.isDirectory,
    required this.onPressed,
  });

  @override
  State<_AnimatedSideloadButton> createState() =>
      _AnimatedSideloadButtonState();
}

class _AnimatedSideloadButtonState extends State<_AnimatedSideloadButton>
    with TickerProviderStateMixin {
  late AnimationController _controller;

  bool _showSuccess = false;

  @override
  void initState() {
    super.initState();

    _controller = AnimationController(
      duration: const Duration(milliseconds: 300),
      vsync: this,
    );
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _onPressed() {
    if (_showSuccess || !widget.isEnabled) return;

    final success = widget.onPressed();

    if (success) {
      setState(() {
        _showSuccess = true;
      });

      _controller.forward().then((_) {
        Future.delayed(const Duration(milliseconds: 500), () {
          if (mounted) {
            _controller.reverse().then((_) {
              if (mounted) {
                setState(() {
                  _showSuccess = false;
                });
              }
            });
          }
        });
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return FilledButton.icon(
      onPressed: widget.isEnabled && !_showSuccess ? _onPressed : null,
      icon: AnimatedSwitcher(
        duration: const Duration(milliseconds: 200),
        child: _showSuccess
            ? const Icon(
                Icons.check,
                key: Key('success'),
                color: Colors.white,
              )
            : const Icon(
                Icons.upload,
                key: Key('idle'),
              ),
      ),
      label: Text(_showSuccess
          ? 'Added to queue!'
          : widget.isDirectory
              ? 'Sideload App'
              : 'Install APK'),
    );
  }
}
