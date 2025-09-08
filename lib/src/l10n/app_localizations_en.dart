// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'Zyde';

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
  String get settingsSectionDownloader => 'Downloader';

  @override
  String get settingsRclonePath => 'Rclone Path';

  @override
  String get settingsRcloneRemote => 'Rclone Remote';

  @override
  String get settingsCustomRemoteName => 'Custom Remote Name';

  @override
  String get settingsCustomInput => '[Custom]';

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
  String get settingsCleanupDeleteAfterInstall => 'Remove after installation';

  @override
  String get settingsCleanupKeepOneVersion => 'Keep latest version only';

  @override
  String get settingsCleanupKeepTwoVersions => 'Keep last two versions';

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
  String get statusAdbNoDevices => 'No devices found';

  @override
  String statusAdbDevicesAvailable(int count) {
    return 'Devices available ($count)';
  }

  @override
  String get statusAdbConnected => 'Connected';

  @override
  String get statusAdbDeviceUnauthorized => 'Device unauthorized';

  @override
  String get statusAdbUnknown => 'Unknown';

  @override
  String statusDeviceInfo(String name, String serial) {
    return 'Device: $name\nSerial: $serial';
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
      other: '# active tasks',
      one: '# active task',
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
  String get connectDeviceToInstall => 'Connect a device to install apps';

  @override
  String get batteryDumpCopied => 'Battery state dump copied to clipboard';

  @override
  String get batteryDumpFailed => 'Failed to obtain battery state dump';

  @override
  String get commonSuccess => 'Success';

  @override
  String get commonError => 'Error';

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
  String get commonCopy => 'Copy';

  @override
  String get commonClose => 'Close';

  @override
  String get commonCancel => 'Cancel';

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
  String updateFromTo(String from, String to) {
    return 'Update from $from to $to';
  }

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
  String get clearSelection => 'Clear selection';

  @override
  String get errorLoadingApps => 'Error loading apps';

  @override
  String get retry => 'Retry';

  @override
  String get availableApps => 'Available Apps';

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
  String get backupsTitle => 'Backups';

  @override
  String get openBackupsFolder => 'Open Backups Folder';

  @override
  String get noBackupsFound => 'No backups found.';

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
  String get taskTypeDownload => 'Download';

  @override
  String get taskTypeDownloadInstall => 'Download & Install';

  @override
  String get taskTypeInstallApk => 'Install APK';

  @override
  String get taskTypeInstallLocalApp => 'Install Local App';

  @override
  String get taskTypeUninstall => 'Uninstall';

  @override
  String get taskTypeBackupApp => 'Backup App';

  @override
  String get taskTypeRestoreBackup => 'Restore Backup';

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
  String get createdWord => 'created';

  @override
  String get closedWord => 'closed';

  @override
  String get noMessage => 'no message';

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
  String get emptyValue => '(empty)';

  @override
  String get errorWord => 'error';
}
