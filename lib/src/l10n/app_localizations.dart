import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_ru.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
      : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
    delegate,
    GlobalMaterialLocalizations.delegate,
    GlobalCupertinoLocalizations.delegate,
    GlobalWidgetsLocalizations.delegate,
  ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('ru')
  ];

  /// No description provided for @appTitle.
  ///
  /// In en, this message translates to:
  /// **'YAAS'**
  String get appTitle;

  /// No description provided for @navHome.
  ///
  /// In en, this message translates to:
  /// **'Home'**
  String get navHome;

  /// No description provided for @navManage.
  ///
  /// In en, this message translates to:
  /// **'Manage'**
  String get navManage;

  /// No description provided for @navDownload.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get navDownload;

  /// No description provided for @navSideload.
  ///
  /// In en, this message translates to:
  /// **'Sideload'**
  String get navSideload;

  /// No description provided for @navBackups.
  ///
  /// In en, this message translates to:
  /// **'Backups'**
  String get navBackups;

  /// No description provided for @navSettings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get navSettings;

  /// No description provided for @navLogs.
  ///
  /// In en, this message translates to:
  /// **'Logs'**
  String get navLogs;

  /// No description provided for @navAbout.
  ///
  /// In en, this message translates to:
  /// **'About'**
  String get navAbout;

  /// No description provided for @settingsTitle.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settingsTitle;

  /// No description provided for @settingsErrorLoading.
  ///
  /// In en, this message translates to:
  /// **'Error loading settings'**
  String get settingsErrorLoading;

  /// No description provided for @settingsResetToDefaults.
  ///
  /// In en, this message translates to:
  /// **'Reset to Defaults'**
  String get settingsResetToDefaults;

  /// No description provided for @settingsRevertChangesTooltip.
  ///
  /// In en, this message translates to:
  /// **'Revert Changes\n(Shift+Click to reset to defaults)'**
  String get settingsRevertChangesTooltip;

  /// No description provided for @settingsSaveChanges.
  ///
  /// In en, this message translates to:
  /// **'Save Changes'**
  String get settingsSaveChanges;

  /// No description provided for @settingsSectionGeneral.
  ///
  /// In en, this message translates to:
  /// **'General'**
  String get settingsSectionGeneral;

  /// No description provided for @settingsLanguage.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get settingsLanguage;

  /// No description provided for @settingsSystemDefault.
  ///
  /// In en, this message translates to:
  /// **'System default'**
  String get settingsSystemDefault;

  /// No description provided for @languageEnglish.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get languageEnglish;

  /// No description provided for @languageRussian.
  ///
  /// In en, this message translates to:
  /// **'Russian'**
  String get languageRussian;

  /// No description provided for @settingsSectionStorage.
  ///
  /// In en, this message translates to:
  /// **'Storage'**
  String get settingsSectionStorage;

  /// No description provided for @settingsDownloadsLocation.
  ///
  /// In en, this message translates to:
  /// **'Downloads Location'**
  String get settingsDownloadsLocation;

  /// No description provided for @settingsBackupsLocation.
  ///
  /// In en, this message translates to:
  /// **'Backups Location'**
  String get settingsBackupsLocation;

  /// No description provided for @settingsSectionAdb.
  ///
  /// In en, this message translates to:
  /// **'ADB'**
  String get settingsSectionAdb;

  /// No description provided for @settingsAdbPath.
  ///
  /// In en, this message translates to:
  /// **'ADB Path'**
  String get settingsAdbPath;

  /// No description provided for @settingsPreferredConnection.
  ///
  /// In en, this message translates to:
  /// **'Preferred Connection Type'**
  String get settingsPreferredConnection;

  /// No description provided for @settingsConnectionUsb.
  ///
  /// In en, this message translates to:
  /// **'USB'**
  String get settingsConnectionUsb;

  /// No description provided for @settingsConnectionWireless.
  ///
  /// In en, this message translates to:
  /// **'Wireless'**
  String get settingsConnectionWireless;

  /// No description provided for @settingsSectionDownloader.
  ///
  /// In en, this message translates to:
  /// **'Downloader'**
  String get settingsSectionDownloader;

  /// No description provided for @settingsRclonePath.
  ///
  /// In en, this message translates to:
  /// **'Rclone Path'**
  String get settingsRclonePath;

  /// No description provided for @settingsRcloneRemote.
  ///
  /// In en, this message translates to:
  /// **'Rclone Remote'**
  String get settingsRcloneRemote;

  /// No description provided for @settingsCustomRemoteName.
  ///
  /// In en, this message translates to:
  /// **'Custom Remote Name'**
  String get settingsCustomRemoteName;

  /// No description provided for @settingsCustomInput.
  ///
  /// In en, this message translates to:
  /// **'[Custom]'**
  String get settingsCustomInput;

  /// No description provided for @settingsNoRemotesFound.
  ///
  /// In en, this message translates to:
  /// **'No remotes found'**
  String get settingsNoRemotesFound;

  /// No description provided for @settingsFailedToLoadRemotes.
  ///
  /// In en, this message translates to:
  /// **'Failed to list remotes'**
  String get settingsFailedToLoadRemotes;

  /// No description provided for @settingsBandwidthLimit.
  ///
  /// In en, this message translates to:
  /// **'Bandwidth Limit'**
  String get settingsBandwidthLimit;

  /// No description provided for @settingsBandwidthHelper.
  ///
  /// In en, this message translates to:
  /// **'(Not implemented) Value in KiB/s or with B|K|M|G|T|P suffix or more (click for documentation)'**
  String get settingsBandwidthHelper;

  /// No description provided for @settingsDownloadsCleanup.
  ///
  /// In en, this message translates to:
  /// **'Downloads Cleanup'**
  String get settingsDownloadsCleanup;

  /// No description provided for @settingsCleanupDeleteAfterInstall.
  ///
  /// In en, this message translates to:
  /// **'Remove after installation'**
  String get settingsCleanupDeleteAfterInstall;

  /// No description provided for @settingsCleanupKeepOneVersion.
  ///
  /// In en, this message translates to:
  /// **'Keep latest version only'**
  String get settingsCleanupKeepOneVersion;

  /// No description provided for @settingsCleanupKeepTwoVersions.
  ///
  /// In en, this message translates to:
  /// **'Keep last two versions'**
  String get settingsCleanupKeepTwoVersions;

  /// No description provided for @settingsCleanupKeepAllVersions.
  ///
  /// In en, this message translates to:
  /// **'Keep all versions'**
  String get settingsCleanupKeepAllVersions;

  /// No description provided for @settingsBrowse.
  ///
  /// In en, this message translates to:
  /// **'Browse'**
  String get settingsBrowse;

  /// No description provided for @selectLabel.
  ///
  /// In en, this message translates to:
  /// **'Select {label}'**
  String selectLabel(String label);

  /// No description provided for @selectLabelDirectory.
  ///
  /// In en, this message translates to:
  /// **'Select {label} Directory'**
  String selectLabelDirectory(String label);

  /// No description provided for @couldNotOpenUrl.
  ///
  /// In en, this message translates to:
  /// **'Could not open {url}'**
  String couldNotOpenUrl(String url);

  /// No description provided for @statusAdb.
  ///
  /// In en, this message translates to:
  /// **'ADB Status: {status}'**
  String statusAdb(String status);

  /// No description provided for @statusAdbServerNotRunning.
  ///
  /// In en, this message translates to:
  /// **'ADB server not running'**
  String get statusAdbServerNotRunning;

  /// No description provided for @statusAdbServerStarting.
  ///
  /// In en, this message translates to:
  /// **'Starting ADB server'**
  String get statusAdbServerStarting;

  /// No description provided for @statusAdbNoDevices.
  ///
  /// In en, this message translates to:
  /// **'No devices found'**
  String get statusAdbNoDevices;

  /// No description provided for @statusAdbDevicesAvailable.
  ///
  /// In en, this message translates to:
  /// **'Devices available ({count})'**
  String statusAdbDevicesAvailable(int count);

  /// No description provided for @statusAdbConnected.
  ///
  /// In en, this message translates to:
  /// **'Connected'**
  String get statusAdbConnected;

  /// No description provided for @statusAdbDeviceUnauthorized.
  ///
  /// In en, this message translates to:
  /// **'Device unauthorized'**
  String get statusAdbDeviceUnauthorized;

  /// No description provided for @statusAdbUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get statusAdbUnknown;

  /// No description provided for @statusDeviceInfo.
  ///
  /// In en, this message translates to:
  /// **'Device: {name}\nSerial: {serial}'**
  String statusDeviceInfo(String name, String serial);

  /// No description provided for @storageTooltip.
  ///
  /// In en, this message translates to:
  /// **'{available} free of {total}'**
  String storageTooltip(String available, String total);

  /// No description provided for @activeTasks.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one {{count} active task} other {{count} active tasks}}'**
  String activeTasks(int count);

  /// No description provided for @viewTasks.
  ///
  /// In en, this message translates to:
  /// **'View tasks'**
  String get viewTasks;

  /// No description provided for @refreshAllData.
  ///
  /// In en, this message translates to:
  /// **'Refresh all data'**
  String get refreshAllData;

  /// No description provided for @noDeviceConnected.
  ///
  /// In en, this message translates to:
  /// **'No device connected'**
  String get noDeviceConnected;

  /// No description provided for @dragDropDropToInstall.
  ///
  /// In en, this message translates to:
  /// **'Drop to Install / Restore'**
  String get dragDropDropToInstall;

  /// No description provided for @dragDropNoDevice.
  ///
  /// In en, this message translates to:
  /// **'No Device Connected'**
  String get dragDropNoDevice;

  /// No description provided for @dragDropHintConnected.
  ///
  /// In en, this message translates to:
  /// **'Drop APK file/app directory to sideload, or backup folder to restore'**
  String get dragDropHintConnected;

  /// No description provided for @dragDropHintDisconnected.
  ///
  /// In en, this message translates to:
  /// **'Connect a device to enable drag and drop installation'**
  String get dragDropHintDisconnected;

  /// No description provided for @dragDropInvalidDir.
  ///
  /// In en, this message translates to:
  /// **'Dropped directory is not a valid app directory or backup folder'**
  String get dragDropInvalidDir;

  /// No description provided for @dragDropInvalidFile.
  ///
  /// In en, this message translates to:
  /// **'Dropped file is not a valid APK file'**
  String get dragDropInvalidFile;

  /// No description provided for @connectDeviceToInstall.
  ///
  /// In en, this message translates to:
  /// **'Connect a device to install apps'**
  String get connectDeviceToInstall;

  /// No description provided for @batteryDumpCopied.
  ///
  /// In en, this message translates to:
  /// **'Battery state dump copied to clipboard'**
  String get batteryDumpCopied;

  /// No description provided for @batteryDumpFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed to obtain battery state dump'**
  String get batteryDumpFailed;

  /// No description provided for @commonSuccess.
  ///
  /// In en, this message translates to:
  /// **'Success'**
  String get commonSuccess;

  /// No description provided for @commonError.
  ///
  /// In en, this message translates to:
  /// **'Error'**
  String get commonError;

  /// No description provided for @commonYes.
  ///
  /// In en, this message translates to:
  /// **'Yes'**
  String get commonYes;

  /// No description provided for @commonNo.
  ///
  /// In en, this message translates to:
  /// **'No'**
  String get commonNo;

  /// No description provided for @deviceActions.
  ///
  /// In en, this message translates to:
  /// **'Device Actions'**
  String get deviceActions;

  /// No description provided for @deviceProximitySensor.
  ///
  /// In en, this message translates to:
  /// **'Proximity Sensor'**
  String get deviceProximitySensor;

  /// No description provided for @disableProximitySensor.
  ///
  /// In en, this message translates to:
  /// **'Disable proximity sensor'**
  String get disableProximitySensor;

  /// No description provided for @enableProximitySensor.
  ///
  /// In en, this message translates to:
  /// **'Enable proximity sensor'**
  String get enableProximitySensor;

  /// No description provided for @deviceGuardian.
  ///
  /// In en, this message translates to:
  /// **'Guardian'**
  String get deviceGuardian;

  /// No description provided for @guardianSuspend.
  ///
  /// In en, this message translates to:
  /// **'Suspend Guardian'**
  String get guardianSuspend;

  /// No description provided for @guardianResume.
  ///
  /// In en, this message translates to:
  /// **'Resume Guardian'**
  String get guardianResume;

  /// No description provided for @copiedToClipboard.
  ///
  /// In en, this message translates to:
  /// **'Copied to clipboard'**
  String get copiedToClipboard;

  /// No description provided for @clickToCopy.
  ///
  /// In en, this message translates to:
  /// **'Click to copy'**
  String get clickToCopy;

  /// No description provided for @detailsPackageName.
  ///
  /// In en, this message translates to:
  /// **'Package Name:'**
  String get detailsPackageName;

  /// No description provided for @detailsVersion.
  ///
  /// In en, this message translates to:
  /// **'Version:'**
  String get detailsVersion;

  /// No description provided for @detailsVersionCode.
  ///
  /// In en, this message translates to:
  /// **'Version Code:'**
  String get detailsVersionCode;

  /// No description provided for @detailsIsVr.
  ///
  /// In en, this message translates to:
  /// **'Is VR:'**
  String get detailsIsVr;

  /// No description provided for @detailsIsLaunchable.
  ///
  /// In en, this message translates to:
  /// **'Is Launchable:'**
  String get detailsIsLaunchable;

  /// No description provided for @detailsIsSystem.
  ///
  /// In en, this message translates to:
  /// **'Is System:'**
  String get detailsIsSystem;

  /// No description provided for @detailsStorageUsage.
  ///
  /// In en, this message translates to:
  /// **'Storage Usage:'**
  String get detailsStorageUsage;

  /// No description provided for @detailsApp.
  ///
  /// In en, this message translates to:
  /// **'App:'**
  String get detailsApp;

  /// No description provided for @detailsData.
  ///
  /// In en, this message translates to:
  /// **'Data:'**
  String get detailsData;

  /// No description provided for @detailsCache.
  ///
  /// In en, this message translates to:
  /// **'Cache:'**
  String get detailsCache;

  /// No description provided for @detailsTotal.
  ///
  /// In en, this message translates to:
  /// **'Total:'**
  String get detailsTotal;

  /// No description provided for @detailsRating.
  ///
  /// In en, this message translates to:
  /// **'Rating:'**
  String get detailsRating;

  /// No description provided for @commonCopy.
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get commonCopy;

  /// No description provided for @commonClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get commonClose;

  /// No description provided for @commonCancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get commonCancel;

  /// No description provided for @availableVersions.
  ///
  /// In en, this message translates to:
  /// **'Available Versions'**
  String get availableVersions;

  /// No description provided for @installNewerVersion.
  ///
  /// In en, this message translates to:
  /// **'Install newer version'**
  String get installNewerVersion;

  /// No description provided for @reinstallThisVersion.
  ///
  /// In en, this message translates to:
  /// **'Reinstall this version'**
  String get reinstallThisVersion;

  /// No description provided for @holdShiftToReinstall.
  ///
  /// In en, this message translates to:
  /// **'Hold Shift to reinstall this version'**
  String get holdShiftToReinstall;

  /// No description provided for @cannotDowngrade.
  ///
  /// In en, this message translates to:
  /// **'Cannot downgrade to older version'**
  String get cannotDowngrade;

  /// No description provided for @newerVersion.
  ///
  /// In en, this message translates to:
  /// **'Newer version'**
  String get newerVersion;

  /// No description provided for @sameVersion.
  ///
  /// In en, this message translates to:
  /// **'Same version'**
  String get sameVersion;

  /// No description provided for @olderVersion.
  ///
  /// In en, this message translates to:
  /// **'Older version'**
  String get olderVersion;

  /// No description provided for @update.
  ///
  /// In en, this message translates to:
  /// **'Update'**
  String get update;

  /// No description provided for @install.
  ///
  /// In en, this message translates to:
  /// **'Install'**
  String get install;

  /// No description provided for @checkForUpdates.
  ///
  /// In en, this message translates to:
  /// **'Check for updates'**
  String get checkForUpdates;

  /// No description provided for @noMatchingCloudApp.
  ///
  /// In en, this message translates to:
  /// **'No matching app found in cloud repository'**
  String get noMatchingCloudApp;

  /// No description provided for @updateFromTo.
  ///
  /// In en, this message translates to:
  /// **'Update from {from} to {to}'**
  String updateFromTo(String from, String to);

  /// No description provided for @noAppsInCategory.
  ///
  /// In en, this message translates to:
  /// **'No apps in this category'**
  String get noAppsInCategory;

  /// No description provided for @appDetails.
  ///
  /// In en, this message translates to:
  /// **'App Details'**
  String get appDetails;

  /// No description provided for @launch.
  ///
  /// In en, this message translates to:
  /// **'Launch'**
  String get launch;

  /// No description provided for @forceStop.
  ///
  /// In en, this message translates to:
  /// **'Force Stop'**
  String get forceStop;

  /// No description provided for @backupApp.
  ///
  /// In en, this message translates to:
  /// **'Backup App'**
  String get backupApp;

  /// No description provided for @backup.
  ///
  /// In en, this message translates to:
  /// **'Backup'**
  String get backup;

  /// No description provided for @uninstall.
  ///
  /// In en, this message translates to:
  /// **'Uninstall'**
  String get uninstall;

  /// No description provided for @segmentVrApps.
  ///
  /// In en, this message translates to:
  /// **'VR Apps ({count})'**
  String segmentVrApps(int count);

  /// No description provided for @segmentOtherApps.
  ///
  /// In en, this message translates to:
  /// **'Other Apps ({count})'**
  String segmentOtherApps(int count);

  /// No description provided for @segmentSystemApps.
  ///
  /// In en, this message translates to:
  /// **'System & Hidden Apps ({count})'**
  String segmentSystemApps(int count);

  /// No description provided for @noAppsFound.
  ///
  /// In en, this message translates to:
  /// **'No apps found'**
  String get noAppsFound;

  /// No description provided for @noAppsAvailable.
  ///
  /// In en, this message translates to:
  /// **'No apps available'**
  String get noAppsAvailable;

  /// No description provided for @copyFullName.
  ///
  /// In en, this message translates to:
  /// **'Copy full name'**
  String get copyFullName;

  /// No description provided for @copyPackageName.
  ///
  /// In en, this message translates to:
  /// **'Copy package name'**
  String get copyPackageName;

  /// No description provided for @sizeAndDate.
  ///
  /// In en, this message translates to:
  /// **'Size: {size} • Last Updated: {date}'**
  String sizeAndDate(String size, String date);

  /// No description provided for @downloadToComputer.
  ///
  /// In en, this message translates to:
  /// **'Download to computer'**
  String get downloadToComputer;

  /// No description provided for @downloadAndInstall.
  ///
  /// In en, this message translates to:
  /// **'Download and install on device'**
  String get downloadAndInstall;

  /// No description provided for @downloadAndInstallNotConnected.
  ///
  /// In en, this message translates to:
  /// **'Download and install on device (not connected)'**
  String get downloadAndInstallNotConnected;

  /// No description provided for @sortBy.
  ///
  /// In en, this message translates to:
  /// **'Sort by'**
  String get sortBy;

  /// No description provided for @sortNameAsc.
  ///
  /// In en, this message translates to:
  /// **'Name (A to Z)'**
  String get sortNameAsc;

  /// No description provided for @sortNameDesc.
  ///
  /// In en, this message translates to:
  /// **'Name (Z to A)'**
  String get sortNameDesc;

  /// No description provided for @sortDateOldest.
  ///
  /// In en, this message translates to:
  /// **'Date (Oldest first)'**
  String get sortDateOldest;

  /// No description provided for @sortDateNewest.
  ///
  /// In en, this message translates to:
  /// **'Date (Newest first)'**
  String get sortDateNewest;

  /// No description provided for @sortSizeSmallest.
  ///
  /// In en, this message translates to:
  /// **'Size (Smallest first)'**
  String get sortSizeSmallest;

  /// No description provided for @sortSizeLargest.
  ///
  /// In en, this message translates to:
  /// **'Size (Largest first)'**
  String get sortSizeLargest;

  /// No description provided for @searchAppsHint.
  ///
  /// In en, this message translates to:
  /// **'Search apps...'**
  String get searchAppsHint;

  /// No description provided for @clearSearch.
  ///
  /// In en, this message translates to:
  /// **'Clear search'**
  String get clearSearch;

  /// No description provided for @search.
  ///
  /// In en, this message translates to:
  /// **'Search'**
  String get search;

  /// No description provided for @showAllItems.
  ///
  /// In en, this message translates to:
  /// **'Show all items'**
  String get showAllItems;

  /// No description provided for @showOnlySelectedItems.
  ///
  /// In en, this message translates to:
  /// **'Show only selected items'**
  String get showOnlySelectedItems;

  /// No description provided for @filterNoItems.
  ///
  /// In en, this message translates to:
  /// **'Filter (no items selected)'**
  String get filterNoItems;

  /// No description provided for @selectedSummary.
  ///
  /// In en, this message translates to:
  /// **'{count} selected • {total} total'**
  String selectedSummary(int count, String total);

  /// No description provided for @downloadSelected.
  ///
  /// In en, this message translates to:
  /// **'Download Selected'**
  String get downloadSelected;

  /// No description provided for @installSelected.
  ///
  /// In en, this message translates to:
  /// **'Install Selected'**
  String get installSelected;

  /// No description provided for @clearSelection.
  ///
  /// In en, this message translates to:
  /// **'Clear selection'**
  String get clearSelection;

  /// No description provided for @errorLoadingApps.
  ///
  /// In en, this message translates to:
  /// **'Error loading apps'**
  String get errorLoadingApps;

  /// No description provided for @retry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get retry;

  /// No description provided for @availableApps.
  ///
  /// In en, this message translates to:
  /// **'Available Apps'**
  String get availableApps;

  /// No description provided for @underConstruction.
  ///
  /// In en, this message translates to:
  /// **'Under construction'**
  String get underConstruction;

  /// No description provided for @multiSelect.
  ///
  /// In en, this message translates to:
  /// **'Multi-select'**
  String get multiSelect;

  /// No description provided for @refresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get refresh;

  /// No description provided for @showingSelectedOnly.
  ///
  /// In en, this message translates to:
  /// **'Showing selected items only'**
  String get showingSelectedOnly;

  /// No description provided for @deviceTitle.
  ///
  /// In en, this message translates to:
  /// **'Device'**
  String get deviceTitle;

  /// No description provided for @leftController.
  ///
  /// In en, this message translates to:
  /// **'Left Controller'**
  String get leftController;

  /// No description provided for @rightController.
  ///
  /// In en, this message translates to:
  /// **'Right Controller'**
  String get rightController;

  /// No description provided for @headset.
  ///
  /// In en, this message translates to:
  /// **'Headset'**
  String get headset;

  /// No description provided for @deviceActionsTooltip.
  ///
  /// In en, this message translates to:
  /// **'Device actions'**
  String get deviceActionsTooltip;

  /// No description provided for @statusLabel.
  ///
  /// In en, this message translates to:
  /// **'Status'**
  String get statusLabel;

  /// No description provided for @batteryLabel.
  ///
  /// In en, this message translates to:
  /// **'Battery'**
  String get batteryLabel;

  /// No description provided for @powerOffDevice.
  ///
  /// In en, this message translates to:
  /// **'Power off device'**
  String get powerOffDevice;

  /// No description provided for @powerOffConfirm.
  ///
  /// In en, this message translates to:
  /// **'Are you sure you want to power off the device?'**
  String get powerOffConfirm;

  /// No description provided for @powerOffMenu.
  ///
  /// In en, this message translates to:
  /// **'Power off...'**
  String get powerOffMenu;

  /// No description provided for @rebootMenu.
  ///
  /// In en, this message translates to:
  /// **'Reboot...'**
  String get rebootMenu;

  /// No description provided for @rebootOptions.
  ///
  /// In en, this message translates to:
  /// **'Reboot options'**
  String get rebootOptions;

  /// No description provided for @rebootNormal.
  ///
  /// In en, this message translates to:
  /// **'Normal'**
  String get rebootNormal;

  /// No description provided for @rebootBootloader.
  ///
  /// In en, this message translates to:
  /// **'Bootloader'**
  String get rebootBootloader;

  /// No description provided for @rebootRecovery.
  ///
  /// In en, this message translates to:
  /// **'Recovery'**
  String get rebootRecovery;

  /// No description provided for @rebootFastboot.
  ///
  /// In en, this message translates to:
  /// **'Fastboot'**
  String get rebootFastboot;

  /// No description provided for @rebootDevice.
  ///
  /// In en, this message translates to:
  /// **'Reboot device'**
  String get rebootDevice;

  /// No description provided for @rebootNowConfirm.
  ///
  /// In en, this message translates to:
  /// **'Reboot the device now?'**
  String get rebootNowConfirm;

  /// No description provided for @rebootToBootloader.
  ///
  /// In en, this message translates to:
  /// **'Reboot to bootloader'**
  String get rebootToBootloader;

  /// No description provided for @rebootToBootloaderConfirm.
  ///
  /// In en, this message translates to:
  /// **'Reboot the device to bootloader?'**
  String get rebootToBootloaderConfirm;

  /// No description provided for @rebootToRecovery.
  ///
  /// In en, this message translates to:
  /// **'Reboot to recovery'**
  String get rebootToRecovery;

  /// No description provided for @rebootToRecoveryConfirm.
  ///
  /// In en, this message translates to:
  /// **'Reboot the device to recovery?'**
  String get rebootToRecoveryConfirm;

  /// No description provided for @rebootToFastboot.
  ///
  /// In en, this message translates to:
  /// **'Reboot to fastboot'**
  String get rebootToFastboot;

  /// No description provided for @rebootToFastbootConfirm.
  ///
  /// In en, this message translates to:
  /// **'Reboot the device to fastboot?'**
  String get rebootToFastbootConfirm;

  /// No description provided for @commonConfirm.
  ///
  /// In en, this message translates to:
  /// **'Confirm'**
  String get commonConfirm;

  /// No description provided for @delete.
  ///
  /// In en, this message translates to:
  /// **'Delete'**
  String get delete;

  /// No description provided for @restore.
  ///
  /// In en, this message translates to:
  /// **'Restore'**
  String get restore;

  /// No description provided for @backupsTitle.
  ///
  /// In en, this message translates to:
  /// **'Backups'**
  String get backupsTitle;

  /// No description provided for @openBackupsFolder.
  ///
  /// In en, this message translates to:
  /// **'Open Backups Folder'**
  String get openBackupsFolder;

  /// No description provided for @noBackupsFound.
  ///
  /// In en, this message translates to:
  /// **'No backups found.'**
  String get noBackupsFound;

  /// No description provided for @unsupportedPlatform.
  ///
  /// In en, this message translates to:
  /// **'Platform not supported'**
  String get unsupportedPlatform;

  /// No description provided for @folderPathCopied.
  ///
  /// In en, this message translates to:
  /// **'Folder path copied to clipboard'**
  String get folderPathCopied;

  /// No description provided for @unableToOpenFolder.
  ///
  /// In en, this message translates to:
  /// **'Unable to open folder: {path}'**
  String unableToOpenFolder(String path);

  /// No description provided for @openFolderTooltip.
  ///
  /// In en, this message translates to:
  /// **'Open Folder'**
  String get openFolderTooltip;

  /// No description provided for @unknownTime.
  ///
  /// In en, this message translates to:
  /// **'Unknown time'**
  String get unknownTime;

  /// No description provided for @partAPK.
  ///
  /// In en, this message translates to:
  /// **'APK'**
  String get partAPK;

  /// No description provided for @partPrivate.
  ///
  /// In en, this message translates to:
  /// **'Private'**
  String get partPrivate;

  /// No description provided for @partShared.
  ///
  /// In en, this message translates to:
  /// **'Shared'**
  String get partShared;

  /// No description provided for @partOBB.
  ///
  /// In en, this message translates to:
  /// **'OBB'**
  String get partOBB;

  /// No description provided for @noPartsDetected.
  ///
  /// In en, this message translates to:
  /// **'No parts detected'**
  String get noPartsDetected;

  /// No description provided for @deleteBackupTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete Backup'**
  String get deleteBackupTitle;

  /// No description provided for @deleteBackupConfirm.
  ///
  /// In en, this message translates to:
  /// **'Are you sure you want to delete \"{name}\"?'**
  String deleteBackupConfirm(String name);

  /// No description provided for @backupDeletedTitle.
  ///
  /// In en, this message translates to:
  /// **'Backup deleted'**
  String get backupDeletedTitle;

  /// No description provided for @fatalErrorTitle.
  ///
  /// In en, this message translates to:
  /// **'Fatal Error'**
  String get fatalErrorTitle;

  /// No description provided for @exitApplication.
  ///
  /// In en, this message translates to:
  /// **'Exit Application'**
  String get exitApplication;

  /// No description provided for @errorCopied.
  ///
  /// In en, this message translates to:
  /// **'Error message copied to clipboard'**
  String get errorCopied;

  /// No description provided for @copyError.
  ///
  /// In en, this message translates to:
  /// **'Copy Error'**
  String get copyError;

  /// No description provided for @selectAppDirectoryTitle.
  ///
  /// In en, this message translates to:
  /// **'Select app directory'**
  String get selectAppDirectoryTitle;

  /// No description provided for @selectApkFileTitle.
  ///
  /// In en, this message translates to:
  /// **'Select APK file'**
  String get selectApkFileTitle;

  /// No description provided for @selectedInvalidDir.
  ///
  /// In en, this message translates to:
  /// **'Selected path is not a valid app directory'**
  String get selectedInvalidDir;

  /// No description provided for @selectedInvalidApk.
  ///
  /// In en, this message translates to:
  /// **'Selected path is not a valid APK file'**
  String get selectedInvalidApk;

  /// No description provided for @singleApk.
  ///
  /// In en, this message translates to:
  /// **'Single APK'**
  String get singleApk;

  /// No description provided for @appDirectory.
  ///
  /// In en, this message translates to:
  /// **'App Directory'**
  String get appDirectory;

  /// No description provided for @appDirectoryPath.
  ///
  /// In en, this message translates to:
  /// **'App Directory Path'**
  String get appDirectoryPath;

  /// No description provided for @apkFilePath.
  ///
  /// In en, this message translates to:
  /// **'APK File Path'**
  String get apkFilePath;

  /// No description provided for @pathHintDirectory.
  ///
  /// In en, this message translates to:
  /// **'Select or enter app directory path'**
  String get pathHintDirectory;

  /// No description provided for @pathHintApk.
  ///
  /// In en, this message translates to:
  /// **'Select or enter APK file path'**
  String get pathHintApk;

  /// No description provided for @directoryRequirements.
  ///
  /// In en, this message translates to:
  /// **'The directory should contain an APK file and optionally an OBB data directory, or install.txt file.'**
  String get directoryRequirements;

  /// No description provided for @proTipDragDrop.
  ///
  /// In en, this message translates to:
  /// **'Pro tip: You can also drag and drop APK files or app directories anywhere in the app to install them.'**
  String get proTipDragDrop;

  /// No description provided for @addedToQueue.
  ///
  /// In en, this message translates to:
  /// **'Added to queue!'**
  String get addedToQueue;

  /// No description provided for @sideloadApp.
  ///
  /// In en, this message translates to:
  /// **'Sideload App'**
  String get sideloadApp;

  /// No description provided for @installApk.
  ///
  /// In en, this message translates to:
  /// **'Install APK'**
  String get installApk;

  /// No description provided for @tasksTitle.
  ///
  /// In en, this message translates to:
  /// **'Tasks'**
  String get tasksTitle;

  /// No description provided for @tasksTabActive.
  ///
  /// In en, this message translates to:
  /// **'Active'**
  String get tasksTabActive;

  /// No description provided for @tasksTabRecent.
  ///
  /// In en, this message translates to:
  /// **'Recent'**
  String get tasksTabRecent;

  /// No description provided for @tasksEmptyActive.
  ///
  /// In en, this message translates to:
  /// **'No active tasks'**
  String get tasksEmptyActive;

  /// No description provided for @tasksEmptyRecent.
  ///
  /// In en, this message translates to:
  /// **'No recent tasks'**
  String get tasksEmptyRecent;

  /// No description provided for @cancelTask.
  ///
  /// In en, this message translates to:
  /// **'Cancel Task'**
  String get cancelTask;

  /// No description provided for @taskTypeDownload.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get taskTypeDownload;

  /// No description provided for @taskTypeDownloadInstall.
  ///
  /// In en, this message translates to:
  /// **'Download & Install'**
  String get taskTypeDownloadInstall;

  /// No description provided for @taskTypeInstallApk.
  ///
  /// In en, this message translates to:
  /// **'Install APK'**
  String get taskTypeInstallApk;

  /// No description provided for @taskTypeInstallLocalApp.
  ///
  /// In en, this message translates to:
  /// **'Install Local App'**
  String get taskTypeInstallLocalApp;

  /// No description provided for @taskTypeUninstall.
  ///
  /// In en, this message translates to:
  /// **'Uninstall'**
  String get taskTypeUninstall;

  /// No description provided for @taskTypeBackupApp.
  ///
  /// In en, this message translates to:
  /// **'Backup App'**
  String get taskTypeBackupApp;

  /// No description provided for @taskTypeRestoreBackup.
  ///
  /// In en, this message translates to:
  /// **'Restore Backup'**
  String get taskTypeRestoreBackup;

  /// No description provided for @taskStatusWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting'**
  String get taskStatusWaiting;

  /// No description provided for @taskStatusRunning.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get taskStatusRunning;

  /// No description provided for @taskStatusCompleted.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get taskStatusCompleted;

  /// No description provided for @taskStatusFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get taskStatusFailed;

  /// No description provided for @taskStatusCancelled.
  ///
  /// In en, this message translates to:
  /// **'Cancelled'**
  String get taskStatusCancelled;

  /// No description provided for @taskUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get taskUnknown;

  /// No description provided for @backupOptionsTitle.
  ///
  /// In en, this message translates to:
  /// **'Backup Options'**
  String get backupOptionsTitle;

  /// No description provided for @backupSelectParts.
  ///
  /// In en, this message translates to:
  /// **'Select parts to back up:'**
  String get backupSelectParts;

  /// No description provided for @backupAppData.
  ///
  /// In en, this message translates to:
  /// **'App data'**
  String get backupAppData;

  /// No description provided for @backupApk.
  ///
  /// In en, this message translates to:
  /// **'APK'**
  String get backupApk;

  /// No description provided for @backupObbFiles.
  ///
  /// In en, this message translates to:
  /// **'OBB files'**
  String get backupObbFiles;

  /// No description provided for @backupNameSuffix.
  ///
  /// In en, this message translates to:
  /// **'Name suffix (optional)'**
  String get backupNameSuffix;

  /// No description provided for @backupNameSuffixHint.
  ///
  /// In en, this message translates to:
  /// **'e.g. pre-update'**
  String get backupNameSuffixHint;

  /// No description provided for @startBackup.
  ///
  /// In en, this message translates to:
  /// **'Start Backup'**
  String get startBackup;

  /// No description provided for @logsSearchTooltip.
  ///
  /// In en, this message translates to:
  /// **'Search logs by level, message, target, or span id. Examples: \"error\", \"info\", \"adb\", \"connect\", \"13\"'**
  String get logsSearchTooltip;

  /// No description provided for @logsSearchHint.
  ///
  /// In en, this message translates to:
  /// **'Search logs...'**
  String get logsSearchHint;

  /// No description provided for @clearCurrentLogs.
  ///
  /// In en, this message translates to:
  /// **'Clear current logs'**
  String get clearCurrentLogs;

  /// No description provided for @exportLogs.
  ///
  /// In en, this message translates to:
  /// **'Export logs'**
  String get exportLogs;

  /// No description provided for @openLogsDirectory.
  ///
  /// In en, this message translates to:
  /// **'Open logs directory'**
  String get openLogsDirectory;

  /// No description provided for @clearFilters.
  ///
  /// In en, this message translates to:
  /// **'Clear filters'**
  String get clearFilters;

  /// No description provided for @noLogsToDisplay.
  ///
  /// In en, this message translates to:
  /// **'No logs to display'**
  String get noLogsToDisplay;

  /// No description provided for @logsAppearHere.
  ///
  /// In en, this message translates to:
  /// **'Log messages will appear here as they are generated'**
  String get logsAppearHere;

  /// No description provided for @logEntryCopied.
  ///
  /// In en, this message translates to:
  /// **'Log entry copied to clipboard'**
  String get logEntryCopied;

  /// No description provided for @spanId.
  ///
  /// In en, this message translates to:
  /// **'Span ID'**
  String get spanId;

  /// No description provided for @filterBySpanId.
  ///
  /// In en, this message translates to:
  /// **'Filter logs by this span ID'**
  String get filterBySpanId;

  /// No description provided for @spanTrace.
  ///
  /// In en, this message translates to:
  /// **'Span Trace:'**
  String get spanTrace;

  /// No description provided for @spansLabel.
  ///
  /// In en, this message translates to:
  /// **'SPANS'**
  String get spansLabel;

  /// No description provided for @logsSpanEventsTooltip.
  ///
  /// In en, this message translates to:
  /// **'Show/hide span creation and destruction events. Spans track execution flow.'**
  String get logsSpanEventsTooltip;

  /// No description provided for @logsOpenNotSupported.
  ///
  /// In en, this message translates to:
  /// **'Platform not supported. Logs directory path copied to clipboard: {path}'**
  String logsOpenNotSupported(String path);

  /// No description provided for @logsOpenFailed.
  ///
  /// In en, this message translates to:
  /// **'Unable to open logs directory (copied to clipboard): {path}'**
  String logsOpenFailed(String path);

  /// No description provided for @createdWord.
  ///
  /// In en, this message translates to:
  /// **'created'**
  String get createdWord;

  /// No description provided for @closedWord.
  ///
  /// In en, this message translates to:
  /// **'closed'**
  String get closedWord;

  /// No description provided for @noMessage.
  ///
  /// In en, this message translates to:
  /// **'no message'**
  String get noMessage;

  /// No description provided for @uninstallAppTitle.
  ///
  /// In en, this message translates to:
  /// **'Uninstall App'**
  String get uninstallAppTitle;

  /// No description provided for @uninstallConfirmMessage.
  ///
  /// In en, this message translates to:
  /// **'Are you sure you want to uninstall \"{app}\"?\n\nThis will permanently delete the app and all its data.'**
  String uninstallConfirmMessage(String app);

  /// No description provided for @uninstalledDone.
  ///
  /// In en, this message translates to:
  /// **'Uninstalled!'**
  String get uninstalledDone;

  /// No description provided for @uninstalling.
  ///
  /// In en, this message translates to:
  /// **'Uninstalling...'**
  String get uninstalling;

  /// No description provided for @clearLogsTitle.
  ///
  /// In en, this message translates to:
  /// **'Clear Logs'**
  String get clearLogsTitle;

  /// No description provided for @clearLogsMessage.
  ///
  /// In en, this message translates to:
  /// **'This will clear all current session logs. This action cannot be undone.'**
  String get clearLogsMessage;

  /// No description provided for @commonClear.
  ///
  /// In en, this message translates to:
  /// **'Clear'**
  String get commonClear;

  /// No description provided for @logsCopied.
  ///
  /// In en, this message translates to:
  /// **'{count} logs copied to clipboard'**
  String logsCopied(int count);

  /// No description provided for @emptyValue.
  ///
  /// In en, this message translates to:
  /// **'(empty)'**
  String get emptyValue;

  /// No description provided for @errorWord.
  ///
  /// In en, this message translates to:
  /// **'error'**
  String get errorWord;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'ru'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'ru':
      return AppLocalizationsRu();
  }

  throw FlutterError(
      'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
      'an issue with the localizations generation tool. Please file an issue '
      'on GitHub with a reproducible sample app and the gen-l10n configuration '
      'that was used.');
}
