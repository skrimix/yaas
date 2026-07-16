import 'package:flutter/material.dart';

import '../../src/l10n/app_localizations.dart';

Future<bool> showActiveTasksCloseDialog({
  required BuildContext context,
  required int activeTaskCount,
  required Future<void> Function() prepareShutdown,
}) async {
  final result = await showDialog<bool>(
    context: context,
    barrierDismissible: false,
    builder: (context) => ActiveTasksCloseDialog(
      activeTaskCount: activeTaskCount,
      prepareShutdown: prepareShutdown,
    ),
  );
  return result ?? false;
}

class ActiveTasksCloseDialog extends StatefulWidget {
  const ActiveTasksCloseDialog({
    required this.activeTaskCount,
    required this.prepareShutdown,
    super.key,
  });

  final int activeTaskCount;
  final Future<void> Function() prepareShutdown;

  @override
  State<ActiveTasksCloseDialog> createState() => _ActiveTasksCloseDialogState();
}

class _ActiveTasksCloseDialogState extends State<ActiveTasksCloseDialog> {
  bool _isClosing = false;

  Future<void> _confirmExit() async {
    if (_isClosing) return;
    setState(() => _isClosing = true);

    await widget.prepareShutdown();
    if (mounted) {
      Navigator.of(context).pop(true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final colorScheme = Theme.of(context).colorScheme;

    return PopScope(
      canPop: !_isClosing,
      child: AlertDialog(
        title: Text(l10n.activeTasksExitTitle),
        content: _isClosing
            ? Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const SizedBox(
                    width: 24,
                    height: 24,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                  const SizedBox(width: 16),
                  Flexible(child: Text(l10n.cancellingTasksBeforeExit)),
                ],
              )
            : Text(l10n.activeTasksExitMessage(widget.activeTaskCount)),
        actions: _isClosing
            ? null
            : [
                TextButton(
                  key: const ValueKey('activeTasksCloseCancel'),
                  onPressed: () => Navigator.of(context).pop(false),
                  child: Text(l10n.commonCancel),
                ),
                FilledButton(
                  key: const ValueKey('activeTasksCloseConfirm'),
                  style: ButtonStyle(
                    backgroundColor: WidgetStatePropertyAll(colorScheme.error),
                    foregroundColor:
                        WidgetStatePropertyAll(colorScheme.onError),
                  ),
                  onPressed: _confirmExit,
                  child: Text(l10n.exitAndCancelTasks),
                ),
              ],
      ),
    );
  }
}
