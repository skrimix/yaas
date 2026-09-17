import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import '../../utils/theme_utils.dart' as app_theme;
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
import '../common/settings_tiles.dart';
import '../dialogs/downloader_setup_dialog.dart';

enum SettingTextField {
  rcloneRemoteName,
  adbPath,
  downloadsLocation,
  backupsLocation,
  bandwidthLimit,
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
  static const double _maxContentWidth = 1000;

  final Map<SettingTextField, TextEditingController> _textControllers = {};

  late Settings _originalSettings;
  late Settings _currentFormSettings;
  bool _hasChanges = false;
  SettingsState? _settingsState;
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

    for (final setting in SettingTextField.values) {
      _textControllers[setting] = TextEditingController();
    }
    _updateAllControllers();

    settingsState.addListener(_onSettingsProviderUpdated);
    _unregisterSaveErrorListener =
        settingsState.addSaveErrorListener(_onSettingsSaveError);
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
    super.dispose();
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

    return Scaffold(
      backgroundColor: Colors.transparent,
      bottomNavigationBar: _buildSaveBar(l10n),
      body: LayoutBuilder(
        builder: (context, constraints) {
          final side =
              math.max(16.0, (constraints.maxWidth - _maxContentWidth) / 2);
          final titleInset = math.max(0.0, side - 16);
          final sections = _buildSettingsSections(l10n, settingsState);

          return CustomScrollView(
            slivers: [
              SliverAppBar.large(
                pinned: true,
                title: Padding(
                  padding: EdgeInsets.only(left: titleInset),
                  child: Text(l10n.settingsTitle),
                ),
                actions: [
                  _buildOverflowMenu(l10n),
                  SizedBox(width: titleInset),
                ],
              ),
              SliverPadding(
                padding: EdgeInsets.only(left: side, right: side, bottom: 24),
                sliver: SliverToBoxAdapter(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (var i = 0; i < sections.length; i++) ...[
                        if (i > 0)
                          const SizedBox(
                              height: SettingsMetrics.sectionSpacing),
                        sections[i],
                      ],
                    ],
                  ),
                ),
              ),
            ],
          );
        },
      ),
    );
  }

  Widget _buildOverflowMenu(AppLocalizations l10n) {
    return PopupMenuButton<VoidCallback>(
      tooltip: l10n.commonMoreActions,
      position: PopupMenuPosition.under,
      icon: const Icon(Icons.more_vert),
      onSelected: (action) => action(),
      itemBuilder: (context) => [
        PopupMenuItem<VoidCallback>(
          value: _revertChanges,
          enabled: _hasChanges,
          child: ListTile(
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.undo),
            title: Text(l10n.settingsRevertChanges),
          ),
        ),
        PopupMenuItem<VoidCallback>(
          value: _resetToDefaults,
          child: ListTile(
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.restart_alt),
            title: Text(l10n.settingsResetToDefaults),
          ),
        ),
      ],
    );
  }

  Widget _buildSaveBar(AppLocalizations l10n) {
    final theme = Theme.of(context);
    return AnimatedSize(
      duration: const Duration(milliseconds: 180),
      curve: Curves.easeOutCubic,
      alignment: Alignment.topCenter,
      child: !_hasChanges
          ? const SizedBox(width: double.infinity)
          : Material(
              color: theme.colorScheme.surfaceContainer,
              child: SafeArea(
                top: false,
                child: Padding(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                  child: Center(
                    heightFactor: 1,
                    child: ConstrainedBox(
                      constraints:
                          const BoxConstraints(maxWidth: _maxContentWidth),
                      child: Wrap(
                        alignment: WrapAlignment.spaceBetween,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        spacing: 16,
                        runSpacing: 8,
                        children: [
                          Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Icon(Icons.edit_note,
                                  size: 20, color: theme.colorScheme.primary),
                              const SizedBox(width: 10),
                              Flexible(
                                child: Text(
                                  l10n.settingsUnsavedChanges,
                                  style: theme.textTheme.bodyMedium,
                                ),
                              ),
                            ],
                          ),
                          Wrap(
                            alignment: WrapAlignment.end,
                            spacing: 8,
                            runSpacing: 8,
                            children: [
                              TextButton(
                                onPressed: _revertChanges,
                                child: Text(l10n.settingsRevertChanges),
                              ),
                              FilledButton(
                                onPressed: _saveSettings,
                                child: Text(l10n.settingsSaveChanges),
                              ),
                            ],
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
    );
  }

  Widget _buildErrorView(AppLocalizations l10n, String error) {
    final theme = Theme.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline, size: 40, color: theme.colorScheme.error),
            const SizedBox(height: 12),
            Text(l10n.settingsErrorLoading, style: theme.textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(error, textAlign: TextAlign.center),
          ],
        ),
      ),
    );
  }

  List<Widget> _buildSettingsSections(
      AppLocalizations l10n, SettingsState settingsState) {
    return <Widget>[
      SettingsSection(
        title: l10n.settingsSectionAppearance,
        icon: Icons.palette_outlined,
        children: [
          _buildThemeSetting(l10n, settingsState),
          SettingSwitchTile(
            icon: Icons.format_color_fill,
            title: l10n.settingsUseSystemColor,
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
      SettingsSection(
        title: l10n.settingsSectionGeneral,
        icon: Icons.tune,
        children: [
          SettingChoiceTile<String>(
            icon: Icons.translate,
            title: l10n.settingsLanguage,
            value: settingsState.settings.localeCode.isEmpty
                ? 'system'
                : settingsState.settings.localeCode,
            choices: [
              SettingChoice(value: 'system', label: l10n.settingsSystemDefault),
              SettingChoice(value: 'en', label: l10n.languageEnglish),
              SettingChoice(value: 'ru', label: l10n.languageRussian),
            ],
            onChanged: settingsState.setLocaleCode,
          ),
          SettingChoiceTile<NavigationRailLabelVisibility>(
            icon: Icons.label_outline,
            title: l10n.settingsNavigationRailLabels,
            value: _currentFormSettings.navigationRailLabelVisibility,
            choices: NavigationRailLabelVisibility.values
                .map((visibility) => SettingChoice(
                      value: visibility,
                      label: _formatNavigationRailLabelVisibility(
                          l10n, visibility),
                    ))
                .toList(),
            onChanged: (value) {
              setState(() => _currentFormSettings = _currentFormSettings
                  .copyWith(navigationRailLabelVisibility: value));
              _checkForChanges();
            },
          ),
          SettingChoiceTile<String>(
            icon: Icons.launch,
            title: l10n.settingsStartupPage,
            value: _currentFormSettings.startupPageKey,
            choices: _computeStartupPageOptions(settingsState, l10n)
                .map((page) =>
                    SettingChoice(value: page.key, label: page.label(l10n)))
                .toList(),
            onChanged: (value) {
              setState(() => _currentFormSettings =
                  _currentFormSettings.copyWith(startupPageKey: value));
              _checkForChanges();
            },
          ),
          Consumer<SettingsState>(builder: (context, settings, _) {
            final hasFavorites = settings.favoritePackages.isNotEmpty;
            return SettingTile(
              icon: Icons.star_outline,
              title: l10n.settingsFavorites,
              enabled: hasFavorites,
              trailing: TextButton.icon(
                icon: const Icon(Icons.delete_outline, size: 18),
                onPressed: hasFavorites
                    ? () async {
                        final confirmed = await _confirmClearFavorites(l10n);
                        if (!confirmed) return;
                        settings.clearFavorites();
                      }
                    : null,
                label: Text(l10n.commonClear),
              ),
            );
          }),
        ],
      ),
      SettingsSection(
        title: l10n.settingsSectionStorage,
        icon: Icons.folder_outlined,
        children: [
          _buildPathSetting(
            field: SettingTextField.downloadsLocation,
            icon: Icons.download_outlined,
            label: l10n.settingsDownloadsLocation,
            isDirectory: true,
            currentValue: _currentFormSettings.downloadsLocation,
          ),
          _buildPathSetting(
            field: SettingTextField.backupsLocation,
            icon: Icons.inventory_2_outlined,
            label: l10n.settingsBackupsLocation,
            isDirectory: true,
            currentValue: _currentFormSettings.backupsLocation,
          ),
        ],
      ),
      SettingsSection(
        title: l10n.settingsSectionAdb,
        icon: Icons.phonelink_setup_outlined,
        children: [
          _buildPathSetting(
            field: SettingTextField.adbPath,
            icon: Icons.terminal,
            label: l10n.settingsAdbPath,
            isDirectory: false,
            currentValue: _currentFormSettings.adbPath,
          ),
          _buildConnectionKindSetting(l10n),
          SettingSwitchTile(
            icon: Icons.wifi_tethering,
            title: l10n.settingsMdnsAutoConnect,
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
          SettingSwitchTile(
            icon: Icons.autorenew,
            title: l10n.settingsAutoReinstallOnConflict,
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
          SettingSwitchTile(
            icon: Icons.backup_outlined,
            title: l10n.settingsAutoBackupOnUninstall,
            description: l10n.settingsAutoBackupOnUninstallHelp,
            value: _currentFormSettings.autoBackupOnUninstall,
            onChanged: (v) {
              setState(() {
                _currentFormSettings =
                    _currentFormSettings.copyWith(autoBackupOnUninstall: v);
                _checkForChanges();
              });
            },
          ),
          if (Platform.isWindows) _buildCastingToolTile(context),
        ],
      ),
      SettingsSection(
        title: l10n.settingsSectionDownloader,
        icon: Icons.cloud_outlined,
        children: [
          if (settingsState.isDownloaderInitializing)
            _buildDownloaderInitTile(l10n, settingsState),
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
                icon: Icons.speed_outlined,
                label: l10n.settingsBandwidthLimit,
                description: InkWell(
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
            SettingChoiceTile<DownloadCleanupPolicy>(
              icon: Icons.auto_delete_outlined,
              title: l10n.settingsDownloadsCleanup,
              value: _currentFormSettings.cleanupPolicy,
              choices: DownloadCleanupPolicy.values
                  .map((policy) => SettingChoice(
                        value: policy,
                        label: _formatCleanupPolicy(l10n, policy),
                      ))
                  .toList(),
              onChanged: (value) {
                setState(() => _currentFormSettings =
                    _currentFormSettings.copyWith(cleanupPolicy: value));
                _checkForChanges();
              },
            ),
            if (_currentFormSettings.cleanupPolicy ==
                    DownloadCleanupPolicy.keepOneVersion ||
                _currentFormSettings.cleanupPolicy ==
                    DownloadCleanupPolicy.keepTwoVersions)
              SettingChoiceTile<DownloadCleanupTiming>(
                icon: Icons.schedule,
                title: l10n.settingsCleanupTiming,
                value: _currentFormSettings.cleanupTiming,
                choices: DownloadCleanupTiming.values
                    .map((timing) => SettingChoice(
                          value: timing,
                          label: switch (timing) {
                            DownloadCleanupTiming.afterInstall =>
                              l10n.settingsCleanupAfterInstall,
                            DownloadCleanupTiming.afterDownload =>
                              l10n.settingsCleanupAfterDownload,
                          },
                        ))
                    .toList(),
                onChanged: (value) {
                  setState(() => _currentFormSettings =
                      _currentFormSettings.copyWith(cleanupTiming: value));
                  _checkForChanges();
                },
              ),
            if (settingsState.downloaderSupportsDownloadModeSelection)
              _buildDownloadModeSetting(l10n),
            SettingSwitchTile(
              icon: Icons.description_outlined,
              title: l10n.settingsWriteLegacyReleaseJson,
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
      SettingsSection(
        title: l10n.settingsSectionExperimental,
        icon: Icons.science_outlined,
        children: [
          SettingSwitchTile(
            icon: Icons.cast_connected_outlined,
            title: l10n.settingsExperimentalNativeCast,
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

  Widget _buildThemeSetting(
      AppLocalizations l10n, SettingsState settingsState) {
    return SettingTile(
      icon: Icons.brightness_6_outlined,
      title: l10n.settingsTheme,
      content: Align(
        alignment: Alignment.centerLeft,
        child: SegmentedButton<ThemePreference>(
          showSelectedIcon: false,
          segments: [
            ButtonSegment(
              value: ThemePreference.auto,
              icon: const Icon(Icons.brightness_auto),
              label: Text(l10n.themeAuto),
            ),
            ButtonSegment(
              value: ThemePreference.light,
              icon: const Icon(Icons.light_mode_outlined),
              label: Text(l10n.themeLight),
            ),
            ButtonSegment(
              value: ThemePreference.dark,
              icon: const Icon(Icons.dark_mode_outlined),
              label: Text(l10n.themeDark),
            ),
          ],
          selected: {_currentFormSettings.themePreference},
          onSelectionChanged: (selection) {
            final value = selection.first;
            settingsState.setThemePreference(value);
            setState(() {
              _currentFormSettings =
                  _currentFormSettings.copyWith(themePreference: value);
              _hasChanges = false;
            });
          },
        ),
      ),
    );
  }

  Widget _buildConnectionKindSetting(AppLocalizations l10n) {
    return SettingTile(
      icon: Icons.cable_outlined,
      title: l10n.settingsPreferredConnection,
      content: Align(
        alignment: Alignment.centerLeft,
        child: SegmentedButton<ConnectionKind>(
          showSelectedIcon: false,
          segments: ConnectionKind.values
              .map((kind) => ButtonSegment(
                    value: kind,
                    icon: Icon(kind == ConnectionKind.usb
                        ? Icons.usb
                        : Icons.wifi_outlined),
                    label: Text(_formatConnectionKind(l10n, kind)),
                  ))
              .toList(),
          selected: {_currentFormSettings.preferredConnectionType},
          onSelectionChanged: (selection) {
            setState(() => _currentFormSettings = _currentFormSettings.copyWith(
                preferredConnectionType: selection.first));
            _checkForChanges();
          },
        ),
      ),
    );
  }

  Widget _buildCastingToolTile(BuildContext context) {
    // Request status once when this tile first builds
    if (!_castingStatusRequested) {
      _castingStatusRequested = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        const GetCastingStatusRequest().sendSignalToRust();
      });
    }

    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);

    return StreamBuilder(
      stream: CastingStatusChanged.rustSignalStream,
      builder: (context, snapshot) {
        final msg = snapshot.data?.message;
        final installed = msg?.installed == true;
        final path = msg?.exePath ?? '';

        return SettingTile(
          icon: Icons.cast,
          title: l10n.castingToolTitle,
          description: Row(
            children: [
              Icon(
                installed ? Icons.check_circle : Icons.info_outline,
                size: 16,
                color: installed
                    ? theme.colorScheme.primary
                    : theme.colorScheme.onSurfaceVariant,
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  installed
                      ? '${l10n.castingToolStatusInstalled}${path.isNotEmpty ? ' • $path' : ''}'
                      : l10n.castingToolStatusNotInstalled,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
          trailing: IconButton(
            tooltip: l10n.castingToolRefresh,
            onPressed: () => const GetCastingStatusRequest().sendSignalToRust(),
            icon: const Icon(Icons.refresh, size: 20),
          ),
          content: Align(
            alignment: Alignment.centerLeft,
            child: FilledButton.tonalIcon(
              onPressed: _confirmCastingToolDownload,
              icon: const Icon(Icons.download, size: 18),
              label: Text(l10n.castingToolDownloadUpdate),
            ),
          ),
          footer: StreamBuilder(
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
              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  LinearProgressIndicator(value: value),
                  const SizedBox(height: 6),
                  Text(
                    percent == null
                        ? l10n.castingToolDownloading
                        : '${l10n.castingToolDownloading} ($percent%)',
                    style: theme.textTheme.bodySmall,
                  ),
                ],
              );
            },
          ),
        );
      },
    );
  }

  Future<void> _confirmCastingToolDownload() async {
    final l10n = AppLocalizations.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(l10n.castingToolInstallUpdateTitle),
        content: Text(l10n.castingToolInstallUpdateDesc),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(l10n.commonDownload),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    const DownloadCastingBundleRequest().sendSignalToRust();
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(l10n.castingToolDownloading)),
    );
  }

  Widget _buildDownloaderInitTile(
    AppLocalizations l10n,
    SettingsState settingsState,
  ) {
    return SettingTile(
      icon: Icons.downloading,
      title: l10n.preparingDownloader,
      description: settingsState.isDownloaderInitDownloadActive
          ? Text(l10n.downloadingRcloneFiles)
          : null,
      trailing: const SizedBox(
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
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Container(
        padding: const EdgeInsets.fromLTRB(16, 14, 8, 8),
        decoration: BoxDecoration(
          color: colors.errorContainer,
          borderRadius: BorderRadius.circular(12),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Icon(Icons.error_outline,
                    size: 20, color: colors.onErrorContainer),
                const SizedBox(width: 10),
                Flexible(
                  child: buildCopyableText(
                    context,
                    error,
                    style: TextStyle(color: colors.onErrorContainer),
                  ),
                ),
              ],
            ),
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
      ),
    );
  }

  Widget _buildPathSetting({
    required SettingTextField field,
    required IconData icon,
    required String label,
    required bool isDirectory,
    required String currentValue,
  }) {
    final l10n = AppLocalizations.of(context);
    return _buildTextSetting(
      field: field,
      icon: icon,
      label: label,
      suffixIcon: IconButton(
        icon: const Icon(Icons.folder_open, size: 20),
        tooltip: l10n.settingsBrowse,
        onPressed: () => _pickPath(field, isDirectory, currentValue, label),
      ),
    );
  }

  Widget _buildTextSetting({
    required SettingTextField field,
    required IconData icon,
    required String label,
    Widget? description,
    Widget? suffixIcon,
  }) {
    return SettingTile(
      icon: icon,
      title: label,
      description: description,
      content: Semantics(
        label: label,
        child: TextField(
          controller: _textControllers[field],
          decoration: settingsInputDecoration(context, suffixIcon: suffixIcon),
          onChanged: (value) => _updateSetting(field, value),
        ),
      ),
    );
  }

  Widget _buildDownloadModeSetting(AppLocalizations l10n) {
    return SettingChoiceTile<DownloadMode>(
      icon: Icons.sync_alt,
      title: l10n.settingsDownloadMode,
      description: l10n.settingsDownloadModeHelp,
      value: _currentFormSettings.downloadMode,
      choices: DownloadMode.values
          .map((mode) => SettingChoice(
                value: mode,
                label: _formatDownloadMode(l10n, mode),
              ))
          .toList(),
      onChanged: (value) {
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
    final error = settingsState.remotesError;
    final colors = Theme.of(context).colorScheme;

    return SettingChoiceTile<String>(
      icon: Icons.cloud_queue,
      title: l10n.settingsRcloneRemote,
      value: remotes.contains(currentRemote) ? currentRemote : null,
      placeholder: currentRemote.isEmpty ? null : currentRemote,
      choices: remotes
          .map((remote) => SettingChoice(value: remote, label: remote))
          .toList(),
      onChanged: settingsState.isRemotesLoading || remotes.isEmpty
          ? null
          : (value) => _updateSetting(
                SettingTextField.rcloneRemoteName,
                value,
                updateController: true,
              ),
      trailing: settingsState.isRemotesLoading
          ? const SizedBox(
              width: 36,
              height: 36,
              child: Center(
                child: SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              ),
            )
          : IconButton(
              onPressed: settingsState.refreshRcloneRemotes,
              tooltip: l10n.refresh,
              icon: const Icon(Icons.refresh, size: 20),
            ),
      footer: error != null
          ? _statusText(
              icon: Icons.error_outline,
              color: colors.error,
              text: '${l10n.settingsFailedToLoadRemotes}: $error',
              copyable: true,
            )
          : !settingsState.isRemotesLoading && remotes.isEmpty
              ? _statusText(
                  icon: Icons.info_outline,
                  color: colors.onSurfaceVariant,
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
    return SettingTile(
      icon: Icons.cloud_download_outlined,
      title: l10n.settingsDownloaderSource,
      value: selectedSource?.displayName ?? l10n.downloaderSourceNoSelection,
      description:
          selectedSource != null && selectedSource.description.isNotEmpty
              ? SelectableLinkText(text: selectedSource.description)
              : null,
      content: Align(
        alignment: Alignment.centerLeft,
        child: FilledButton.tonalIcon(
          onPressed: _showDownloaderSourcesDialog,
          icon: const Icon(Icons.link, size: 18),
          label: Text(l10n.installDownloaderConfigFromUrl),
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
    final currentKey = _currentFormSettings.seedColorKey;
    final isCustomColor = app_theme.isHexColor(currentKey);
    final shouldUseCustom = _seedColorCustom || isCustomColor;
    final enabled = !settingsState.settings.useSystemColor;

    // Initialize custom color text field if needed
    if (isCustomColor && !_seedColorCustom) {
      final hex = currentKey.substring(app_theme.kCustomColorKey.length);
      _customColorController.text = hex.toUpperCase();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        setState(() {
          _seedColorCustom = true;
        });
      });
    }

    void applySeedKey(String key) {
      settingsState.setSeedColorKey(key);
      setState(() {
        _currentFormSettings = _currentFormSettings.copyWith(seedColorKey: key);
        _hasChanges = false;
      });
    }

    final swatches = <Widget>[
      for (final key in app_theme.kSeedColorPalette.keys)
        _ColorSwatchButton(
          color: app_theme.seedFromKey(key),
          tooltip: app_theme.seedLabel(l10n, key),
          selected: !shouldUseCustom && currentKey == key,
          enabled: enabled,
          onPressed: () {
            setState(() => _seedColorCustom = false);
            applySeedKey(key);
          },
        ),
      _ColorSwatchButton(
        color: isCustomColor ? app_theme.seedFromKey(currentKey) : null,
        icon: Icons.colorize,
        tooltip: l10n.settingsCustomInput,
        selected: shouldUseCustom,
        enabled: enabled,
        onPressed: () => setState(() => _seedColorCustom = true),
      ),
    ];

    final hexInvalid = _customColorController.text.isNotEmpty &&
        app_theme.parseHexColor(_customColorController.text) == null;

    return SettingTile(
      icon: Icons.color_lens_outlined,
      title: l10n.settingsSeedColor,
      enabled: enabled,
      content: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(spacing: 10, runSpacing: 10, children: swatches),
          if (shouldUseCustom) ...[
            const SizedBox(height: 14),
            SizedBox(
              width: 220,
              child: Semantics(
                label: l10n.settingsCustomInput,
                child: TextField(
                  controller: _customColorController,
                  enabled: enabled,
                  decoration: settingsInputDecoration(
                    context,
                    hintText: 'FF5733',
                    prefixText: '#',
                    errorText: hexInvalid ? l10n.settingsInvalidHexColor : null,
                  ),
                  onChanged: (value) {
                    final normalized =
                        value.replaceAll('#', '').trim().toUpperCase();
                    if (app_theme.parseHexColor(normalized) != null) {
                      applySeedKey('${app_theme.kCustomColorKey}$normalized');
                    } else {
                      setState(() {});
                    }
                  },
                ),
              ),
            ),
            const SizedBox(height: 6),
            Text(
              l10n.settingsCustomColorHint,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
            ),
          ],
        ],
      ),
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

/// A circular color option used by the accent color picker.
class _ColorSwatchButton extends StatelessWidget {
  const _ColorSwatchButton({
    required this.selected,
    required this.enabled,
    required this.onPressed,
    required this.tooltip,
    this.color,
    this.icon,
  });

  final Color? color;
  final IconData? icon;
  final String tooltip;
  final bool selected;
  final bool enabled;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final swatchColor = color ?? colors.surfaceContainerHighest;
    final contentColor =
        ThemeData.estimateBrightnessForColor(swatchColor) == Brightness.dark
            ? Colors.white
            : Colors.black;

    return Tooltip(
      message: tooltip,
      child: Opacity(
        opacity: enabled ? 1 : 0.4,
        child: Material(
          color: swatchColor,
          shape: CircleBorder(
            side: selected
                ? BorderSide(color: colors.onSurface, width: 2)
                : BorderSide(color: colors.outlineVariant, width: 1),
          ),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: enabled ? onPressed : null,
            child: SizedBox(
              width: 36,
              height: 36,
              child: Center(
                child: selected
                    ? Icon(Icons.check, size: 20, color: contentColor)
                    : icon == null
                        ? null
                        : Icon(icon, size: 18, color: contentColor),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
