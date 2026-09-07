import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';
import '../../providers/settings_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

class AnimatedUninstallDialog extends StatefulWidget {
  final InstalledPackage app;
  final Stream<RustSignalPack<AdbCommandCompletedEvent>>? completionEvents;
  final ValueChanged<AdbRequest>? requestSender;

  const AnimatedUninstallDialog({
    super.key,
    required this.app,
    this.completionEvents,
    this.requestSender,
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
  bool _showCloseButton = false;
  bool _skipBackup = false;
  bool _backingUp = false;

  Timer? _closeButtonTimer;
  StreamSubscription<RustSignalPack<AdbCommandCompletedEvent>>? _subscription;

  @override
  void initState() {
    super.initState();

    _controller = AnimationController(
      duration: const Duration(milliseconds: 300),
      vsync: this,
    );

    _subscription =
        (widget.completionEvents ?? AdbCommandCompletedEvent.rustSignalStream)
            .listen((event) {
      final signal = event.message;
      if (signal.commandType == AdbCommandKind.uninstallPackage &&
          signal.commandKey == widget.app.packageName &&
          _isUninstalling) {
        _handleUninstallCompleted(signal.success);
      }
    });
  }

  @override
  void dispose() {
    _closeButtonTimer?.cancel();
    _subscription?.cancel();
    _controller.dispose();
    super.dispose();
  }

  void _handleUninstallCompleted(bool success) {
    if (!mounted) return;

    _closeButtonTimer?.cancel();

    setState(() {
      _isUninstalling = false;
      _showSuccess = success;
      _showCloseButton = false;
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
      _backingUp =
          context.read<SettingsState>().settings.autoBackupOnUninstall &&
              !_skipBackup;
      _isUninstalling = true;
      _showCloseButton = false;
    });

    _closeButtonTimer?.cancel();
    _closeButtonTimer = Timer(const Duration(seconds: 1), () {
      if (!mounted || _showSuccess) return;
      setState(() {
        _showCloseButton = true;
      });
    });

    final request = AdbRequest(
      command: AdbCommandUninstallPackage(
        packageName: widget.app.packageName,
        skipBackup: _skipBackup,
      ),
      commandKey: widget.app.packageName,
    );
    final sender = widget.requestSender;
    if (sender == null) {
      request.sendSignalToRust();
    } else {
      sender(request);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final autoBackup =
        context.watch<SettingsState>().settings.autoBackupOnUninstall;
    final backingUp = _isUninstalling || _showSuccess
        ? _backingUp
        : autoBackup && !_skipBackup;
    return AlertDialog(
      constraints: const BoxConstraints.tightFor(width: 400),
      scrollable: true,
      title: Row(
        children: [
          Expanded(child: Text(l10n.uninstallAppTitle)),
          if (_showCloseButton)
            IconButton(
              icon: const Icon(Icons.close),
              tooltip: l10n.commonClose,
              onPressed: () => Navigator.of(context).pop(),
            ),
        ],
      ),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          IndexedStack(
            index: backingUp ? 1 : 0,
            children: [
              Text(l10n.uninstallConfirmMessage(widget.app.label)),
              Text(l10n.uninstallWithBackupConfirmMessage(widget.app.label)),
            ],
          ),
          if (autoBackup) ...[
            const SizedBox(height: 12),
            CheckboxListTile(
              value: _skipBackup,
              onChanged: _isUninstalling || _showSuccess
                  ? null
                  : (value) => setState(() => _skipBackup = value ?? false),
              title: Text(l10n.uninstallSkipBackup),
              controlAffinity: ListTileControlAffinity.leading,
              contentPadding: EdgeInsets.zero,
              dense: true,
            ),
          ],
        ],
      ),
      actions: [
        TextButton(
          onPressed: _isUninstalling ? null : () => Navigator.of(context).pop(),
          child: Text(l10n.commonCancel),
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
              ? l10n.uninstalledDone
              : _isUninstalling
                  ? (backingUp
                      ? l10n.backingUpAndUninstalling
                      : l10n.uninstalling)
                  : l10n.uninstall),
        ),
      ],
    );
  }
}
