import 'package:flutter/material.dart';
import '../../src/bindings/bindings.dart';

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
      taskType: TaskType.backupApp,
      params: TaskParams(
        packageName: app.packageName,
        backupApk: _backupApk,
        backupData: _backupData,
        backupObb: _backupObb,
        backupNameAppend: suffix.isEmpty ? null : suffix,
        displayName: name,
      ),
    ).sendSignalToRust();

    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Backup Options'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('Select parts to back up:'),
          const SizedBox(height: 8),
          CheckboxListTile(
            value: _backupData,
            onChanged: (v) => setState(() => _backupData = v ?? true),
            title: const Text('App data'),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
          ),
          CheckboxListTile(
            value: _backupApk,
            onChanged: (v) => setState(() => _backupApk = v ?? false),
            title: const Text('APK'),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
          ),
          CheckboxListTile(
            value: _backupObb,
            onChanged: (v) => setState(() => _backupObb = v ?? false),
            title: const Text('OBB files'),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
          ),
          const SizedBox(height: 8),
          TextField(
            controller: _suffixController,
            decoration: const InputDecoration(
              labelText: 'Name suffix (optional)',
              hintText: 'e.g. pre-update',
            ),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed:
              _backupApk || _backupData || _backupObb ? _startBackup : null,
          child: const Text('Start Backup'),
        ),
      ],
    );
  }
}
