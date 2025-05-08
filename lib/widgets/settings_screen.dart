import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:file_picker/file_picker.dart';
import 'package:rql/src/bindings/bindings.dart';
import '../providers/settings_state.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  String _rclonePath = '';
  String _rcloneRemoteName = '';
  String _adbPath = '';
  late ConnectionType _preferredConnectionType;
  String _downloadsLocation = '';
  String _backupsLocation = '';
  String _bandwidthLimit = '';
  late DownloadCleanupPolicy _cleanupPolicy;

  // Text controllers to maintain cursor position
  late TextEditingController _rclonePathController;
  late TextEditingController _rcloneRemoteNameController;
  late TextEditingController _adbPathController;
  late TextEditingController _downloadsLocationController;
  late TextEditingController _backupsLocationController;
  late TextEditingController _bandwidthLimitController;

  // Store original settings to detect changes
  late Settings _originalSettings;
  bool _hasChanges = false;

  String? _error;

  @override
  void initState() {
    super.initState();
    final settingsState = Provider.of<SettingsState>(context, listen: false);
    final currentSettings = settingsState.settings;

    // Store original settings
    _originalSettings = currentSettings;

    _rclonePath = currentSettings.rclonePath;
    _rcloneRemoteName = currentSettings.rcloneRemoteName;
    _adbPath = currentSettings.adbPath;
    _preferredConnectionType = currentSettings.preferredConnectionType;
    _downloadsLocation = currentSettings.downloadsLocation;
    _backupsLocation = currentSettings.backupsLocation;
    _bandwidthLimit = currentSettings.bandwidthLimit;
    _cleanupPolicy = currentSettings.cleanupPolicy;

    // Initialize controllers with current values
    _rclonePathController = TextEditingController(text: _rclonePath);
    _rcloneRemoteNameController =
        TextEditingController(text: _rcloneRemoteName);
    _adbPathController = TextEditingController(text: _adbPath);
    _downloadsLocationController =
        TextEditingController(text: _downloadsLocation);
    _backupsLocationController = TextEditingController(text: _backupsLocation);
    _bandwidthLimitController = TextEditingController(text: _bandwidthLimit);

    _error = settingsState.error;
    settingsState.addListener(() {
      setState(() {
        print('settingsState changed');
        _error = settingsState.error;
      });
    });
  }

  @override
  void dispose() {
    // Dispose controllers to prevent memory leaks
    _rclonePathController.dispose();
    _rcloneRemoteNameController.dispose();
    _adbPathController.dispose();
    _downloadsLocationController.dispose();
    _backupsLocationController.dispose();
    _bandwidthLimitController.dispose();
    super.dispose();
  }

  void _checkForChanges() {
    final newSettings = Settings(
      rclonePath: _rclonePath,
      rcloneRemoteName: _rcloneRemoteName,
      adbPath: _adbPath,
      preferredConnectionType: _preferredConnectionType,
      downloadsLocation: _downloadsLocation,
      backupsLocation: _backupsLocation,
      bandwidthLimit: _bandwidthLimit,
      cleanupPolicy: _cleanupPolicy,
    );

    setState(() {
      _hasChanges = _originalSettings.rclonePath != newSettings.rclonePath ||
          _originalSettings.rcloneRemoteName != newSettings.rcloneRemoteName ||
          _originalSettings.adbPath != newSettings.adbPath ||
          _originalSettings.preferredConnectionType !=
              newSettings.preferredConnectionType ||
          _originalSettings.downloadsLocation !=
              newSettings.downloadsLocation ||
          _originalSettings.backupsLocation != newSettings.backupsLocation ||
          _originalSettings.bandwidthLimit != newSettings.bandwidthLimit ||
          _originalSettings.cleanupPolicy != newSettings.cleanupPolicy;
    });
  }

  void _revertChanges() {
    setState(() {
      _rclonePath = _originalSettings.rclonePath;
      _rcloneRemoteName = _originalSettings.rcloneRemoteName;
      _adbPath = _originalSettings.adbPath;
      _preferredConnectionType = _originalSettings.preferredConnectionType;
      _downloadsLocation = _originalSettings.downloadsLocation;
      _backupsLocation = _originalSettings.backupsLocation;
      _bandwidthLimit = _originalSettings.bandwidthLimit;
      _cleanupPolicy = _originalSettings.cleanupPolicy;

      // Update controller values
      _rclonePathController.text = _rclonePath;
      _rcloneRemoteNameController.text = _rcloneRemoteName;
      _adbPathController.text = _adbPath;
      _downloadsLocationController.text = _downloadsLocation;
      _backupsLocationController.text = _backupsLocation;
      _bandwidthLimitController.text = _bandwidthLimit;

      _hasChanges = false;
    });
  }

  void _saveSettings() {
    final newSettings = Settings(
      rclonePath: _rclonePath,
      rcloneRemoteName: _rcloneRemoteName,
      adbPath: _adbPath,
      preferredConnectionType: _preferredConnectionType,
      downloadsLocation: _downloadsLocation,
      backupsLocation: _backupsLocation,
      bandwidthLimit: _bandwidthLimit,
      cleanupPolicy: _cleanupPolicy,
    );
    Provider.of<SettingsState>(context, listen: false)
        .saveSettings(newSettings);

    setState(() {
      _originalSettings = newSettings;
      _hasChanges = false;
    });
  }

  Future<void> _pickPath(BuildContext context, String label, bool isDirectory,
      String currentValue, ValueChanged<String> onChanged) async {
    String? path;
    if (!isDirectory) {
      final result = await FilePicker.platform.pickFiles(
        dialogTitle: 'Select $label',
        initialDirectory: currentValue,
      );
      path = result?.files.single.path;
    } else {
      path = await FilePicker.platform.getDirectoryPath(
        dialogTitle: 'Select $label Directory',
        initialDirectory: currentValue,
      );
    }

    if (path != null) {
      onChanged(path);
      _checkForChanges();
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              'Error loading settings',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            Text(_error!),
          ],
        ),
      );
    }

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
              Row(
                children: [
                  SizedBox(
                    height: 32,
                    width: 32,
                    child: IconButton.filledTonal(
                      onPressed: _hasChanges ? _revertChanges : null,
                      iconSize: 16,
                      icon: const Icon(Icons.undo),
                      tooltip: 'Revert Changes',
                    ),
                  ),
                  const SizedBox(width: 8),
                  FilledButton.icon(
                    onPressed: _hasChanges ? _saveSettings : null,
                    icon: const Icon(Icons.save),
                    label: const Text('Save Changes'),
                  ),
                ],
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
                controller: _rclonePathController,
                onChanged: (value) => setState(() => _rclonePath = value),
                onBrowse: () => _pickPath(
                    context,
                    'Rclone binary',
                    false,
                    _rclonePath,
                    (value) => setState(() => _rclonePath = value)),
              ),
              _buildTextSetting(
                context,
                label: 'Rclone Remote Name',
                value: _rcloneRemoteName,
                controller: _rcloneRemoteNameController,
                onChanged: (value) => setState(() => _rcloneRemoteName = value),
              ),
              _buildPathSetting(
                context,
                label: 'ADB binary',
                value: _adbPath,
                controller: _adbPathController,
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
                controller: _downloadsLocationController,
                onChanged: (value) =>
                    setState(() => _downloadsLocation = value),
                onBrowse: () => _pickPath(
                    context,
                    'Downloads',
                    true,
                    _downloadsLocation,
                    (value) => setState(() => _downloadsLocation = value)),
              ),
              _buildPathSetting(
                context,
                label: 'Backups Location',
                value: _backupsLocation,
                controller: _backupsLocationController,
                onChanged: (value) => setState(() => _backupsLocation = value),
                onBrowse: () => _pickPath(
                    context,
                    'Backups',
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
                controller: _bandwidthLimitController,
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
    TextEditingController? controller,
  }) {
    return _buildTextSetting(
      context,
      label: label,
      value: value,
      controller: controller,
      onChanged: (value) {
        onChanged(value);
        _checkForChanges();
      },
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
    TextEditingController? controller,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8.0),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: controller ?? TextEditingController(text: value),
              decoration: InputDecoration(
                labelText: label,
                helperText: helperText,
                border: const OutlineInputBorder(),
              ),
              onChanged: (value) {
                onChanged(value);
                _checkForChanges();
              },
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
        onChanged: (value) {
          onChanged(value);
          _checkForChanges();
        },
        decoration: InputDecoration(
          labelText: label,
          border: const OutlineInputBorder(),
        ),
      ),
    );
  }
}
