import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:file_picker/file_picker.dart';
import '../providers/settings_state.dart';

class Settings extends StatefulWidget {
  const Settings({super.key});

  @override
  State<Settings> createState() => _SettingsState();
}

class _SettingsState extends State<Settings> {
  late String _rclonePath;
  late String _rcloneRemoteName;
  late String _adbPath;
  late ConnectionType _preferredConnectionType;
  late String _downloadsLocation;
  late String _backupsLocation;
  late String _bandwidthLimit;
  late DownloadCleanupPolicy _cleanupPolicy;

  @override
  void initState() {
    super.initState();
    final settings = Provider.of<SettingsState>(context, listen: false);
    _rclonePath = settings.rclonePath;
    _rcloneRemoteName = settings.rcloneRemoteName;
    _adbPath = settings.adbPath;
    _preferredConnectionType = settings.preferredConnectionType;
    _downloadsLocation = settings.downloadsLocation;
    _backupsLocation = settings.backupsLocation;
    _bandwidthLimit = settings.bandwidthLimit;
    _cleanupPolicy = settings.cleanupPolicy;
  }

  void _saveSettings() {
    final settings = Provider.of<SettingsState>(context, listen: false);
    settings.setRclonePath(_rclonePath);
    settings.setRcloneRemoteName(_rcloneRemoteName);
    settings.setAdbPath(_adbPath);
    settings.setPreferredConnectionType(_preferredConnectionType);
    settings.setDownloadsLocation(_downloadsLocation);
    settings.setBackupsLocation(_backupsLocation);
    settings.setBandwidthLimit(_bandwidthLimit);
    settings.setCleanupPolicy(_cleanupPolicy);
  }

  Future<void> _pickPath(BuildContext context, String label, bool isDirectory,
      String currentValue, ValueChanged<String> onChanged) async {
    String? path;
    if (!isDirectory) {
      final result = await FilePicker.platform.pickFiles(
        dialogTitle: 'Select $label',
        initialDirectory: currentValue.replaceAll('~', '/home'),
      );
      path = result?.files.single.path;
    } else {
      path = await FilePicker.platform.getDirectoryPath(
        dialogTitle: 'Select $label Directory',
        initialDirectory: currentValue.replaceAll('~', '/home'),
      );
    }

    if (path != null) {
      onChanged(path);
    }
  }

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Settings',
                style: Theme.of(context).textTheme.headlineMedium,
              ),
              FilledButton.icon(
                onPressed: _saveSettings,
                icon: const Icon(Icons.save),
                label: const Text('Save Changes'),
              ),
            ],
          ),
          const SizedBox(height: 24),
          _buildSection(
            context,
            title: 'External Tools',
            children: [
              _buildPathSetting(
                context,
                label: 'Rclone Path',
                value: _rclonePath,
                onChanged: (value) => setState(() => _rclonePath = value),
                onBrowse: () => _pickPath(
                    context,
                    'Rclone Binary',
                    false,
                    _rclonePath,
                    (value) => setState(() => _rclonePath = value)),
              ),
              _buildTextSetting(
                context,
                label: 'Rclone Remote Name',
                value: _rcloneRemoteName,
                onChanged: (value) => setState(() => _rcloneRemoteName = value),
              ),
              _buildPathSetting(
                context,
                label: 'ADB Path',
                value: _adbPath,
                onChanged: (value) => setState(() => _adbPath = value),
                onBrowse: () => _pickPath(context, 'ADB Binary', false,
                    _adbPath, (value) => setState(() => _adbPath = value)),
              ),
            ],
          ),
          const SizedBox(height: 24),
          _buildSection(
            context,
            title: 'Connection',
            children: [
              _buildDropdownSetting<ConnectionType>(
                context,
                label: 'Preferred Connection Type',
                value: _preferredConnectionType,
                items: ConnectionType.values.map((type) {
                  return DropdownMenuItem(
                    value: type,
                    child: Text(type.name.toUpperCase()),
                  );
                }).toList(),
                onChanged: (value) {
                  if (value != null) {
                    setState(() => _preferredConnectionType = value);
                  }
                },
              ),
            ],
          ),
          const SizedBox(height: 24),
          _buildSection(
            context,
            title: 'Storage',
            children: [
              _buildPathSetting(
                context,
                label: 'Downloads Location',
                value: _downloadsLocation,
                onChanged: (value) =>
                    setState(() => _downloadsLocation = value),
                onBrowse: () => _pickPath(
                    context,
                    'Downloads Location',
                    true,
                    _downloadsLocation,
                    (value) => setState(() => _downloadsLocation = value)),
              ),
              _buildPathSetting(
                context,
                label: 'Backups Location',
                value: _backupsLocation,
                onChanged: (value) => setState(() => _backupsLocation = value),
                onBrowse: () => _pickPath(
                    context,
                    'Backups Location',
                    true,
                    _backupsLocation,
                    (value) => setState(() => _backupsLocation = value)),
              ),
            ],
          ),
          const SizedBox(height: 24),
          _buildSection(
            context,
            title: 'Network',
            children: [
              _buildTextSetting(
                context,
                label: 'Bandwidth Limit',
                value: _bandwidthLimit,
                onChanged: (value) => setState(() => _bandwidthLimit = value),
                helperText:
                    'Value in KiB/s or with B|K|M|G|T|P suffix (empty for no limit)',
              ),
            ],
          ),
          const SizedBox(height: 24),
          _buildSection(
            context,
            title: 'Downloads Management',
            children: [
              _buildDropdownSetting<DownloadCleanupPolicy>(
                context,
                label: 'Downloads Cleanup',
                value: _cleanupPolicy,
                items: DownloadCleanupPolicy.values.map((policy) {
                  return DropdownMenuItem(
                    value: policy,
                    child: Text(_formatCleanupPolicy(policy)),
                  );
                }).toList(),
                onChanged: (value) {
                  if (value != null) setState(() => _cleanupPolicy = value);
                },
              ),
            ],
          ),
        ],
      ),
    );
  }

  String _formatCleanupPolicy(DownloadCleanupPolicy policy) {
    switch (policy) {
      case DownloadCleanupPolicy.deleteAfterInstall:
        return 'Remove after installation';
      case DownloadCleanupPolicy.keepOneVersion:
        return 'Keep latest version only';
      case DownloadCleanupPolicy.keepTwoVersions:
        return 'Keep last two versions';
      case DownloadCleanupPolicy.keepAllVersions:
        return 'Keep all versions';
    }
  }

  Widget _buildSection(
    BuildContext context, {
    required String title,
    required List<Widget> children,
  }) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title,
          style: Theme.of(context).textTheme.titleMedium,
        ),
        const SizedBox(height: 8),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16.0),
            child: Column(
              children: children,
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildPathSetting(
    BuildContext context, {
    required String label,
    required String value,
    required ValueChanged<String> onChanged,
    required VoidCallback onBrowse,
  }) {
    return _buildTextSetting(
      context,
      label: label,
      value: value,
      onChanged: onChanged,
      trailing: IconButton.filledTonal(
        icon: const Icon(Icons.folder_open),
        onPressed: onBrowse,
        tooltip: 'Browse',
      ),
    );
  }

  Widget _buildTextSetting(
    BuildContext context, {
    required String label,
    required String value,
    required ValueChanged<String> onChanged,
    Widget? trailing,
    String? helperText,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8.0),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: TextEditingController(text: value),
              decoration: InputDecoration(
                labelText: label,
                helperText: helperText,
                border: const OutlineInputBorder(),
              ),
              onChanged: onChanged,
            ),
          ),
          if (trailing != null) ...[
            const SizedBox(width: 8),
            trailing,
          ],
        ],
      ),
    );
  }

  Widget _buildDropdownSetting<T>(
    BuildContext context, {
    required String label,
    required T value,
    required List<DropdownMenuItem<T>> items,
    required ValueChanged<T?> onChanged,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8.0),
      child: DropdownButtonFormField<T>(
        value: value,
        items: items,
        onChanged: onChanged,
        decoration: InputDecoration(
          labelText: label,
          border: const OutlineInputBorder(),
        ),
      ),
    );
  }
}
