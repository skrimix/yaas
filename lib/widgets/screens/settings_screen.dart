import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import '../../utils/theme_utils.dart' as app_theme;
import 'package:flutter/services.dart';
import 'package:file_picker/file_picker.dart';
import 'package:provider/provider.dart';
import '../../src/bindings/bindings.dart';
import 'package:url_launcher/url_launcher.dart';
import '../../providers/settings_state.dart';
import '../../navigation.dart';
import '../../src/l10n/app_localizations.dart';
import '../../utils/utils.dart';
import '../../utils/sideload_utils.dart';
import '../common/selectable_link_text.dart';
import '../common/setting_row.dart';
import '../dialogs/downloader_setup_dialog.dart';

enum SettingTextField {
  rcloneRemoteName,
  adbPath,
  downloadsLocation,
  backupsLocation,
  bandwidthLimit,
}

class SettingsConstants {
  static const double sectionSpacing = 24.0;
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
  bool _seedColorCustom = false;
  final TextEditingController _customColorController = TextEditingController();
  bool _castingStatusRequested = false;
  VoidCallback? _unregisterSaveErrorListener;

  @override
  void initState() {
    super.initState();

    final settingsState = Provider.of<SettingsState>(context, listen: false);
    _settingsState = settingsState;
    _originalSettings = settingsState.settings;
    _currentFormSettings = _originalSettings.copyWith();
    _originalSettings = settingsState.settings;
    _currentFormSettings = _originalSettings.copyWith();

    for (final setting in SettingTextField.values) {
      _textControllers[setting] = TextEditingController();
    }
    _updateAllControllers();

    settingsState.addListener(_onSettingsProviderUpdated);
    _unregisterSaveErrorListener =
        settingsState.addSaveErrorListener(_onSettingsSaveError);

    HardwareKeyboard.instance.addHandler(_handleKeyEvent);
  }

  void _onSettingsSaveError(String error) {
    if (!mounted) return;
    final l10n = AppLocalizations.of(context);
    SideloadUtils.showErrorToast(context, l10n.settingsSaveError(error));
  }

  @override
  void dispose() {
    _settingsState?.removeListener(_onSettingsProviderUpdated);
    _unregisterSaveErrorListener?.call();
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

    final result = await FilePicker.pickFile(
      dialogTitle: l10n.selectLabel(label),
      initialDirectory: initialDirectory,
    );
    return result?.path;
  }

  Future<String?> _pickDirectory(String currentValue, String label) async {
    final l10n = AppLocalizations.of(context);
    String? initialDirectory;
    if (currentValue.isNotEmpty && Directory(currentValue).existsSync()) {
      initialDirectory = currentValue;
    }

    return FilePicker.getDirectoryPath(
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

  Future<void> _showDownloaderSourcesDialog() async {
    await showDialog<void>(
      context: context,
      builder: (ctx) => const DownloaderSetupDialog(),
    );
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

  String _formatDownloadMode(AppLocalizations l10n, DownloadMode mode) =>
      switch (mode) {
        DownloadMode.staged => l10n.settingsDownloadModeStaged,
        DownloadMode.streamed => l10n.settingsDownloadModeStreamed,
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

  String _formatConnectionKind(AppLocalizations l10n, ConnectionKind type) =>
      switch (type) {
        ConnectionKind.usb => l10n.settingsConnectionUsb,
        ConnectionKind.wireless => l10n.settingsConnectionWireless,
      };

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

    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 1000),
        child: Column(
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
        ),
      ),
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
    return SizedBox(
      width: double.infinity,
      child: Wrap(
        alignment: WrapAlignment.spaceBetween,
        crossAxisAlignment: WrapCrossAlignment.center,
        spacing: 16,
        runSpacing: 12,
        children: [
          Text(
            l10n.settingsTitle,
            style: Theme.of(context).textTheme.headlineMedium,
          ),
          Row(
            mainAxisSize: MainAxisSize.min,
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
                      icon:
                          Icon(isShiftPressed ? Icons.restart_alt : Icons.undo),
                      tooltip: isShiftPressed
                          ? l10n.settingsResetToDefaults
                          : l10n.settingsRevertChangesTooltip,
                    ),
                  );
                },
              ),
              const SizedBox(width: SettingsConstants.verticalSpacing),
              Flexible(
                child: FilledButton.icon(
                  onPressed: _hasChanges ? _saveSettings : null,
                  icon: const Icon(Icons.save),
                  label: Text(l10n.settingsSaveChanges),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  List<Widget> _buildSettingsSections(
      AppLocalizations l10n, SettingsState settingsState) {
    final sections = <Widget>[
      _buildSection(
        title: l10n.settingsSectionAppearance,
        children: [
          _buildDropdownSetting<ThemePreference>(
            label: l10n.settingsTheme,
            value: _currentFormSettings.themePreference,
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
          ),
          _buildSwitchSetting(
            label: l10n.settingsUseSystemColor,
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
            items: _computeStartupPageOptions(settingsState, l10n)
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
          Consumer<SettingsState>(builder: (context, settings, _) {
            final hasFavorites = settings.favoritePackages.isNotEmpty;
            return SettingRow(
              label: l10n.settingsFavorites,
              control: Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                  onPressed: hasFavorites
                      ? () async {
                          final confirmed = await _confirmClearFavorites(l10n);
                          if (!confirmed) return;
                          settings.clearFavorites();
                        }
                      : null,
                  child: Text(l10n.commonClear),
                ),
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
          _buildSwitchSetting(
            label: l10n.settingsMdnsAutoConnect,
            description: l10n.settingsMdnsAutoConnectHelp,
            value: _currentFormSettings.mdnsAutoConnect,
            onChanged: (v) {
              setState(() {
                _currentFormSettings =
                    _currentFormSettings.copyWith(mdnsAutoConnect: v);
                _checkForChanges();
              });
            },
          ),
          _buildSwitchSetting(
            label: l10n.settingsAutoReinstallOnConflict,
            description: l10n.settingsAutoReinstallOnConflictHelp,
            value: _currentFormSettings.autoReinstallOnConflict,
            onChanged: (v) {
              setState(() {
                _currentFormSettings =
                    _currentFormSettings.copyWith(autoReinstallOnConflict: v);
                _checkForChanges();
              });
            },
          ),
          _buildDropdownSetting<ConnectionKind>(
            label: l10n.settingsPreferredConnection,
            value: _currentFormSettings.preferredConnectionType,
            items: ConnectionKind.values.map((type) {
              return DropdownMenuItem(
                value: type,
                child: Text(_formatConnectionKind(l10n, type)),
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
          if (Platform.isWindows) _buildCastingToolCard(context),
        ],
      ),
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: l10n.settingsSectionExperimental,
        children: [
          _buildSwitchSetting(
            label: l10n.settingsExperimentalNativeCast,
            description: l10n.settingsExperimentalNativeCastWarning,
            value: settingsState.settings.experimentalNativeCast,
            onChanged: (v) {
              settingsState.setExperimentalNativeCast(v);
              setState(() {
                _currentFormSettings =
                    _currentFormSettings.copyWith(experimentalNativeCast: v);
                _hasChanges = false;
              });
            },
          ),
        ],
      ),
    ];

    // Downloader section
    sections.addAll([
      const SizedBox(height: SettingsConstants.sectionSpacing),
      _buildSection(
        title: l10n.settingsSectionDownloader,
        children: [
          if (settingsState.isDownloaderInitializing)
            _buildDownloaderInitBanner(l10n, settingsState),
          if (!settingsState.isDownloaderInitializing &&
              settingsState.downloaderError != null)
            _buildDownloaderErrorBanner(l10n, settingsState.downloaderError!),
          _buildDownloaderSourceSummary(l10n, settingsState),
          if (settingsState.isDownloaderAvailable) ...[
            if (settingsState.downloaderSupportsRemoteSelection)
              _buildRcloneRemoteSelector(l10n),
            if (settingsState.downloaderSupportsBandwidthLimit)
              _buildTextSetting(
                field: SettingTextField.bandwidthLimit,
                label: l10n.settingsBandwidthLimit,
                helper: InkWell(
                  onTap: () => _launchURL(
                      'https://rclone.org/docs/#bwlimit-bwtimetable'),
                  child: Text(
                    l10n.settingsBandwidthHelper,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Theme.of(context).colorScheme.primary,
                          decoration: TextDecoration.underline,
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
            if (settingsState.downloaderSupportsDownloadModeSelection)
              _buildDownloadModeSetting(l10n),
            _buildSwitchSetting(
              label: l10n.settingsWriteLegacyReleaseJson,
              description: l10n.settingsWriteLegacyReleaseJsonHelp,
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
        ],
      ),
    ]);

    return sections;
  }

  List<StartupPageOption> _computeStartupPageOptions(
      SettingsState settingsState, AppLocalizations l10n) {
    final all = AppPageRegistry.pages
        .map((page) => (key: page.key, label: page.label))
        .toList(growable: false);
    if (settingsState.isDownloaderAvailable) return all;
    return all
        .where((e) => e.key != 'download' && e.key != 'downloads')
        .toList(growable: false);
  }

  Widget _buildSection({
    required String title,
    required List<Widget> children,
  }) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(left: 16, bottom: 8),
          child: Text(title, style: Theme.of(context).textTheme.titleSmall),
        ),
        Card(
          margin: EdgeInsets.zero,
          elevation: 0,
          clipBehavior: Clip.antiAlias,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (var i = 0; i < children.length; i++) ...[
                if (i > 0)
                  Divider(
                    height: 1,
                    indent: 16,
                    endIndent: 16,
                    color: Theme.of(context)
                        .colorScheme
                        .outlineVariant
                        .withValues(alpha: 0.45),
                  ),
                children[i],
              ],
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildSwitchSetting({
    required String label,
    String? description,
    required bool value,
    required ValueChanged<bool> onChanged,
  }) {
    return MergeSemantics(
      child: InkWell(
        onTap: () => onChanged(!value),
        child: SettingRow(
          label: label,
          description: description == null ? null : Text(description),
          compact: true,
          control: Switch(value: value, onChanged: onChanged),
        ),
      ),
    );
  }

  InputDecoration _controlDecoration({String? hintText}) {
    final colors = Theme.of(context).colorScheme;
    final border = OutlineInputBorder(
      borderRadius: BorderRadius.circular(8),
      borderSide: BorderSide.none,
    );
    return InputDecoration(
      hintText: hintText,
      filled: true,
      fillColor: colors.surfaceContainerHighest.withValues(alpha: 0.55),
      isDense: true,
      contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
      border: border,
      enabledBorder: border,
      disabledBorder: border,
      focusedBorder: border.copyWith(
        borderSide: BorderSide(color: colors.primary, width: 2),
      ),
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

    return SettingRow(
      label: AppLocalizations.of(context).castingToolTitle,
      control: Align(
        alignment: Alignment.centerRight,
        child: TextButton(
          onPressed: () async {
            final confirmed = await showDialog<bool>(
              context: context,
              builder: (ctx) => AlertDialog(
                title: Text(
                    AppLocalizations.of(context).castingToolInstallUpdateTitle),
                content: Text(
                    AppLocalizations.of(context).castingToolInstallUpdateDesc),
                actions: [
                  TextButton(
                    onPressed: () => Navigator.pop(ctx, false),
                    child: Text(AppLocalizations.of(context).commonCancel),
                  ),
                  FilledButton(
                    onPressed: () => Navigator.pop(ctx, true),
                    child: Text(AppLocalizations.of(context).commonDownload),
                  ),
                ],
              ),
            );
            if (confirmed == true) {
              const DownloadCastingBundleRequest().sendSignalToRust();
              if (!context.mounted) return;
              final l10n = AppLocalizations.of(context);
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text(l10n.castingToolDownloading)),
              );
            }
          },
          child: Text(AppLocalizations.of(context).castingToolDownloadUpdate),
        ),
      ),
      footer: StreamBuilder(
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
                    tooltip: AppLocalizations.of(context).castingToolRefresh,
                    onPressed: () =>
                        const GetCastingStatusRequest().sendSignalToRust(),
                    icon: const Icon(Icons.refresh, size: 18),
                  ),
                ],
              ),
              const SizedBox(height: 6),
              StreamBuilder(
                stream: CastingDownloadProgress.rustSignalStream,
                builder: (context, snap2) {
                  final prog = snap2.data?.message;
                  if (installed || prog == null) {
                    return const SizedBox.shrink();
                  }
                  final total = prog.total?.toInt().toDouble();
                  final received = prog.received.toInt().toDouble();
                  final value = total == null || total == 0
                      ? null
                      : math.min(1.0, math.max(0.0, received / total));
                  final percent = value == null ? null : (value * 100).round();
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
    );
  }

  Widget _buildDownloaderInitBanner(
    AppLocalizations l10n,
    SettingsState settingsState,
  ) {
    return SettingRow(
      label: l10n.preparingDownloader,
      description: settingsState.isDownloaderInitDownloadActive
          ? Text(l10n.downloadingRcloneFiles)
          : null,
      compact: true,
      control: const SizedBox(
        width: 20,
        height: 20,
        child: CircularProgressIndicator(strokeWidth: 2),
      ),
      footer:
          LinearProgressIndicator(value: settingsState.downloaderInitProgress),
    );
  }

  Widget _buildDownloaderErrorBanner(
    AppLocalizations l10n,
    String error,
  ) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _statusText(
            icon: Icons.error_outline,
            color: Theme.of(context).colorScheme.error,
            text: error,
            copyable: true,
          ),
          const SizedBox(height: 8),
          Align(
            alignment: Alignment.centerRight,
            child: TextButton.icon(
              onPressed: () =>
                  const RetryDownloaderInitRequest().sendSignalToRust(),
              icon: const Icon(Icons.refresh, size: 18),
              label: Text(l10n.retry),
            ),
          ),
        ],
      ),
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
      fullWidth: true,
      trailing: TextButton.icon(
        icon: const Icon(Icons.folder_open, size: 18),
        onPressed: () => _pickPath(field, isDirectory, currentValue, label),
        label: Text(l10n.settingsBrowse),
      ),
    );
  }

  Widget _buildTextSetting({
    required SettingTextField field,
    required String label,
    Widget? trailing,
    Widget? helper,
    bool fullWidth = false,
  }) {
    return SettingRow(
      label: label,
      description: helper,
      fullWidth: fullWidth,
      control: Row(
        children: [
          Expanded(
            child: Semantics(
              label: label,
              child: TextField(
                controller: _textControllers[field],
                decoration: _controlDecoration(),
                onChanged: (value) => _updateSetting(field, value),
              ),
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

  Widget _dropdownControl<T>({
    required String label,
    required T? value,
    required List<DropdownMenuItem<T>> items,
    required ValueChanged<T?>? onChanged,
    Widget? hint,
  }) {
    return Semantics(
      label: label,
      child: DropdownButtonFormField<T>(
        initialValue: value,
        isExpanded: true,
        itemHeight: null,
        hint: hint,
        items: items,
        selectedItemBuilder: (context) => items
            .map((item) => Align(
                  alignment: Alignment.centerLeft,
                  child: DefaultTextStyle.merge(
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    child: item.child,
                  ),
                ))
            .toList(),
        onChanged: onChanged,
        decoration: _controlDecoration(),
        borderRadius: BorderRadius.circular(8),
      ),
    );
  }

  Widget _buildDropdownSetting<T>({
    required String label,
    required T value,
    required List<DropdownMenuItem<T>> items,
    required ValueChanged<T?>? onChanged,
    String? description,
  }) {
    return SettingRow(
      label: label,
      description: description == null ? null : Text(description),
      enabled: onChanged != null,
      control: _dropdownControl(
        label: label,
        value: value,
        items: items,
        onChanged: onChanged,
      ),
    );
  }

  Widget _buildDownloadModeSetting(AppLocalizations l10n) {
    return _buildDropdownSetting<DownloadMode>(
      label: l10n.settingsDownloadMode,
      description: l10n.settingsDownloadModeHelp,
      value: _currentFormSettings.downloadMode,
      items: DownloadMode.values.map((mode) {
        return DropdownMenuItem(
          value: mode,
          child: Text(_formatDownloadMode(l10n, mode)),
        );
      }).toList(),
      onChanged: (value) {
        if (value == null) return;
        setState(() => _currentFormSettings =
            _currentFormSettings.copyWith(downloadMode: value));
        _checkForChanges();
      },
    );
  }

  Widget _buildRcloneRemoteSelector(AppLocalizations l10n) {
    final settingsState = _settingsState!;
    final remotes = settingsState.rcloneRemotes;
    final currentRemote = _currentFormSettings.rcloneRemoteName;
    final dropdownValue =
        remotes.contains(currentRemote) ? currentRemote : null;
    final error = settingsState.remotesError;

    return SettingRow(
      label: l10n.settingsRcloneRemote,
      control: Row(
        children: [
          Expanded(
            child: _dropdownControl<String>(
              label: l10n.settingsRcloneRemote,
              value: dropdownValue,
              hint: currentRemote.isEmpty ? null : Text(currentRemote),
              items: remotes
                  .map((remote) => DropdownMenuItem(
                        value: remote,
                        child: Text(remote),
                      ))
                  .toList(),
              onChanged: settingsState.isRemotesLoading || remotes.isEmpty
                  ? null
                  : (value) {
                      if (value == null) return;
                      _updateSetting(SettingTextField.rcloneRemoteName, value,
                          updateController: true);
                    },
            ),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 40,
            height: 40,
            child: settingsState.isRemotesLoading
                ? const Center(
                    child: SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                  )
                : IconButton(
                    onPressed: settingsState.refreshRcloneRemotes,
                    tooltip: l10n.refresh,
                    icon: const Icon(Icons.refresh, size: 20),
                  ),
          ),
        ],
      ),
      footer: error != null
          ? _statusText(
              icon: Icons.error_outline,
              color: Theme.of(context).colorScheme.error,
              text: '${l10n.settingsFailedToLoadRemotes}: $error',
              copyable: true,
            )
          : !settingsState.isRemotesLoading && remotes.isEmpty
              ? _statusText(
                  icon: Icons.info_outline,
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                  text: l10n.settingsNoRemotesFound,
                )
              : null,
    );
  }

  Widget _buildDownloaderSourceSummary(
    AppLocalizations l10n,
    SettingsState settingsState,
  ) {
    final selectedSource = settingsState.activeDownloaderConfig;
    return SettingRow(
      label: l10n.settingsDownloaderSource,
      description: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(selectedSource?.displayName ?? l10n.downloaderSourceNoSelection),
          if (selectedSource != null &&
              selectedSource.description.isNotEmpty) ...[
            const SizedBox(height: 4),
            SelectableLinkText(text: selectedSource.description),
          ],
        ],
      ),
      control: Align(
        alignment: Alignment.centerRight,
        child: TextButton(
          onPressed: _showDownloaderSourcesDialog,
          child: Text(l10n.installDownloaderConfigFromUrl),
        ),
      ),
      footer: settingsState.downloaderSourcesError == null
          ? null
          : _statusText(
              icon: Icons.error_outline,
              color: Theme.of(context).colorScheme.error,
              text: settingsState.downloaderSourcesError!,
              copyable: true,
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
              Flexible(child: Text(app_theme.seedLabel(l10n, key))),
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
            Flexible(child: Text(l10n.settingsCustomInput)),
          ],
        ),
      ),
    ];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _buildDropdownSetting<String>(
          label: l10n.settingsSeedColor,
          value: dropdownValue,
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
        ),
        if (shouldUseCustom)
          SettingRow(
            label: l10n.settingsCustomInput,
            description: Text(l10n.settingsCustomColorHint),
            enabled: !settingsState.settings.useSystemColor,
            control: Semantics(
              label: l10n.settingsCustomInput,
              child: TextField(
                controller: _customColorController,
                enabled: !settingsState.settings.useSystemColor,
                decoration: _controlDecoration(hintText: 'FF5733').copyWith(
                  prefixText: '#',
                  errorMaxLines: 2,
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
          ),
      ],
    );
  }

  Widget _statusText(
      {required IconData icon,
      required Color color,
      required String text,
      bool copyable = false}) {
    final style =
        TextStyle(color: color, fontSize: 12, fontWeight: FontWeight.w600);
    return Row(
      children: [
        Icon(icon, color: color, size: 18),
        const SizedBox(width: 6),
        Flexible(
          child: copyable
              ? buildCopyableText(
                  context,
                  text,
                  style: style,
                )
              : Text(text, style: style),
        ),
      ],
    );
  }
}
