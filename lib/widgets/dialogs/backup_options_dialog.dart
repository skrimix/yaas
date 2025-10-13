import 'package:flutter/material.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

class BackupOptionsDialog extends StatefulWidget {
  final InstalledPackage app;
  const BackupOptionsDialog({super.key, required this.app});

  @override
  State<BackupOptionsDialog> createState() => _BackupOptionsDialogState();
}

class _BackupOptionsDialogState extends State<BackupOptionsDialog> {
  bool _backupData = true;
  bool _backupApk = false;
  bool _backupObb = false;
  final TextEditingController _suffixController = TextEditingController();

  @override
  void dispose() {
    _suffixController.dispose();
    super.dispose();
  }

  void _startBackup() {
    final app = widget.app;
    final suffix = _suffixController.text.trim();
    final name = app.label.isNotEmpty ? app.label : app.packageName;

    TaskRequest(
      task: TaskBackupApp(
        packageName: app.packageName,
        displayName: name,
        backupApk: _backupApk,
        backupData: _backupData,
        backupObb: _backupObb,
        backupNameAppend: suffix.isEmpty ? null : suffix,
      ),
    ).sendSignalToRust();

    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.backupOptionsTitle),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l10n.backupSelectParts),
          const SizedBox(height: 8),
          CheckboxListTile(
            value: _backupData,
            onChanged: (v) => setState(() => _backupData = v ?? true),
            title: Text(l10n.backupAppData),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
          ),
          CheckboxListTile(
            value: _backupApk,
            onChanged: (v) => setState(() => _backupApk = v ?? false),
            title: Text(l10n.backupApk),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
          ),
          CheckboxListTile(
            value: _backupObb,
            onChanged: (v) => setState(() => _backupObb = v ?? false),
            title: Text(l10n.backupObbFiles),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
          ),
          const SizedBox(height: 8),
          TextField(
            controller: _suffixController,
            decoration: InputDecoration(
              labelText: l10n.backupNameSuffix,
              hintText: l10n.backupNameSuffixHint,
            ),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.commonCancel),
        ),
        FilledButton(
          onPressed:
              _backupApk || _backupData || _backupObb ? _startBackup : null,
          child: Text(l10n.startBackup),
        ),
      ],
    );
  }
}
