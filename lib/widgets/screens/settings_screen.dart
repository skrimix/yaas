import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:file_picker/file_picker.dart';
import '../../src/bindings/bindings.dart';
import 'package:url_launcher/url_launcher.dart';
import '../../providers/settings_state.dart';

enum SettingTextField {
  rclonePath,
  rcloneRemoteName,
  adbPath,
  downloadsLocation,
  backupsLocation,
  bandwidthLimit,
}

class SettingsConstants {
  static const double sectionSpacing = 12.0;
  static const double padding = 16.0;
  static const double verticalSpacing = 8.0;
  static const double iconButtonSize = 32.0;
  static const double iconSize = 16.0;
}

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  final Map<SettingTextField, TextEditingController> _textControllers = {};

  late Settings _originalSettings;
  late Settings _currentFormSettings;
  bool _hasChanges = false;
  SettingsState? _settingsState;
  final ValueNotifier<bool> _isShiftPressedNotifier =
      ValueNotifier<bool>(false);

  @override
  void initState() {
    super.initState();

    final settingsState = Provider.of<SettingsState>(context, listen: false);
    _settingsState = settingsState;
    _originalSettings = settingsState.settings;
    _currentFormSettings = _originalSettings.copyWith();

    for (final setting in SettingTextField.values) {
      _textControllers[setting] = TextEditingController();
    }
    _updateAllControllers();

    // Listen for provider updates to sync defaults once loaded
    settingsState.addListener(_onSettingsProviderUpdated);

    // Track Shift key state similar to manage_apps.dart
    HardwareKeyboard.instance.addHandler(_handleKeyEvent);
  }

  @override
  void dispose() {
    _settingsState?.removeListener(_onSettingsProviderUpdated);
    for (final controller in _textControllers.values) {
      controller.dispose();
    }
    HardwareKeyboard.instance.removeHandler(_handleKeyEvent);
    _isShiftPressedNotifier.dispose();
    super.dispose();
  }

  bool _handleKeyEvent(KeyEvent event) {
    final isShiftPressed = HardwareKeyboard.instance.isShiftPressed;
    if (_isShiftPressedNotifier.value != isShiftPressed) {
      _isShiftPressedNotifier.value = isShiftPressed;
      setState(() {});
    }
    return false;
  }

  void _onSettingsProviderUpdated() {
    final settingsState = _settingsState;
    if (settingsState == null) return;

    final newSettings = settingsState.settings;
    // Only sync if form has no local edits to avoid stomping user input
    if (!_hasChanges && newSettings != _originalSettings) {
      setState(() {
        _originalSettings = newSettings.copyWith();
        _currentFormSettings = newSettings.copyWith();
        _updateAllControllers();
        _hasChanges = false;
      });
    }
  }

  void _updateSetting(SettingTextField field, String value,
      {bool updateController = false}) {
    setState(() {
      if (updateController) {
        _textControllers[field]?.text = value;
      }

      _currentFormSettings = switch (field) {
        SettingTextField.rclonePath =>
          _currentFormSettings.copyWith(rclonePath: value),
        SettingTextField.rcloneRemoteName =>
          _currentFormSettings.copyWith(rcloneRemoteName: value),
        SettingTextField.adbPath =>
          _currentFormSettings.copyWith(adbPath: value),
        SettingTextField.downloadsLocation =>
          _currentFormSettings.copyWith(downloadsLocation: value),
        SettingTextField.backupsLocation =>
          _currentFormSettings.copyWith(backupsLocation: value),
        SettingTextField.bandwidthLimit =>
          _currentFormSettings.copyWith(bandwidthLimit: value),
      };

      _checkForChanges();
    });
  }

  void _updateAllControllers() {
    for (final setting in SettingTextField.values) {
      _textControllers[setting]?.text = switch (setting) {
        SettingTextField.rclonePath => _currentFormSettings.rclonePath,
        SettingTextField.rcloneRemoteName =>
          _currentFormSettings.rcloneRemoteName,
        SettingTextField.adbPath => _currentFormSettings.adbPath,
        SettingTextField.downloadsLocation =>
          _currentFormSettings.downloadsLocation,
        SettingTextField.backupsLocation =>
          _currentFormSettings.backupsLocation,
        SettingTextField.bandwidthLimit => _currentFormSettings.bandwidthLimit,
      };
    }
  }

  void _checkForChanges() {
    final bool changed = _currentFormSettings != _originalSettings;
    if (changed != _hasChanges) {
      setState(() {
        _hasChanges = changed;
      });
    }
  }

  void _revertChanges() {
    setState(() {
      _currentFormSettings = _originalSettings.copyWith();
      _updateAllControllers();
      _hasChanges = false;
    });
  }

  void _resetToDefaults() {
    // Allow provider update callback to overwrite form fields with defaults
    setState(() {
      _hasChanges = false;
    });
    Provider.of<SettingsState>(context, listen: false).resetToDefaults();
  }

  void _saveSettings() {
    Provider.of<SettingsState>(context, listen: false)
        .save(_currentFormSettings);

    setState(() {
      _originalSettings = _currentFormSettings;
      _hasChanges = false;
    });
  }

  Future<void> _pickPath(SettingTextField field, bool isDirectory,
      String currentValue, String label) async {
    String? path;

    if (!isDirectory) {
      path = await _pickFile(currentValue, label);
    } else {
      path = await _pickDirectory(currentValue, label);
    }

    if (path != null) {
      _updateSetting(field, path, updateController: true);
    }
  }

  Future<String?> _pickFile(String currentValue, String label) async {
    String? initialDirectory;
    if (currentValue.isNotEmpty && File(currentValue).existsSync()) {
      initialDirectory = currentValue;
    }

    final result = await FilePicker.platform.pickFiles(
      dialogTitle: 'Select $label',
      initialDirectory: initialDirectory,
    );
    return result?.files.single.path;
  }

  Future<String?> _pickDirectory(String currentValue, String label) async {
    String? initialDirectory;
    if (currentValue.isNotEmpty && Directory(currentValue).existsSync()) {
      initialDirectory = currentValue;
    }

    return FilePicker.platform.getDirectoryPath(
      dialogTitle: 'Select $label Directory',
      initialDirectory: initialDirectory,
    );
  }

  Future<void> _launchURL(String url) async {
    final uri = Uri.parse(url);
    if (!await launchUrl(uri)) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Could not open $url'),
          ),
        );
      }
    }
  }

  String _formatCleanupPolicy(DownloadCleanupPolicy policy) => switch (policy) {
        DownloadCleanupPolicy.deleteAfterInstall => 'Remove after installation',
        DownloadCleanupPolicy.keepOneVersion => 'Keep latest version only',
        DownloadCleanupPolicy.keepTwoVersions => 'Keep last two versions',
        DownloadCleanupPolicy.keepAllVersions => 'Keep all versions',
      };

  String _formatConnectionType(ConnectionType type) => switch (type) {
        ConnectionType.usb => 'USB',
        ConnectionType.wireless => 'Wireless',
      };

  @override
  Widget build(BuildContext context) {
    final settingsState = Provider.of<SettingsState>(context);
    final error = settingsState.error;

    if (error != null) {
      return _buildErrorView(error);
    }

    if (settingsState.isLoading) {
      return const Center(child: CircularProgressIndicator());
    }

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
            SettingsConstants.padding,
            SettingsConstants.padding,
            SettingsConstants.padding,
            SettingsConstants.verticalSpacing,
          ),
          child: _buildHeader(),
        ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(SettingsConstants.padding),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: _buildSettingsSections(),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildErrorView(String error) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            'Error loading settings',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: SettingsConstants.verticalSpacing),
          Text(error),
        ],
      ),
    );
  }

  Widget _buildHeader() {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Text(
          'Settings',
          style: Theme.of(context).textTheme.headlineMedium,
        ),
        Row(
          children: [
            ValueListenableBuilder<bool>(
              valueListenable: _isShiftPressedNotifier,
              builder: (context, isShiftPressed, _) {
                final bool enabled = isShiftPressed || _hasChanges;
                return SizedBox(
                  height: SettingsConstants.iconButtonSize,
                  width: SettingsConstants.iconButtonSize,
                  child: IconButton.filledTonal(
                    onPressed: enabled
                        ? (isShiftPressed ? _resetToDefaults : _revertChanges)
                        : null,
                    iconSize: SettingsConstants.iconSize,
                    icon: Icon(isShiftPressed ? Icons.restart_alt : Icons.undo),
                    tooltip: isShiftPressed
                        ? 'Reset to Defaults'
                        : 'Revert Changes\n(Shift+Click to reset to defaults)',
                  ),
                );
              },
            ),
            const SizedBox(width: SettingsConstants.verticalSpacing),
            FilledButton.icon(
              onPressed: _hasChanges ? _saveSettings : null,
              icon: const Icon(Icons.save),
              label: const Text('Save Changes'),
            ),
          ],
        ),
      ],
    );
  }

  List<Widget> _buildSettingsSections() {
    return [
      _buildSection(
        title: 'Storage',
        children: [
          _buildPathSetting(
            field: SettingTextField.downloadsLocation,
            label: 'Downloads Location',
            isDirectory: true,
            currentValue: _currentFormSettings.downloadsLocation,
          ),
          _buildPathSetting(
            field: SettingTextField.backupsLocation,
            label: 'Backups Location',
            isDirectory: true,
            currentValue: _currentFormSettings.backupsLocation,
          ),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: 'ADB',
        children: [
          _buildPathSetting(
            field: SettingTextField.adbPath,
            label: 'ADB Path',
            isDirectory: false,
            currentValue: _currentFormSettings.adbPath,
          ),
          _buildDropdownSetting<ConnectionType>(
            label: 'Preferred Connection Type',
            value: _currentFormSettings.preferredConnectionType,
            items: ConnectionType.values.map((type) {
              return DropdownMenuItem(
                value: type,
                child: Text(_formatConnectionType(type)),
              );
            }).toList(),
            onChanged: (value) {
              if (value != null) {
                setState(() => _currentFormSettings = _currentFormSettings
                    .copyWith(preferredConnectionType: value));
                _checkForChanges();
              }
            },
          ),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: 'Downloader',
        children: [
          _buildPathSetting(
            field: SettingTextField.rclonePath,
            label: 'Rclone Path',
            isDirectory: false,
            currentValue: _currentFormSettings.rclonePath,
          ),
          _buildTextSetting(
            field: SettingTextField.rcloneRemoteName,
            label: 'Rclone Remote Name',
          ),
          _buildTextSetting(
            field: SettingTextField.bandwidthLimit,
            label: 'Bandwidth Limit',
            // helperText:
            //     'Value in KiB/s or with B|K|M|G|T|P suffix (empty for no limit)',
            helper: InkWell(
              onTap: () =>
                  _launchURL('https://rclone.org/docs/#bwlimit-bandwidth-spec'),
              child: Text(
                'Value in KiB/s or with B|K|M|G|T|P suffix or more (click for documentation)',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: Theme.of(context).hintColor,
                    ),
              ),
            ),
          ),
          _buildDropdownSetting<DownloadCleanupPolicy>(
            label: 'Downloads Cleanup',
            value: _currentFormSettings.cleanupPolicy,
            items: DownloadCleanupPolicy.values.map((policy) {
              return DropdownMenuItem(
                value: policy,
                child: Text(_formatCleanupPolicy(policy)),
              );
            }).toList(),
            onChanged: (value) {
              if (value != null) {
                setState(() => _currentFormSettings =
                    _currentFormSettings.copyWith(cleanupPolicy: value));
                _checkForChanges();
              }
            },
          ),
        ],
      ),
    ];
  }

  Widget _buildSection({
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
        Card(
          child: Padding(
            padding: const EdgeInsets.all(SettingsConstants.padding),
            child: Column(children: children),
          ),
        ),
      ],
    );
  }

  Widget _buildPathSetting({
    required SettingTextField field,
    required String label,
    required bool isDirectory,
    required String currentValue,
  }) {
    return _buildTextSetting(
      field: field,
      label: label,
      trailing: IconButton.filledTonal(
        icon: const Icon(Icons.folder_open),
        onPressed: () => _pickPath(field, isDirectory, currentValue, label),
        tooltip: 'Browse',
      ),
    );
  }

  Widget _buildTextSetting({
    required SettingTextField field,
    required String label,
    Widget? trailing,
    String? helperText,
    Widget? helper,
  }) {
    final controller = _textControllers[field];

    return Padding(
      padding: const EdgeInsets.symmetric(
          vertical: SettingsConstants.verticalSpacing),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: controller,
              decoration: InputDecoration(
                labelText: label,
                helperText: helper == null ? helperText : null,
                border: const OutlineInputBorder(),
                helper: helper,
              ),
              onChanged: (value) => _updateSetting(field, value),
            ),
          ),
          if (trailing != null) ...[
            const SizedBox(width: SettingsConstants.verticalSpacing),
            trailing,
          ],
        ],
      ),
    );
  }

  Widget _buildDropdownSetting<T>({
    required String label,
    required T value,
    required List<DropdownMenuItem<T>> items,
    required ValueChanged<T?> onChanged,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(
          vertical: SettingsConstants.verticalSpacing),
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
