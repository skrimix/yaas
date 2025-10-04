import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import '../../utils/theme_utils.dart' as app_theme;
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:file_picker/file_picker.dart';
import '../../src/bindings/bindings.dart';
import 'package:url_launcher/url_launcher.dart';
import '../../providers/settings_state.dart';
import '../../src/l10n/app_localizations.dart';

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

// TODO: validate paths on input change
typedef StartupPageOption = ({
  String key,
  String Function(AppLocalizations l10n) label,
});

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key, required this.pageOptions});

  final List<StartupPageOption> pageOptions;

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
  bool _rcloneRemoteCustom = false;
  bool _seedColorCustom = false;
  late final List<StartupPageOption> _pageOptions;
  final TextEditingController _customColorController = TextEditingController();
  bool _castingStatusRequested = false;

  @override
  void initState() {
    super.initState();

    final settingsState = Provider.of<SettingsState>(context, listen: false);
    _settingsState = settingsState;
    _originalSettings = settingsState.settings;
    _currentFormSettings = _originalSettings.copyWith();
    _pageOptions = List.unmodifiable(widget.pageOptions);
    _originalSettings = settingsState.settings;
    _currentFormSettings = _originalSettings.copyWith();

    for (final setting in SettingTextField.values) {
      _textControllers[setting] = TextEditingController();
    }
    _updateAllControllers();

    settingsState.addListener(_onSettingsProviderUpdated);

    HardwareKeyboard.instance.addHandler(_handleKeyEvent);
  }

  @override
  void dispose() {
    _settingsState?.removeListener(_onSettingsProviderUpdated);
    for (final controller in _textControllers.values) {
      controller.dispose();
    }
    _customColorController.dispose();
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

  Future<bool> _confirmClearFavorites(AppLocalizations l10n) async {
    final res = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.clearFavoritesTitle),
        content: Text(l10n.clearFavoritesConfirm),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(l10n.commonConfirm),
          ),
        ],
      ),
    );
    return res ?? false;
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
    final l10n = AppLocalizations.of(context);
    String? initialDirectory;
    if (currentValue.isNotEmpty && File(currentValue).existsSync()) {
      initialDirectory = currentValue;
    }

    final result = await FilePicker.platform.pickFiles(
      dialogTitle: l10n.selectLabel(label),
      initialDirectory: initialDirectory,
    );
    return result?.files.single.path;
  }

  Future<String?> _pickDirectory(String currentValue, String label) async {
    final l10n = AppLocalizations.of(context);
    String? initialDirectory;
    if (currentValue.isNotEmpty && Directory(currentValue).existsSync()) {
      initialDirectory = currentValue;
    }

    return FilePicker.platform.getDirectoryPath(
      dialogTitle: l10n.selectLabelDirectory(label),
      initialDirectory: initialDirectory,
    );
  }

  Future<void> _launchURL(String url) async {
    final uri = Uri.parse(url);
    if (!await launchUrl(uri)) {
      if (mounted) {
        final l10n = AppLocalizations.of(context);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(l10n.couldNotOpenUrl(url)),
          ),
        );
      }
    }
  }

  String _formatCleanupPolicy(
          AppLocalizations l10n, DownloadCleanupPolicy policy) =>
      switch (policy) {
        DownloadCleanupPolicy.deleteAfterInstall =>
          l10n.settingsCleanupDeleteAfterInstall,
        DownloadCleanupPolicy.keepOneVersion =>
          l10n.settingsCleanupKeepOneVersion,
        DownloadCleanupPolicy.keepTwoVersions =>
          l10n.settingsCleanupKeepTwoVersions,
        DownloadCleanupPolicy.keepAllVersions =>
          l10n.settingsCleanupKeepAllVersions,
      };

  String _formatNavigationRailLabelVisibility(
    AppLocalizations l10n,
    NavigationRailLabelVisibility visibility,
  ) =>
      switch (visibility) {
        NavigationRailLabelVisibility.selected =>
          l10n.settingsNavigationRailLabelsSelected,
        NavigationRailLabelVisibility.all =>
          l10n.settingsNavigationRailLabelsAll,
      };

  // String _formatConnectionType(AppLocalizations l10n, ConnectionType type) =>
  //     switch (type) {
  //       ConnectionType.usb => l10n.settingsConnectionUsb,
  //       ConnectionType.wireless => l10n.settingsConnectionWireless,
  //     };

  @override
  Widget build(BuildContext context) {
    final settingsState = Provider.of<SettingsState>(context);
    final l10n = AppLocalizations.of(context);
    final error = settingsState.error;

    if (error != null) {
      return _buildErrorView(l10n, error);
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
          child: _buildHeader(l10n),
        ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(SettingsConstants.padding),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: _buildSettingsSections(l10n, settingsState),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildErrorView(AppLocalizations l10n, String error) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            l10n.settingsErrorLoading,
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: SettingsConstants.verticalSpacing),
          Text(error),
        ],
      ),
    );
  }

  Widget _buildHeader(AppLocalizations l10n) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Text(
          l10n.settingsTitle,
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
                        ? l10n.settingsResetToDefaults
                        : l10n.settingsRevertChangesTooltip,
                  ),
                );
              },
            ),
            const SizedBox(width: SettingsConstants.verticalSpacing),
            FilledButton.icon(
              onPressed: _hasChanges ? _saveSettings : null,
              icon: const Icon(Icons.save),
              label: Text(l10n.settingsSaveChanges),
            ),
          ],
        ),
      ],
    );
  }

  List<Widget> _buildSettingsSections(
      AppLocalizations l10n, SettingsState settingsState) {
    return [
      _buildSection(
        title: l10n.settingsSectionAppearance,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(
                vertical: SettingsConstants.verticalSpacing),
            child: DropdownButtonFormField<ThemePreference>(
              initialValue: _currentFormSettings.themePreference,
              items: [
                DropdownMenuItem(
                  value: ThemePreference.auto,
                  child: Text(l10n.themeAuto),
                ),
                DropdownMenuItem(
                  value: ThemePreference.dark,
                  child: Text(l10n.themeDark),
                ),
                DropdownMenuItem(
                  value: ThemePreference.light,
                  child: Text(l10n.themeLight),
                ),
              ],
              onChanged: (value) {
                if (value == null) return;
                settingsState.setThemePreference(value);
                setState(() {
                  _currentFormSettings =
                      _currentFormSettings.copyWith(themePreference: value);
                  _hasChanges = false;
                });
              },
              decoration: InputDecoration(
                labelText: l10n.settingsTheme,
                border: const OutlineInputBorder(),
              ),
            ),
          ),
          SwitchListTile(
            title: Text(l10n.settingsUseSystemColor),
            value: settingsState.settings.useSystemColor,
            onChanged: (v) {
              settingsState.setUseSystemColor(v);
              setState(() {
                _currentFormSettings =
                    _currentFormSettings.copyWith(useSystemColor: v);
                _hasChanges = false;
              });
            },
          ),
          _buildSeedColorSelector(l10n, settingsState),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: l10n.settingsSectionGeneral,
        children: [
          _buildDropdownSetting<String>(
            label: l10n.settingsLanguage,
            value: settingsState.settings.localeCode.isEmpty
                ? 'system'
                : settingsState.settings.localeCode,
            items: [
              DropdownMenuItem(
                  value: 'system', child: Text(l10n.settingsSystemDefault)),
              DropdownMenuItem(value: 'en', child: Text(l10n.languageEnglish)),
              DropdownMenuItem(value: 'ru', child: Text(l10n.languageRussian)),
            ],
            onChanged: (code) {
              if (code != null) {
                settingsState.setLocaleCode(code);
              }
            },
          ),
          _buildDropdownSetting<NavigationRailLabelVisibility>(
            label: l10n.settingsNavigationRailLabels,
            value: _currentFormSettings.navigationRailLabelVisibility,
            items: NavigationRailLabelVisibility.values
                .map((visibility) => DropdownMenuItem(
                      value: visibility,
                      child: Text(_formatNavigationRailLabelVisibility(
                          l10n, visibility)),
                    ))
                .toList(),
            onChanged: (value) {
              if (value != null) {
                setState(() => _currentFormSettings = _currentFormSettings
                    .copyWith(navigationRailLabelVisibility: value));
                _checkForChanges();
              }
            },
          ),
          _buildDropdownSetting<String>(
            label: l10n.settingsStartupPage,
            value: _currentFormSettings.startupPageKey,
            items: _pageOptions
                .map((page) => DropdownMenuItem(
                      value: page.key,
                      child: Text(page.label(l10n)),
                    ))
                .toList(),
            onChanged: (value) {
              if (value != null) {
                setState(() => _currentFormSettings =
                    _currentFormSettings.copyWith(startupPageKey: value));
                _checkForChanges();
              }
            },
          ),
          const Divider(height: 24),
          Consumer<SettingsState>(builder: (context, settings, _) {
            final hasFavorites = settings.favoritePackages.isNotEmpty;
            return Align(
              alignment: Alignment.centerLeft,
              child: FilledButton.tonalIcon(
                onPressed: hasFavorites
                    ? () async {
                        final confirmed = await _confirmClearFavorites(l10n);
                        if (!confirmed) return;
                        settings.clearFavorites();
                      }
                    : null,
                icon: const Icon(Icons.star_outline),
                label: Text(l10n.clearFavorites),
              ),
            );
          }),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: l10n.settingsSectionStorage,
        children: [
          _buildPathSetting(
            field: SettingTextField.downloadsLocation,
            label: l10n.settingsDownloadsLocation,
            isDirectory: true,
            currentValue: _currentFormSettings.downloadsLocation,
          ),
          _buildPathSetting(
            field: SettingTextField.backupsLocation,
            label: l10n.settingsBackupsLocation,
            isDirectory: true,
            currentValue: _currentFormSettings.backupsLocation,
          ),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: l10n.settingsSectionAdb,
        children: [
          _buildPathSetting(
            field: SettingTextField.adbPath,
            label: l10n.settingsAdbPath,
            isDirectory: false,
            currentValue: _currentFormSettings.adbPath,
          ),
          // TODO: implement
          // _buildDropdownSetting<ConnectionType>(
          //   label: l10n.settingsPreferredConnection,
          //   value: _currentFormSettings.preferredConnectionType,
          //   items: ConnectionType.values.map((type) {
          //     return DropdownMenuItem(
          //       value: type,
          //       child: Text(_formatConnectionType(l10n, type)),
          //     );
          //   }).toList(),
          //   onChanged: (value) {
          //     if (value != null) {
          //       setState(() => _currentFormSettings = _currentFormSettings
          //           .copyWith(preferredConnectionType: value));
          //       _checkForChanges();
          //     }
          //   },
          // ),
          _buildDropdownSetting<ConnectionType?>(
            label: l10n.settingsPreferredConnection,
            value: null,
            items: [],
            onChanged: null,
            disabledHint: const Text('Not implemented'),
          ),
          if (Platform.isWindows)
            Padding(
              padding:
                  const EdgeInsets.only(top: SettingsConstants.verticalSpacing),
              child: _buildCastingToolCard(context),
            ),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: l10n.settingsSectionDownloader,
        children: [
          _buildPathSetting(
            field: SettingTextField.rclonePath,
            label: l10n.settingsRclonePath,
            isDirectory: false,
            currentValue: _currentFormSettings.rclonePath,
          ),
          _buildRcloneRemoteSelector(l10n),
          // TODO: implement
          _buildTextSetting(
            field: SettingTextField.bandwidthLimit,
            enabled: false,
            label: l10n.settingsBandwidthLimit,
            // helperText:
            //     'Value in KiB/s or with B|K|M|G|T|P suffix (empty for no limit)',
            helper: InkWell(
              onTap: () =>
                  _launchURL('https://rclone.org/docs/#bwlimit-bandwidth-spec'),
              child: Text(
                l10n.settingsBandwidthHelper,
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: Theme.of(context).hintColor,
                    ),
              ),
            ),
          ),
          _buildDropdownSetting<DownloadCleanupPolicy>(
            label: l10n.settingsDownloadsCleanup,
            value: _currentFormSettings.cleanupPolicy,
            items: DownloadCleanupPolicy.values.map((policy) {
              return DropdownMenuItem(
                value: policy,
                child: Text(_formatCleanupPolicy(l10n, policy)),
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
          const SizedBox(height: SettingsConstants.verticalSpacing),
          SwitchListTile(
            title: Text(l10n.settingsWriteLegacyReleaseJson),
            subtitle: Text(l10n.settingsWriteLegacyReleaseJsonHelp),
            value: _currentFormSettings.writeLegacyReleaseJson,
            onChanged: (v) {
              setState(() {
                _currentFormSettings =
                    _currentFormSettings.copyWith(writeLegacyReleaseJson: v);
                _checkForChanges();
              });
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

  Widget _buildCastingToolCard(BuildContext context) {
    // Request status once when this card first builds
    if (!_castingStatusRequested) {
      _castingStatusRequested = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        const GetCastingStatusRequest().sendSignalToRust();
      });
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            const Icon(Icons.cast),
            const SizedBox(width: 8),
            Expanded(
              child: Text(AppLocalizations.of(context).castingToolTitle),
            ),
            FilledButton.tonal(
              onPressed: () async {
                final confirmed = await showDialog<bool>(
                  context: context,
                  builder: (ctx) => AlertDialog(
                    title: Text(AppLocalizations.of(context)
                        .castingToolInstallUpdateTitle),
                    content: Text(AppLocalizations.of(context)
                        .castingToolInstallUpdateDesc),
                    actions: [
                      TextButton(
                        onPressed: () => Navigator.pop(ctx, false),
                        child: Text(AppLocalizations.of(context).commonCancel),
                      ),
                      FilledButton(
                        onPressed: () => Navigator.pop(ctx, true),
                        child:
                            Text(AppLocalizations.of(context).commonDownload),
                      ),
                    ],
                  ),
                );
                if (confirmed == true) {
                  const DownloadCastingBundleRequest(url: null)
                      .sendSignalToRust();
                  if (!context.mounted) return;
                  final l10n = AppLocalizations.of(context);
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: Text(l10n.castingToolDownloading)),
                  );
                }
              },
              child:
                  Text(AppLocalizations.of(context).castingToolDownloadUpdate),
            ),
          ],
        ),
        const SizedBox(height: 8),
        StreamBuilder(
          stream: CastingStatusChanged.rustSignalStream,
          builder: (context, snapshot) {
            final msg = snapshot.data?.message;
            final installed = msg?.installed == true;
            final path = msg?.exePath ?? '';
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(installed ? Icons.check_circle : Icons.info_outline,
                        size: 16,
                        color: installed
                            ? Colors.green
                            : Theme.of(context).colorScheme.secondary),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        installed
                            ? '${AppLocalizations.of(context).castingToolStatusInstalled}${path.isNotEmpty ? ' • $path' : ''}'
                            : AppLocalizations.of(context)
                                .castingToolStatusNotInstalled,
                        style: Theme.of(context).textTheme.bodySmall,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    IconButton(
                      tooltip:
                          AppLocalizations.of(context).castingToolRefresh,
                      onPressed: () => const GetCastingStatusRequest()
                          .sendSignalToRust(),
                      icon: const Icon(Icons.refresh, size: 18),
                    ),
                  ],
                ),
                const SizedBox(height: 6),
                StreamBuilder(
                  stream: CastingDownloadProgress.rustSignalStream,
                  builder: (context, snap2) {
                    final prog = snap2.data?.message;
                    if (installed || prog == null) return const SizedBox.shrink();
                    final total = prog.total?.toInt().toDouble();
                    final received = prog.received.toInt().toDouble();
                    final value = total == null || total == 0
                        ? null
                        : math.min(1.0, math.max(0.0, received / total));
                    if (value == 1.0) return const SizedBox.shrink();
                    final percent =
                        value == null ? null : (value * 100).round();
                    final l10n = AppLocalizations.of(context);
                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        LinearProgressIndicator(value: value),
                        const SizedBox(height: 4),
                        Text(
                          percent == null
                              ? l10n.castingToolDownloading
                              : '${l10n.castingToolDownloading} ($percent%)',
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ],
                    );
                  },
                ),
              ],
            );
          },
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
    final l10n = AppLocalizations.of(context);
    return _buildTextSetting(
      field: field,
      label: label,
      trailing: IconButton.filledTonal(
        icon: const Icon(Icons.folder_open),
        onPressed: () => _pickPath(field, isDirectory, currentValue, label),
        tooltip: l10n.settingsBrowse,
      ),
    );
  }

  Widget _buildTextSetting({
    required SettingTextField field,
    required String label,
    Widget? trailing,
    String? helperText,
    Widget? helper,
    bool enabled = true,
  }) {
    final controller = _textControllers[field];

    return Padding(
      padding: const EdgeInsets.symmetric(
          vertical: SettingsConstants.verticalSpacing),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              enabled: enabled,
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
    required ValueChanged<T?>? onChanged,
    Widget? disabledHint,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(
          vertical: SettingsConstants.verticalSpacing),
      child: DropdownButtonFormField<T>(
        disabledHint: disabledHint,
        initialValue: value,
        items: items,
        onChanged: onChanged,
        decoration: InputDecoration(
          labelText: label,
          border: const OutlineInputBorder(),
        ),
      ),
    );
  }

  Widget _buildRcloneRemoteSelector(AppLocalizations l10n) {
    final settingsState = _settingsState!;
    final remotes = settingsState.rcloneRemotes;
    const customValue = '__custom__';

    final currentRemote = _currentFormSettings.rcloneRemoteName;
    final isCurrentInList = remotes.contains(currentRemote);
    final shouldUseCustom = _rcloneRemoteCustom ||
        settingsState.isRemotesLoading ||
        remotes.isEmpty ||
        !isCurrentInList;
    final dropdownValue = shouldUseCustom ? customValue : currentRemote;

    final l10n = AppLocalizations.of(context);
    final items = <DropdownMenuItem<String>>[
      ...remotes.map((r) => DropdownMenuItem(value: r, child: Text(r))),
      DropdownMenuItem(
          value: customValue, child: Text(l10n.settingsCustomInput)),
    ];

    return Padding(
      padding: const EdgeInsets.symmetric(
          vertical: SettingsConstants.verticalSpacing),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Status (error/warning) above the selector
          if (settingsState.remotesError != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: _statusText(
                icon: Icons.error_outline,
                color: Theme.of(context).colorScheme.error,
                text:
                    '${l10n.settingsFailedToLoadRemotes}: ${settingsState.remotesError ?? ''}',
              ),
            )
          else if (!settingsState.isRemotesLoading && remotes.isEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: _statusText(
                icon: Icons.warning_amber_rounded,
                color: Colors.amber,
                text: l10n.settingsNoRemotesFound,
              ),
            ),
          Row(
            children: [
              Expanded(
                child: DropdownButtonFormField<String>(
                  initialValue: dropdownValue,
                  items: items,
                  onChanged: (value) {
                    if (value == null) return;
                    if (value == customValue) {
                      // Switch to custom mode, keep existing text
                      setState(() {
                        _rcloneRemoteCustom = true;
                      });
                    } else {
                      setState(() {
                        _rcloneRemoteCustom = false;
                      });
                      _updateSetting(SettingTextField.rcloneRemoteName, value,
                          updateController: true);
                    }
                  },
                  decoration: InputDecoration(
                    labelText: l10n.settingsRcloneRemote,
                    border: const OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: SettingsConstants.verticalSpacing),
              SizedBox(
                height: SettingsConstants.iconButtonSize,
                width: SettingsConstants.iconButtonSize,
                child: settingsState.isRemotesLoading
                    ? const Center(
                        child: SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        ),
                      )
                    : IconButton.filledTonal(
                        onPressed: () {
                          settingsState.refreshRcloneRemotes();
                        },
                        iconSize: SettingsConstants.iconSize,
                        tooltip: l10n.refresh,
                        icon: const Icon(Icons.refresh),
                      ),
              ),
            ],
          ),
          if (dropdownValue == customValue)
            Padding(
              padding:
                  const EdgeInsets.only(top: SettingsConstants.verticalSpacing),
              child: TextField(
                controller: _textControllers[SettingTextField.rcloneRemoteName],
                decoration: InputDecoration(
                  labelText: l10n.settingsCustomRemoteName,
                  border: const OutlineInputBorder(),
                ),
                onChanged: (value) =>
                    _updateSetting(SettingTextField.rcloneRemoteName, value),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildSeedColorSelector(
      AppLocalizations l10n, SettingsState settingsState) {
    final customValue = app_theme.kCustomColorKey;
    final currentKey = _currentFormSettings.seedColorKey;
    final isCustomColor = app_theme.isHexColor(currentKey);
    final shouldUseCustom = _seedColorCustom || isCustomColor;
    final dropdownValue = shouldUseCustom ? customValue : currentKey;

    // Initialize custom color text field if needed
    if (isCustomColor && !_seedColorCustom) {
      final hex = currentKey.substring(app_theme.kCustomColorKey.length);
      _customColorController.text = hex.toUpperCase();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        setState(() {
          _seedColorCustom = true;
        });
      });
    }

    final items = <DropdownMenuItem<String>>[
      ...app_theme.kSeedColorPalette.keys.map((key) {
        final color = app_theme.seedFromKey(key);
        return DropdownMenuItem(
          value: key,
          child: Row(
            children: [
              Container(
                width: 16,
                height: 16,
                decoration: BoxDecoration(
                  color: color,
                  shape: BoxShape.circle,
                  border: Border.all(color: Colors.black12),
                ),
              ),
              const SizedBox(width: 8),
              Text(app_theme.seedLabel(l10n, key)),
            ],
          ),
        );
      }),
      DropdownMenuItem(
        value: customValue,
        child: Row(
          children: [
            Container(
              width: 16,
              height: 16,
              decoration: BoxDecoration(
                color: isCustomColor
                    ? app_theme.seedFromKey(currentKey)
                    : Colors.grey,
                shape: BoxShape.circle,
                border: Border.all(color: Colors.black12),
              ),
            ),
            const SizedBox(width: 8),
            Text(l10n.settingsCustomInput),
          ],
        ),
      ),
    ];

    return Padding(
      padding: const EdgeInsets.symmetric(
          vertical: SettingsConstants.verticalSpacing),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          DropdownButtonFormField<String>(
            initialValue: dropdownValue,
            items: items,
            onChanged: settingsState.settings.useSystemColor
                ? null
                : (value) {
                    if (value == null) return;
                    if (value == customValue) {
                      setState(() {
                        _seedColorCustom = true;
                      });
                    } else {
                      setState(() {
                        _seedColorCustom = false;
                      });
                      settingsState.setSeedColorKey(value);
                      setState(() {
                        _currentFormSettings =
                            _currentFormSettings.copyWith(seedColorKey: value);
                        _hasChanges = false;
                      });
                    }
                  },
            decoration: InputDecoration(
              labelText: l10n.settingsSeedColor,
              border: const OutlineInputBorder(),
            ),
          ),
          if (shouldUseCustom)
            Padding(
              padding:
                  const EdgeInsets.only(top: SettingsConstants.verticalSpacing),
              child: TextField(
                controller: _customColorController,
                enabled: !settingsState.settings.useSystemColor,
                decoration: InputDecoration(
                  labelText: l10n.settingsCustomInput,
                  hintText: 'FF5733',
                  prefixText: '#',
                  border: const OutlineInputBorder(),
                  helperText: l10n.settingsCustomColorHint,
                  errorText: _customColorController.text.isNotEmpty &&
                          app_theme
                                  .parseHexColor(_customColorController.text) ==
                              null
                      ? l10n.settingsInvalidHexColor
                      : null,
                ),
                onChanged: (value) {
                  final normalized =
                      value.replaceAll('#', '').trim().toUpperCase();
                  if (app_theme.parseHexColor(normalized) != null) {
                    final customKey = '${app_theme.kCustomColorKey}$normalized';
                    settingsState.setSeedColorKey(customKey);
                    setState(() {
                      _currentFormSettings = _currentFormSettings.copyWith(
                          seedColorKey: customKey);
                      _hasChanges = false;
                    });
                  } else {
                    setState(() {});
                  }
                },
              ),
            ),
        ],
      ),
    );
  }

  Widget _statusText(
      {required IconData icon, required Color color, required String text}) {
    return Row(
      children: [
        Icon(icon, color: color, size: 18),
        const SizedBox(width: 6),
        Flexible(
          child: Text(
            text,
            style: TextStyle(
                color: color, fontSize: 12, fontWeight: FontWeight.w600),
            overflow: TextOverflow.ellipsis,
          ),
        ),
      ],
    );
  }
}
