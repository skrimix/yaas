import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../providers/settings_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

@immutable
class DowngradeConfirmResult {
  const DowngradeConfirmResult({required this.clearData});

  final bool clearData;
}

typedef DowngradeItem = ({InstalledPackage installed, CloudApp target});

/// Shows a confirmation dialog when attempting to downgrade an app.
/// Returns a [DowngradeConfirmResult] if confirmed, or `null` if cancelled.
Future<DowngradeConfirmResult?> showDowngradeConfirmDialog(
  BuildContext context,
  InstalledPackage installed,
  CloudApp target,
) {
  return showDialog<DowngradeConfirmResult>(
    context: context,
    builder: (context) => DowngradeConfirmDialog(
      installed: installed,
      target: target,
    ),
  );
}

/// Shows a confirmation dialog when attempting to downgrade multiple apps.
/// Returns a [DowngradeConfirmResult] if confirmed, or `null` if cancelled.
Future<DowngradeConfirmResult?> showMultipleDowngradeConfirmDialog(
  BuildContext context,
  List<DowngradeItem> items,
) {
  if (items.isEmpty) {
    return Future.value(const DowngradeConfirmResult(clearData: false));
  }
  return showDialog<DowngradeConfirmResult>(
    context: context,
    builder: (context) => MultipleDowngradeConfirmDialog(items: items),
  );
}

class DowngradeConfirmDialog extends StatefulWidget {
  const DowngradeConfirmDialog({
    super.key,
    required this.installed,
    required this.target,
  });

  final InstalledPackage installed;
  final CloudApp target;

  @override
  State<DowngradeConfirmDialog> createState() => _DowngradeConfirmDialogState();
}

class _DowngradeConfirmDialogState extends State<DowngradeConfirmDialog> {
  bool _clearData = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final colorScheme = Theme.of(context).colorScheme;
    final autoReinstall =
        context.watch<SettingsState>().settings.autoReinstallOnConflict;
    final versionCode = '${widget.target.versionCode}';
    final defaultMessage = autoReinstall
        ? l10n.downgradeWithBackupConfirmMessage(versionCode)
        : l10n.downgradeConfirmMessage(versionCode);

    return AlertDialog(
      constraints: const BoxConstraints.tightFor(width: 420),
      scrollable: true,
      title: Text(l10n.downgradeAppTitle),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          IndexedStack(
            index: _clearData ? 1 : 0,
            children: [
              Text(defaultMessage),
              Text(l10n.downgradeClearDataConfirmMessage(versionCode)),
            ],
          ),
          const SizedBox(height: 12),
          CheckboxListTile(
            value: _clearData,
            onChanged: (value) => setState(() => _clearData = value ?? false),
            title: Text(l10n.downgradeClearData),
            controlAffinity: ListTileControlAffinity.leading,
            contentPadding: EdgeInsets.zero,
            dense: true,
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.commonCancel),
        ),
        FilledButton(
          style: ButtonStyle(
            backgroundColor: WidgetStatePropertyAll(colorScheme.error),
            foregroundColor: WidgetStatePropertyAll(colorScheme.onError),
          ),
          onPressed: () => Navigator.of(context).pop(
            DowngradeConfirmResult(clearData: _clearData),
          ),
          child: Text(l10n.commonConfirm),
        ),
      ],
    );
  }
}

class MultipleDowngradeConfirmDialog extends StatefulWidget {
  const MultipleDowngradeConfirmDialog({
    super.key,
    required this.items,
  });

  final List<DowngradeItem> items;

  @override
  State<MultipleDowngradeConfirmDialog> createState() =>
      _MultipleDowngradeConfirmDialogState();
}

class _MultipleDowngradeConfirmDialogState
    extends State<MultipleDowngradeConfirmDialog> {
  bool _clearData = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final colorScheme = Theme.of(context).colorScheme;
    final autoReinstall =
        context.watch<SettingsState>().settings.autoReinstallOnConflict;
    final defaultMessage = autoReinstall
        ? l10n.downgradeMultipleWithBackupConfirmMessage
        : l10n.downgradeMultipleConfirmMessage;
    final list = widget.items
        .map((e) => l10n.downgradeItemFormat(
              e.installed.label,
              '${e.installed.versionCode}',
              '${e.target.versionCode}',
            ))
        .join('\n');

    return AlertDialog(
      constraints: const BoxConstraints.tightFor(width: 440),
      scrollable: true,
      title: Text(l10n.downgradeAppsTitle),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          IndexedStack(
            index: _clearData ? 1 : 0,
            children: [
              Text(defaultMessage),
              Text(l10n.downgradeMultipleClearDataConfirmMessage),
            ],
          ),
          const SizedBox(height: 8),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(
              color: colorScheme.surfaceContainer,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(
                color: colorScheme.outlineVariant,
              ),
            ),
            child: Text(list, softWrap: true),
          ),
          const SizedBox(height: 12),
          CheckboxListTile(
            value: _clearData,
            onChanged: (value) => setState(() => _clearData = value ?? false),
            title: Text(l10n.downgradeClearData),
            controlAffinity: ListTileControlAffinity.leading,
            contentPadding: EdgeInsets.zero,
            dense: true,
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.commonCancel),
        ),
        FilledButton(
          style: ButtonStyle(
            backgroundColor: WidgetStatePropertyAll(colorScheme.error),
            foregroundColor: WidgetStatePropertyAll(colorScheme.onError),
          ),
          onPressed: () => Navigator.of(context).pop(
            DowngradeConfirmResult(clearData: _clearData),
          ),
          child: Text(l10n.commonConfirm),
        ),
      ],
    );
  }
}
