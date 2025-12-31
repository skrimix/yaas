import 'dart:async';

import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../utils/sideload_utils.dart';
import '../../providers/app_state.dart';
import '../../src/l10n/app_localizations.dart';
import '../common/no_device_connected_indicator.dart';

class LocalSideloadScreen extends StatefulWidget {
  const LocalSideloadScreen({super.key});

  @override
  State<LocalSideloadScreen> createState() => _LocalSideloadScreenState();
}

class _LocalSideloadScreenState extends State<LocalSideloadScreen> {
  final _pathController = TextEditingController();
  bool _isDirectory = false;
  AppState? _appState;
  bool _initialized = false;

  @override
  void initState() {
    super.initState();
    ServicesBinding.instance.keyboard.addHandler(_onKey);
    _pathController.addListener(() {
      setState(() {});
      final appState = _appState;
      if (appState != null) {
        appState.setSideloadLastPath(_pathController.text);
      }
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _appState = context.read<AppState>();
    if (!_initialized && _appState != null) {
      _isDirectory = _appState!.sideloadIsDirectory;
      _pathController.text = _appState!.sideloadLastPath;
      _initialized = true;
    }
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
    final l10n = AppLocalizations.of(context);
    if (_isDirectory) {
      path = await FilePicker.platform
          .getDirectoryPath(dialogTitle: l10n.selectAppDirectoryTitle);
    } else {
      final result = await FilePicker.platform.pickFiles(
        dialogTitle: l10n.selectApkFileTitle,
        type: FileType.custom,
        allowedExtensions: ['apk'],
        allowMultiple: false,
      );
      path = result?.files.single.path;
    }

    if (path != null) {
      _pathController.text = path;
      _appState?.setSideloadLastPath(path);
    }
  }

  Future<bool> _install() async {
    final path = _pathController.text;
    if (path.isEmpty) return false;

    final l10n = AppLocalizations.of(context);
    // Validate paths before proceeding
    final isValid = _isDirectory
        ? SideloadUtils.isDirectoryValid(path)
        : SideloadUtils.isValidApkFile(path);
    if (!isValid) {
      final errorMessage =
          _isDirectory ? l10n.selectedInvalidDir : l10n.selectedInvalidApk;
      SideloadUtils.showErrorToast(context, errorMessage);
      return false;
    }

    final size = SideloadUtils.calculateSize(path, _isDirectory);
    if (!mounted) return false;
    final proceed = await SideloadUtils.confirmIfLowSpace(context, size);
    if (!proceed) return false;

    SideloadUtils.installApp(path, _isDirectory);

    _pathController.clear();
    return true;
  }

  bool _onKey(KeyEvent event) {
    if (event is KeyDownEvent &&
        (event.logicalKey == LogicalKeyboardKey.enter ||
            event.logicalKey == LogicalKeyboardKey.numpadEnter) &&
        _pathController.text.isNotEmpty) {
      unawaited(_install());
      return true;
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        final l10n = AppLocalizations.of(context);
        if (!deviceState.isConnected) {
          return const NoDeviceConnectedIndicator();
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
                          l10n.navSideload,
                          style: Theme.of(context).textTheme.headlineMedium,
                          textAlign: TextAlign.center,
                        ),
                        const SizedBox(height: 24),
                        SegmentedButton<bool>(
                          segments: [
                            ButtonSegment(
                              value: false,
                              label: Text(l10n.singleApk),
                              icon: const Icon(Icons.file_present),
                            ),
                            ButtonSegment(
                              value: true,
                              label: Text(l10n.appDirectory),
                              icon: const Icon(Icons.folder),
                            ),
                          ],
                          selected: {_isDirectory},
                          onSelectionChanged: (selection) {
                            setState(() {
                              _isDirectory = selection.first;
                              _pathController.clear();
                            });
                            _appState?.setSideloadIsDirectory(_isDirectory);
                            _appState?.setSideloadLastPath('');
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
                                      ? l10n.appDirectoryPath
                                      : l10n.apkFilePath,
                                  hintText: _isDirectory
                                      ? l10n.pathHintDirectory
                                      : l10n.pathHintApk,
                                  border: const OutlineInputBorder(),
                                ),
                              ),
                            ),
                            const SizedBox(width: 16),
                            IconButton.filledTonal(
                              onPressed: _pickPath,
                              icon: const Icon(Icons.folder_open),
                              tooltip: l10n.settingsBrowse,
                            ),
                          ],
                        ),
                        if (_isDirectory) ...[
                          const SizedBox(height: 16),
                          Text(
                            l10n.directoryRequirements,
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
                          l10n.proTipDragDrop,
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
  final Future<bool> Function() onPressed;

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
  bool _isProcessing = false;

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

  Future<void> _onPressed() async {
    if (_showSuccess || !widget.isEnabled || _isProcessing) return;

    setState(() => _isProcessing = true);
    final success = await widget.onPressed();
    if (!mounted) return;
    setState(() => _isProcessing = false);

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
    final l10n = AppLocalizations.of(context);
    final canPress = widget.isEnabled && !_showSuccess && !_isProcessing;
    return FilledButton.icon(
      onPressed: canPress ? _onPressed : null,
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
          ? l10n.addedToQueue
          : widget.isDirectory
              ? l10n.sideloadApp
              : l10n.installApk),
    );
  }
}
