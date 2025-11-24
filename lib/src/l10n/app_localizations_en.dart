// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'YAAS';

  @override
  String get navHome => 'Home';

  @override
  String get navManage => 'Manage';

  @override
  String get navDownload => 'Download';

  @override
  String get navSideload => 'Sideload';

  @override
  String get navBackups => 'Backups';

  @override
  String get navSettings => 'Settings';

  @override
  String get navLogs => 'Logs';

  @override
  String get navAbout => 'About';

  @override
  String get navDownloads => 'Downloads';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get settingsErrorLoading => 'Error loading settings';

  @override
  String get settingsResetToDefaults => 'Reset to Defaults';

  @override
  String get settingsRevertChangesTooltip =>
      'Revert Changes\n(Shift+Click to reset to defaults)';

  @override
  String get settingsSaveChanges => 'Save Changes';

  @override
  String get settingsSectionGeneral => 'General';

  @override
  String get settingsLanguage => 'Language';

  @override
  String get settingsNavigationRailLabels => 'Navigation rail labels';

  @override
  String get settingsNavigationRailLabelsSelected => 'Selected page only';

  @override
  String get settingsNavigationRailLabelsAll => 'All pages';

  @override
  String get settingsStartupPage => 'Startup page';

  @override
  String get settingsSystemDefault => 'System default';

  @override
  String get languageEnglish => 'English';

  @override
  String get languageRussian => 'Russian';

  @override
  String get settingsSectionStorage => 'Storage';

  @override
  String get settingsDownloadsLocation => 'Downloads Location';

  @override
  String get settingsBackupsLocation => 'Backups Location';

  @override
  String get settingsSectionAdb => 'ADB';

  @override
  String get settingsAdbPath => 'ADB Path';

  @override
  String get settingsPreferredConnection => 'Preferred Connection Type';

  @override
  String get settingsConnectionUsb => 'USB';

  @override
  String get settingsConnectionWireless => 'Wireless';

  @override
  String get settingsMdnsAutoConnect => 'Auto-connect ADB over Wi‑Fi';

  @override
  String get settingsMdnsAutoConnectHelp =>
      'Discover devices via mDNS on the local network and attempt \'adb connect\' automatically. Takes effect after restart.';

  @override
  String get settingsSectionDownloader => 'Downloader';

  @override
  String get preparingDownloader => 'Preparing downloader...';

  @override
  String get downloadingRcloneFiles => 'Downloading rclone files';

  @override
  String get installDownloaderConfig => 'Install local config';

  @override
  String get installDownloaderConfigFromUrl => 'Set up from preset/URL';

  @override
  String get settingsSelectDownloaderConfig => 'Select downloader.json';

  @override
  String downloaderConfigId(String id) {
    return 'Current config ID: $id';
  }

  @override
  String get downloaderConfigFromUrlTitle => 'Set up downloader config';

  @override
  String get downloaderConfigFromUrlDescription =>
      'Choose a preset configuration or use a custom URL. You can always change the configuration later in the settings.';

  @override
  String get downloaderConfigTemplateVrp => 'VRP';

  @override
  String get downloaderConfigTemplateVrpHint => 'Public server';

  @override
  String get downloaderConfigTemplateVrgRus => 'VR Games RUS';

  @override
  String get downloaderConfigTemplateVrgRusHint => 'CIS countries only';

  @override
  String get downloaderConfigTemplateNif => 'NIF';

  @override
  String get downloaderConfigTemplateNifHint => 'Private access';

  @override
  String get downloaderConfigTemplateCustom => 'Custom URL';

  @override
  String get downloaderConfigUrlLabel => 'Config URL';

  @override
  String get downloaderConfigUrlInvalid => 'Please enter a valid http(s) URL';

  @override
  String get downloaderConfigVrgRusTestButton => 'Test access';

  @override
  String get downloaderConfigVrgRusTestOk => 'Access looks OK.';

  @override
  String get downloaderConfigVrgRusTestNoAccess =>
      'No access from this network (request timed out).';

  @override
  String get downloaderConfigVrgRusTestRequiredTooltip =>
      'Run the access test first, or select another server.';

  @override
  String get downloaderConfigNotConfiguredTitle =>
      'Cloud downloader is not configured';

  @override
  String get downloaderConfigNotConfiguredDesc =>
      'Install a downloader.json config to enable cloud app downloads.';

  @override
  String get downloaderConfigInstallButton => 'Install';

  @override
  String get downloaderConfigInstalling => 'Installing...';

  @override
  String get downloaderConfigInstallFailed =>
      'Failed to install downloader config';

  @override
  String get loadingApps => 'Loading apps...';

  @override
  String get settingsSectionAppearance => 'Appearance';

  @override
  String get settingsUseSystemColor => 'Use system color';

  @override
  String get settingsSeedColor => 'Accent color';

  @override
  String get settingsTheme => 'Theme';

  @override
  String get themeAuto => 'Auto';

  @override
  String get themeDark => 'Dark';

  @override
  String get themeLight => 'Light';

  @override
  String get colorDeepPurple => 'Deep purple';

  @override
  String get colorIndigo => 'Indigo';

  @override
  String get colorBlue => 'Blue';

  @override
  String get colorCyan => 'Cyan';

  @override
  String get colorTeal => 'Teal';

  @override
  String get colorGreen => 'Green';

  @override
  String get colorLime => 'Lime';

  @override
  String get colorAmber => 'Amber';

  @override
  String get colorOrange => 'Orange';

  @override
  String get colorDeepOrange => 'Deep orange';

  @override
  String get colorRed => 'Red';

  @override
  String get colorPink => 'Pink';

  @override
  String get colorPurple => 'Purple';

  @override
  String get colorBrown => 'Brown';

  @override
  String get colorBlueGrey => 'Blue grey';

  @override
  String get settingsRclonePath => 'Rclone Path';

  @override
  String get settingsRcloneRemote => 'Rclone Remote';

  @override
  String get settingsCustomRemoteName => 'Custom Remote Name';

  @override
  String get settingsCustomInput => '[Custom]';

  @override
  String get settingsCustomColorHint =>
      'Enter 6-digit hex color (e.g., FF5733)';

  @override
  String get settingsInvalidHexColor => 'Invalid hex color';

  @override
  String get settingsNoRemotesFound => 'No remotes found';

  @override
  String get settingsFailedToLoadRemotes => 'Failed to list remotes';

  @override
  String get settingsBandwidthLimit => 'Bandwidth Limit';

  @override
  String get settingsBandwidthHelper =>
      'Value in KiB/s or with B|K|M|G|T|P suffix or more (click for documentation)';

  @override
  String get settingsDownloadsCleanup => 'Downloads Cleanup';

  @override
  String get settingsWriteLegacyReleaseJson => 'Write legacy release.json';

  @override
  String get settingsWriteLegacyReleaseJsonHelp =>
      'Write release.json in legacy format for compatibility with QLoader';

  @override
  String get settingsCleanupDeleteAfterInstall => 'Remove after installation';

  @override
  String get settingsCleanupKeepOneVersion => 'Keep one version';

  @override
  String get settingsCleanupKeepTwoVersions => 'Keep two versions';

  @override
  String get settingsCleanupKeepAllVersions => 'Keep all versions';

  @override
  String get settingsBrowse => 'Browse';

  @override
  String selectLabel(String label) {
    return 'Select $label';
  }

  @override
  String selectLabelDirectory(String label) {
    return 'Select $label Directory';
  }

  @override
  String couldNotOpenUrl(String url) {
    return 'Could not open $url';
  }

  @override
  String statusAdb(String status) {
    return 'ADB Status: $status';
  }

  @override
  String get statusAdbServerNotRunning => 'ADB server not running';

  @override
  String get statusAdbServerStarting => 'Starting ADB server';

  @override
  String get statusAdbServerStartFailed => 'Failed to start ADB server';

  @override
  String get statusAdbNoDevices => 'No devices found';

  @override
  String statusAdbDevicesAvailable(int count) {
    return 'Devices available ($count)';
  }

  @override
  String get statusAdbConnected => 'Device connected';

  @override
  String get statusAdbDeviceUnauthorized => 'Device unauthorized';

  @override
  String get statusAdbStateOffline => 'Offline';

  @override
  String get statusAdbStateBootloader => 'Bootloader';

  @override
  String get statusAdbStateRecovery => 'Recovery';

  @override
  String get statusAdbStateNoPermissions => 'No permissions';

  @override
  String get statusAdbStateSideload => 'Sideload';

  @override
  String get statusAdbStateAuthorizing => 'Authorizing';

  @override
  String get statusAdbStateUnknown => 'Unknown';

  @override
  String get statusAdbUnknown => 'Unknown';

  @override
  String statusDeviceInfo(String name, String serial) {
    return 'Device: $name\nSerial: $serial';
  }

  @override
  String statusDeviceInfoWireless(String name, String address, String serial) {
    return 'Device: $name\nAddress: $address\nSerial: $serial';
  }

  @override
  String storageTooltip(String available, String total) {
    return '$available free of $total';
  }

  @override
  String activeTasks(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count active tasks',
      one: '$count active task',
    );
    return '$_temp0';
  }

  @override
  String get viewTasks => 'View tasks';

  @override
  String get refreshAllData => 'Refresh all data';

  @override
  String get noDeviceConnected => 'No device connected';

  @override
  String get dragDropDropToInstall => 'Drop to Install / Restore';

  @override
  String get dragDropNoDevice => 'No Device Connected';

  @override
  String get dragDropHintConnected =>
      'Drop APK file/app directory to sideload, or backup folder to restore';

  @override
  String get dragDropHintDisconnected =>
      'Connect a device to enable drag and drop installation';

  @override
  String get dragDropInvalidDir =>
      'Dropped directory is not a valid app directory or backup folder';

  @override
  String get dragDropInvalidFile => 'Dropped file is not a valid APK file';

  @override
  String get dragDropDownloaderConfigTitle => 'Downloader config';

  @override
  String get dragDropDownloaderConfigHint =>
      'Drop downloader.json to install or update the downloader configuration';

  @override
  String get deviceCasting => 'Casting';

  @override
  String get deviceStartCasting => 'Cast';

  @override
  String get castingWirelessUnsupported =>
      'Casting is not supported for wireless devices yet. Connect via USB.';

  @override
  String get castingRequiresDownloadTitle => 'Meta Quest Casting required';

  @override
  String get castingRequiresDownloadPrompt =>
      'This action needs the Meta Quest Casting tool. Download it now?';

  @override
  String get castingToolTitle => 'Meta Quest Casting';

  @override
  String get castingToolDownloadUpdate => 'Download / Update';

  @override
  String get castingToolInstallUpdateTitle => 'Install / Update Casting';

  @override
  String get castingToolInstallUpdateDesc =>
      'This will download the Meta Quest Casting tool and install it into the app data directory.';

  @override
  String get castingToolDownloading => 'Downloading Casting tool...';

  @override
  String get castingToolStatusInstalled => 'Installed';

  @override
  String get castingToolStatusNotInstalled => 'Not installed';

  @override
  String get castingToolRefresh => 'Refresh';

  @override
  String get connectDeviceToInstall => 'Connect a device to install apps';

  @override
  String get connectDeviceToRestore => 'Connect a device to restore backups';

  @override
  String get batteryDumpCopied => 'Battery state dump copied to clipboard';

  @override
  String get batteryDumpFailed => 'Failed to obtain battery state dump';

  @override
  String get commonSuccess => 'Success';

  @override
  String get commonError => 'Error';

  @override
  String get diagnosticsTitle => 'Connection Diagnostics';

  @override
  String get diagnosticsAdbServer => 'ADB server';

  @override
  String get diagnosticsDevices => 'Devices';

  @override
  String get diagnosticsAuthorization => 'Authorization';

  @override
  String get diagnosticsActiveDevice => 'Active device';

  @override
  String get diagnosticsAdbPath => 'ADB path';

  @override
  String get diagnosticsServerNotRunningDesc =>
      'ADB server is not running. Ensure ADB is installed and accessible in PATH or set the ADB path in Settings.';

  @override
  String get diagnosticsServerStartingDesc => 'ADB server is starting...';

  @override
  String get diagnosticsServerStartFailedDesc =>
      'Failed to start the ADB server. Check the ADB path in Settings and view logs for details.';

  @override
  String get diagnosticsServerRunningDesc => 'ADB server is running.';

  @override
  String get diagnosticsNoDevicesDesc =>
      'No devices detected. Enable ADB/developer mode and connect via USB.';

  @override
  String diagnosticsDevicesAvailableDesc(int count) {
    return 'Devices detected ($count)';
  }

  @override
  String get diagnosticsUnauthorizedDesc =>
      'Device is unauthorized. Confirm the authorization prompt on the device.';

  @override
  String get diagnosticsAuthorizedDesc => 'Device authorized.';

  @override
  String get diagnosticsConnectedDesc => 'Device connected and ready.';

  @override
  String get diagnosticsUnknownDesc => 'Unknown state.';

  @override
  String diagnosticsConfiguredPath(String path) {
    return 'Configured path: $path';
  }

  @override
  String get diagnosticsUsingSystemPath => 'Using system PATH';

  @override
  String get commonYes => 'Yes';

  @override
  String get commonNo => 'No';

  @override
  String get deviceActions => 'Device Actions';

  @override
  String get deviceProximitySensor => 'Proximity Sensor';

  @override
  String get disableProximitySensor => 'Disable proximity sensor';

  @override
  String get enableProximitySensor => 'Enable proximity sensor';

  @override
  String get deviceGuardian => 'Guardian';

  @override
  String get guardianSuspend => 'Suspend Guardian';

  @override
  String get guardianResume => 'Resume Guardian';

  @override
  String get deviceWirelessAdb => 'Wireless ADB';

  @override
  String get deviceEnableWirelessAdb => 'Enable ADB over Wi‑Fi';

  @override
  String get copiedToClipboard => 'Copied to clipboard';

  @override
  String get clickToCopy => 'Click to copy';

  @override
  String get detailsPackageName => 'Package Name:';

  @override
  String get detailsVersion => 'Version:';

  @override
  String get detailsVersionCode => 'Version Code:';

  @override
  String get detailsIsVr => 'Is VR:';

  @override
  String get detailsIsLaunchable => 'Is Launchable:';

  @override
  String get detailsIsSystem => 'Is System:';

  @override
  String get detailsStorageUsage => 'Storage Usage:';

  @override
  String get detailsApp => 'App:';

  @override
  String get detailsData => 'Data:';

  @override
  String get detailsCache => 'Cache:';

  @override
  String get detailsTotal => 'Total:';

  @override
  String get detailsRating => 'Rating:';

  @override
  String get detailsReviewsTitle => 'Recent reviews';

  @override
  String get detailsReviewsUnavailable =>
      'Reviews are unavailable for this app.';

  @override
  String get detailsReviewsError => 'Failed to load reviews.';

  @override
  String get detailsReviewsEmpty => 'No reviews available yet.';

  @override
  String get detailsDeveloperResponse => 'Developer response';

  @override
  String get detailsReviewHelpful => 'Helpful';

  @override
  String detailsReviewHelpfulCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count people found this helpful',
      one: '$count person found this helpful',
    );
    return '$_temp0';
  }

  @override
  String get reviewsSortBy => 'Sort by';

  @override
  String get reviewsSortHelpful => 'Most helpful';

  @override
  String get reviewsSortNewest => 'Newest';

  @override
  String get previous => 'Previous';

  @override
  String get next => 'Next';

  @override
  String get reviewsReadAll => 'Read all reviews';

  @override
  String get commonCopy => 'Copy';

  @override
  String get commonClose => 'Close';

  @override
  String get commonCancel => 'Cancel';

  @override
  String get commonDownload => 'Download';

  @override
  String get availableVersions => 'Available Versions';

  @override
  String get installNewerVersion => 'Install newer version';

  @override
  String get reinstallThisVersion => 'Reinstall this version';

  @override
  String get holdShiftToReinstall => 'Hold Shift to reinstall this version';

  @override
  String get cannotDowngrade => 'Cannot downgrade to older version';

  @override
  String get newerVersion => 'Newer version';

  @override
  String get sameVersion => 'Same version';

  @override
  String get olderVersion => 'Older version';

  @override
  String get update => 'Update';

  @override
  String get install => 'Install';

  @override
  String get checkForUpdates => 'Check for updates';

  @override
  String get noMatchingCloudApp => 'No matching app found in cloud repository';

  @override
  String updateTo(String to) {
    return 'Update to $to';
  }

  @override
  String get downgradeAppTitle => 'Downgrade App';

  @override
  String downgradeConfirmMessage(String versionCode) {
    return 'Attempt to downgrade to version $versionCode? This may cause issues.';
  }

  @override
  String get holdShiftToDowngrade => 'Hold Shift to downgrade to this version';

  @override
  String get downgradeToThisVersion => 'Downgrade to this version';

  @override
  String get holdShiftToViewVersions => 'Hold Shift to view versions';

  @override
  String get noAppsInCategory => 'No apps in this category';

  @override
  String get appDetails => 'App Details';

  @override
  String get launch => 'Launch';

  @override
  String get forceStop => 'Force Stop';

  @override
  String get backupApp => 'Backup App';

  @override
  String get backup => 'Backup';

  @override
  String get uninstall => 'Uninstall';

  @override
  String segmentVrApps(int count) {
    return 'VR Apps ($count)';
  }

  @override
  String segmentOtherApps(int count) {
    return 'Other Apps ($count)';
  }

  @override
  String segmentSystemApps(int count) {
    return 'System & Hidden Apps ($count)';
  }

  @override
  String get noAppsFound => 'No apps found';

  @override
  String get noAppsAvailable => 'No apps available';

  @override
  String get copyFullName => 'Copy full name';

  @override
  String get copyPackageName => 'Copy package name';

  @override
  String get addToFavorites => 'Add to favorites';

  @override
  String get removeFromFavorites => 'Remove from favorites';

  @override
  String get clearFavorites => 'Clear favorites';

  @override
  String get clearFavoritesTitle => 'Clear Favorites';

  @override
  String get clearFavoritesConfirm => 'Remove all apps from favorites?';

  @override
  String sizeAndDate(String size, String date) {
    return 'Size: $size • Last Updated: $date';
  }

  @override
  String get downloadToComputer => 'Download to computer';

  @override
  String get downloadAndInstall => 'Download and install on device';

  @override
  String get downloadAndInstallNotConnected =>
      'Download and install on device (not connected)';

  @override
  String get cloudStatusInstalled => 'Installed';

  @override
  String get cloudStatusNewerVersion => 'Newer version';

  @override
  String get cloudStatusOlderVersion => 'Older version';

  @override
  String cloudStatusTooltip(String installedCode, String cloudCode) {
    return 'Installed $installedCode - Cloud $cloudCode';
  }

  @override
  String get sortBy => 'Sort by';

  @override
  String get sortNameAsc => 'Name (A to Z)';

  @override
  String get sortNameDesc => 'Name (Z to A)';

  @override
  String get sortDateOldest => 'Date (Oldest first)';

  @override
  String get sortDateNewest => 'Date (Newest first)';

  @override
  String get sortSizeSmallest => 'Size (Smallest first)';

  @override
  String get sortSizeLargest => 'Size (Largest first)';

  @override
  String get searchAppsHint => 'Search apps...';

  @override
  String get clearSearch => 'Clear search';

  @override
  String get search => 'Search';

  @override
  String get showFavoritesOnly => 'Show favorites only';

  @override
  String get showingFavoritesOnly => 'Showing favorites only';

  @override
  String get showAllItems => 'Show all items';

  @override
  String get showOnlySelectedItems => 'Show only selected items';

  @override
  String get filterNoItems => 'Filter (no items selected)';

  @override
  String selectedSummary(int count, String total) {
    return '$count selected • $total total';
  }

  @override
  String get downloadSelected => 'Download Selected';

  @override
  String get installSelected => 'Install Selected';

  @override
  String get addSelectedToFavorites => 'Favorite Selected';

  @override
  String get removeSelectedFromFavorites => 'Unfavorite Selected';

  @override
  String get clearSelection => 'Clear selection';

  @override
  String get errorLoadingApps => 'Error loading apps';

  @override
  String get retry => 'Retry';

  @override
  String get availableApps => 'Available Apps';

  @override
  String get underConstruction => 'Under construction';

  @override
  String get multiSelect => 'Multi-select';

  @override
  String get refresh => 'Refresh';

  @override
  String get showingSelectedOnly => 'Showing selected items only';

  @override
  String get deviceTitle => 'Device';

  @override
  String get leftController => 'Left Controller';

  @override
  String get rightController => 'Right Controller';

  @override
  String get headset => 'Headset';

  @override
  String get deviceActionsTooltip => 'Device actions';

  @override
  String get statusLabel => 'Status';

  @override
  String get controllerStatusNotConnected => 'Not connected';

  @override
  String get controllerStatusActive => 'Active';

  @override
  String get controllerStatusInactive => 'Inactive';

  @override
  String get controllerStatusDisabled => 'Disabled';

  @override
  String get controllerStatusSearching => 'Searching';

  @override
  String get controllerStatusUnknown => 'Unknown';

  @override
  String get batteryLabel => 'Battery';

  @override
  String get powerOffDevice => 'Power off device';

  @override
  String get powerOffConfirm =>
      'Are you sure you want to power off the device?';

  @override
  String get powerOffMenu => 'Power off...';

  @override
  String get rebootMenu => 'Reboot...';

  @override
  String get rebootOptions => 'Reboot options';

  @override
  String get rebootNormal => 'Normal';

  @override
  String get rebootBootloader => 'Bootloader';

  @override
  String get rebootRecovery => 'Recovery';

  @override
  String get rebootFastboot => 'Fastboot';

  @override
  String get rebootDevice => 'Reboot device';

  @override
  String get rebootNowConfirm => 'Reboot the device now?';

  @override
  String get rebootToBootloader => 'Reboot to bootloader';

  @override
  String get rebootToBootloaderConfirm => 'Reboot the device to bootloader?';

  @override
  String get rebootToRecovery => 'Reboot to recovery';

  @override
  String get rebootToRecoveryConfirm => 'Reboot the device to recovery?';

  @override
  String get rebootToFastboot => 'Reboot to fastboot';

  @override
  String get rebootToFastbootConfirm => 'Reboot the device to fastboot?';

  @override
  String get commonConfirm => 'Confirm';

  @override
  String get delete => 'Delete';

  @override
  String get restore => 'Restore';

  @override
  String get mute => 'Mute';

  @override
  String get unmute => 'Unmute';

  @override
  String get close => 'Close';

  @override
  String get pause => 'Pause';

  @override
  String get checkingTrailerAvailability => 'Checking trailer availability...';

  @override
  String get trailerAvailable => 'Trailer available';

  @override
  String get noTrailer => 'No trailer';

  @override
  String get backupsTitle => 'Backups';

  @override
  String get openBackupsFolder => 'Open Backups Folder';

  @override
  String get openDownloadsFolder => 'Open Downloads Folder';

  @override
  String get downloadsTitle => 'Downloads';

  @override
  String get deleteAllDownloads => 'Delete all downloads';

  @override
  String get deleteAllDownloadsTitle => 'Delete All Downloads';

  @override
  String get deleteAllDownloadsConfirm =>
      'Are you sure you want to delete all downloads?';

  @override
  String deleteAllDownloadsResult(String removed, String skipped) {
    return 'Deleted $removed, skipped $skipped';
  }

  @override
  String get deleteDownloadTitle => 'Delete Download';

  @override
  String deleteDownloadConfirm(String name) {
    return 'Are you sure you want to delete \"$name\"?';
  }

  @override
  String get downloadDeletedTitle => 'Download deleted';

  @override
  String get noBackupsFound => 'No backups found.';

  @override
  String get noDownloadsFound => 'No downloads found.';

  @override
  String get unsupportedPlatform => 'Platform not supported';

  @override
  String get folderPathCopied => 'Folder path copied to clipboard';

  @override
  String unableToOpenFolder(String path) {
    return 'Unable to open folder: $path';
  }

  @override
  String get openFolderTooltip => 'Open Folder';

  @override
  String get unknownTime => 'Unknown time';

  @override
  String get partAPK => 'APK';

  @override
  String get partPrivate => 'Private';

  @override
  String get partShared => 'Shared';

  @override
  String get partOBB => 'OBB';

  @override
  String get noPartsDetected => 'No parts detected';

  @override
  String get deleteBackupTitle => 'Delete Backup';

  @override
  String deleteBackupConfirm(String name) {
    return 'Are you sure you want to delete \"$name\"?';
  }

  @override
  String get backupDeletedTitle => 'Backup deleted';

  @override
  String get fatalErrorTitle => 'Fatal Error';

  @override
  String get exitApplication => 'Exit Application';

  @override
  String get errorCopied => 'Error message copied to clipboard';

  @override
  String get copyError => 'Copy Error';

  @override
  String get selectAppDirectoryTitle => 'Select app directory';

  @override
  String get selectApkFileTitle => 'Select APK file';

  @override
  String get selectedInvalidDir => 'Selected path is not a valid app directory';

  @override
  String get selectedInvalidApk => 'Selected path is not a valid APK file';

  @override
  String get singleApk => 'Single APK';

  @override
  String get appDirectory => 'App Directory';

  @override
  String get appDirectoryPath => 'App Directory Path';

  @override
  String get apkFilePath => 'APK File Path';

  @override
  String get pathHintDirectory => 'Select or enter app directory path';

  @override
  String get pathHintApk => 'Select or enter APK file path';

  @override
  String get directoryRequirements =>
      'The directory should contain an APK file and optionally an OBB data directory, or install.txt file.';

  @override
  String get proTipDragDrop =>
      'Pro tip: You can also drag and drop APK files or app directories anywhere in the app to install them.';

  @override
  String get addedToQueue => 'Added to queue!';

  @override
  String get sideloadApp => 'Sideload App';

  @override
  String get installApk => 'Install APK';

  @override
  String get tasksTitle => 'Tasks';

  @override
  String get tasksTabActive => 'Active';

  @override
  String get tasksTabRecent => 'Recent';

  @override
  String get tasksEmptyActive => 'No active tasks';

  @override
  String get tasksEmptyRecent => 'No recent tasks';

  @override
  String get cancelTask => 'Cancel Task';

  @override
  String get taskKindDownload => 'Download';

  @override
  String get taskKindDownloadInstall => 'Download & Install';

  @override
  String get taskKindInstallApk => 'Install APK';

  @override
  String get taskKindInstallLocalApp => 'Install Local App';

  @override
  String get taskKindUninstall => 'Uninstall';

  @override
  String get taskKindBackupApp => 'Backup App';

  @override
  String get taskKindRestoreBackup => 'Restore Backup';

  @override
  String get taskKindDonateApp => 'Donate App';

  @override
  String get taskStatusWaiting => 'Waiting';

  @override
  String get taskStatusRunning => 'Running';

  @override
  String get taskStatusCompleted => 'Completed';

  @override
  String get taskStatusFailed => 'Failed';

  @override
  String get taskStatusCancelled => 'Cancelled';

  @override
  String get taskUnknown => 'Unknown';

  @override
  String get backupOptionsTitle => 'Backup Options';

  @override
  String get backupSelectParts => 'Select parts to back up:';

  @override
  String get backupAppData => 'App data';

  @override
  String get backupApk => 'APK';

  @override
  String get backupObbFiles => 'OBB files';

  @override
  String get backupNameSuffix => 'Name suffix (optional)';

  @override
  String get backupNameSuffixHint => 'e.g. pre-update';

  @override
  String get startBackup => 'Start Backup';

  @override
  String get logsSearchTooltip =>
      'Search logs by level, message, target, or span id. Examples: \"error\", \"info\", \"adb\", \"connect\", \"13\"';

  @override
  String get logsSearchHint => 'Search logs...';

  @override
  String get clearCurrentLogs => 'Clear current logs';

  @override
  String get exportLogs => 'Export logs';

  @override
  String get openLogsDirectory => 'Open logs directory';

  @override
  String get videoLink => 'Video link';

  @override
  String get clearFilters => 'Clear filters';

  @override
  String get noLogsToDisplay => 'No logs to display';

  @override
  String get logsAppearHere =>
      'Log messages will appear here as they are generated';

  @override
  String get logEntryCopied => 'Log entry copied to clipboard';

  @override
  String get spanId => 'Span ID';

  @override
  String get filterBySpanId => 'Filter logs by this span ID';

  @override
  String get spanTrace => 'Span Trace:';

  @override
  String get spansLabel => 'SPANS';

  @override
  String get logsSpanEventsTooltip =>
      'Show/hide span creation and destruction events. Spans track execution flow.';

  @override
  String logsOpenNotSupported(String path) {
    return 'Platform not supported. Logs directory path copied to clipboard: $path';
  }

  @override
  String logsOpenFailed(String path) {
    return 'Unable to open logs directory (copied to clipboard): $path';
  }

  @override
  String get uninstallAppTitle => 'Uninstall App';

  @override
  String uninstallConfirmMessage(String app) {
    return 'Are you sure you want to uninstall \"$app\"?\n\nThis will permanently delete the app and all its data.';
  }

  @override
  String get uninstalledDone => 'Uninstalled!';

  @override
  String get uninstalling => 'Uninstalling...';

  @override
  String get clearLogsTitle => 'Clear Logs';

  @override
  String get clearLogsMessage =>
      'This will clear all current session logs. This action cannot be undone.';

  @override
  String get commonClear => 'Clear';

  @override
  String logsCopied(int count) {
    return '$count logs copied to clipboard';
  }

  @override
  String get downgradeAppsTitle => 'Downgrade Apps';

  @override
  String get downgradeMultipleConfirmMessage =>
      'The following apps will be downgraded. This may cause issues.';

  @override
  String downgradeItemFormat(
      String name, String installedCode, String cloudCode) {
    return '$name ($installedCode → $cloudCode)';
  }

  @override
  String get downloadedStatusNewerVersion => 'Update available';

  @override
  String get downloadedStatusToolTip => 'Click to go to the list';

  @override
  String get emptyValue => '(empty)';

  @override
  String get navDonate => 'Donate Apps';

  @override
  String get donateAppsDescription =>
      'Select apps to donate (upload) to the community';

  @override
  String get donateShowFiltered => 'Show filtered apps';

  @override
  String get donateHideFiltered => 'Hide filtered apps';

  @override
  String get donateFilterReasonBlacklisted => 'Blacklisted';

  @override
  String get donateFilterReasonRenamed => 'Renamed package';

  @override
  String get donateFilterReasonSystemUnwanted => 'System/unwanted app';

  @override
  String get donateFilterReasonAlreadyExists => 'Already exists';

  @override
  String get donateStatusNewApp => 'New app';

  @override
  String get donateStatusNewerVersion => 'Newer version';

  @override
  String get donateDonateButton => 'Donate';

  @override
  String get donateNoAppsAvailable => 'No apps available for donation';

  @override
  String get donateNoAppsWithFilters => 'No apps match the current filters';

  @override
  String get donateLoadingCloudApps => 'Loading cloud apps list...';

  @override
  String get copyDisplayName => 'Copy display name';

  @override
  String get donateDownloaderNotAvailable => 'Downloader not available';
}
