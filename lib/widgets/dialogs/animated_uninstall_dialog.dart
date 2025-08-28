import 'package:flutter/material.dart';
import '../../src/bindings/bindings.dart';

class AnimatedUninstallDialog extends StatefulWidget {
  final InstalledPackage app;

  const AnimatedUninstallDialog({
    super.key,
    required this.app,
  });

  @override
  State<AnimatedUninstallDialog> createState() =>
      _AnimatedUninstallDialogState();
}

class _AnimatedUninstallDialogState extends State<AnimatedUninstallDialog>
    with TickerProviderStateMixin {
  late AnimationController _controller;

  bool _isUninstalling = false;
  bool _showSuccess = false;

  @override
  void initState() {
    super.initState();

    _controller = AnimationController(
      duration: const Duration(milliseconds: 300),
      vsync: this,
    );

    AdbCommandCompletedEvent.rustSignalStream.listen((event) {
      final signal = event.message;
      if (signal.commandType == AdbCommandType.uninstallPackage &&
          signal.packageName == widget.app.packageName) {
        _handleUninstallCompleted(signal.success);
      }
    });
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _handleUninstallCompleted(bool success) {
    if (!mounted) return;

    setState(() {
      _isUninstalling = false;
      _showSuccess = success;
    });

    if (success) {
      _controller.forward().then((_) {
        Future.delayed(const Duration(milliseconds: 200), () {
          if (mounted) {
            Navigator.of(context).pop();
          }
        });
      });
    }
  }

  void _startUninstall() {
    if (_isUninstalling || _showSuccess) return;

    setState(() {
      _isUninstalling = true;
    });

    AdbRequest(
            command: AdbCommandUninstallPackage(value: widget.app.packageName))
        .sendSignalToRust();

    // Fallback: stop processing after 30 seconds
    Future.delayed(const Duration(seconds: 30), () {
      if (_isUninstalling && mounted) {
        setState(() {
          _isUninstalling = false;
        });
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Uninstall App'),
      content: Text(
          'Are you sure you want to uninstall "${widget.app.label}"?\n\nThis will permanently delete the app and all its data.'),
      actions: [
        TextButton(
          onPressed: _isUninstalling ? null : () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton.icon(
          onPressed: _isUninstalling || _showSuccess ? null : _startUninstall,
          icon: AnimatedSwitcher(
            duration: const Duration(milliseconds: 200),
            child: _showSuccess
                ? const Icon(
                    Icons.check,
                    key: Key('success'),
                    color: Colors.white,
                  )
                : _isUninstalling
                    ? const SizedBox(
                        key: Key('loading'),
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          valueColor:
                              AlwaysStoppedAnimation<Color>(Colors.white),
                        ),
                      )
                    : const Icon(
                        Icons.delete_outline,
                        key: Key('idle'),
                      ),
          ),
          label: Text(_showSuccess
              ? 'Uninstalled!'
              : _isUninstalling
                  ? 'Uninstalling...'
                  : 'Uninstall'),
        ),
      ],
    );
  }
}
