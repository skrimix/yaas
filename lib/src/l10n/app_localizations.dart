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

  /// No description provided for @navDownloads.
  ///
  /// In en, this message translates to:
  /// **'Downloads'**
  String get navDownloads;

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

  /// No description provided for @settingsNavigationRailLabels.
  ///
  /// In en, this message translates to:
  /// **'Navigation rail labels'**
  String get settingsNavigationRailLabels;

  /// No description provided for @settingsNavigationRailLabelsSelected.
  ///
  /// In en, this message translates to:
  /// **'Selected page only'**
  String get settingsNavigationRailLabelsSelected;

  /// No description provided for @settingsNavigationRailLabelsAll.
  ///
  /// In en, this message translates to:
  /// **'All pages'**
  String get settingsNavigationRailLabelsAll;

  /// No description provided for @settingsStartupPage.
  ///
  /// In en, this message translates to:
  /// **'Startup page'**
  String get settingsStartupPage;

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

  /// No description provided for @settingsMdnsAutoConnect.
  ///
  /// In en, this message translates to:
  /// **'Auto-connect ADB over Wi‑Fi'**
  String get settingsMdnsAutoConnect;

  /// No description provided for @settingsMdnsAutoConnectHelp.
  ///
  /// In en, this message translates to:
  /// **'Discover devices via mDNS on the local network and attempt \'adb connect\' automatically. Takes effect after restart.'**
  String get settingsMdnsAutoConnectHelp;

  /// No description provided for @settingsSectionDownloader.
  ///
  /// In en, this message translates to:
  /// **'Downloader'**
  String get settingsSectionDownloader;

  /// No description provided for @preparingDownloader.
  ///
  /// In en, this message translates to:
  /// **'Preparing downloader...'**
  String get preparingDownloader;

  /// No description provided for @downloadingRcloneFiles.
  ///
  /// In en, this message translates to:
  /// **'Downloading rclone files'**
  String get downloadingRcloneFiles;

  /// No description provided for @installDownloaderConfig.
  ///
  /// In en, this message translates to:
  /// **'Install local config'**
  String get installDownloaderConfig;

  /// No description provided for @installDownloaderConfigFromUrl.
  ///
  /// In en, this message translates to:
  /// **'Set up from preset/URL'**
  String get installDownloaderConfigFromUrl;

  /// No description provided for @settingsSelectDownloaderConfig.
  ///
  /// In en, this message translates to:
  /// **'Select downloader.json'**
  String get settingsSelectDownloaderConfig;

  /// No description provided for @downloaderConfigId.
  ///
  /// In en, this message translates to:
  /// **'Current config ID: {id}'**
  String downloaderConfigId(String id);

  /// No description provided for @downloaderConfigFromUrlTitle.
  ///
  /// In en, this message translates to:
  /// **'Set up downloader config'**
  String get downloaderConfigFromUrlTitle;

  /// No description provided for @downloaderConfigFromUrlDescription.
  ///
  /// In en, this message translates to:
  /// **'Choose a preset configuration or use a custom URL. You can always change the configuration later in the settings.'**
  String get downloaderConfigFromUrlDescription;

  /// No description provided for @downloaderConfigTemplateVrp.
  ///
  /// In en, this message translates to:
  /// **'VRP'**
  String get downloaderConfigTemplateVrp;

  /// No description provided for @downloaderConfigTemplateVrpHint.
  ///
  /// In en, this message translates to:
  /// **'Public server'**
  String get downloaderConfigTemplateVrpHint;

  /// No description provided for @downloaderConfigTemplateVrgRus.
  ///
  /// In en, this message translates to:
  /// **'VR Games RUS'**
  String get downloaderConfigTemplateVrgRus;

  /// No description provided for @downloaderConfigTemplateVrgRusHint.
  ///
  /// In en, this message translates to:
  /// **'CIS countries only'**
  String get downloaderConfigTemplateVrgRusHint;

  /// No description provided for @downloaderConfigTemplateNif.
  ///
  /// In en, this message translates to:
  /// **'NIF'**
  String get downloaderConfigTemplateNif;

  /// No description provided for @downloaderConfigTemplateNifHint.
  ///
  /// In en, this message translates to:
  /// **'Private access'**
  String get downloaderConfigTemplateNifHint;

  /// No description provided for @downloaderConfigTemplateCustom.
  ///
  /// In en, this message translates to:
  /// **'Custom URL'**
  String get downloaderConfigTemplateCustom;

  /// No description provided for @downloaderConfigUrlLabel.
  ///
  /// In en, this message translates to:
  /// **'Config URL'**
  String get downloaderConfigUrlLabel;

  /// No description provided for @downloaderConfigUrlInvalid.
  ///
  /// In en, this message translates to:
  /// **'Please enter a valid http(s) URL'**
  String get downloaderConfigUrlInvalid;

  /// No description provided for @downloaderConfigVrgRusTestButton.
  ///
  /// In en, this message translates to:
  /// **'Test access'**
  String get downloaderConfigVrgRusTestButton;

  /// No description provided for @downloaderConfigVrgRusTestOk.
  ///
  /// In en, this message translates to:
  /// **'Access looks OK.'**
  String get downloaderConfigVrgRusTestOk;

  /// No description provided for @downloaderConfigVrgRusTestError.
  ///
  /// In en, this message translates to:
  /// **'Error ({code}): {error}'**
  String downloaderConfigVrgRusTestError(int code, String error);

  /// No description provided for @downloaderConfigVrgRusTestRequiredTooltip.
  ///
  /// In en, this message translates to:
  /// **'Pass the access test first, or select another server.'**
  String get downloaderConfigVrgRusTestRequiredTooltip;

  /// No description provided for @downloaderConfigNotConfiguredTitle.
  ///
  /// In en, this message translates to:
  /// **'Cloud downloader is not configured'**
  String get downloaderConfigNotConfiguredTitle;

  /// No description provided for @downloaderConfigNotConfiguredDesc.
  ///
  /// In en, this message translates to:
  /// **'Install a downloader.json config to enable cloud app downloads.'**
  String get downloaderConfigNotConfiguredDesc;

  /// No description provided for @downloaderConfigInstallButton.
  ///
  /// In en, this message translates to:
  /// **'Install'**
  String get downloaderConfigInstallButton;

  /// No description provided for @downloaderConfigInstalling.
  ///
  /// In en, this message translates to:
  /// **'Installing...'**
  String get downloaderConfigInstalling;

  /// No description provided for @downloaderConfigInstallFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed to install downloader config'**
  String get downloaderConfigInstallFailed;

  /// No description provided for @loadingApps.
  ///
  /// In en, this message translates to:
  /// **'Loading apps...'**
  String get loadingApps;

  /// No description provided for @loadingAppsSlowHint.
  ///
  /// In en, this message translates to:
  /// **'Loading is taking too long? You can try a different remote.'**
  String get loadingAppsSlowHint;

  /// No description provided for @loadingAppsSlowHintButton.
  ///
  /// In en, this message translates to:
  /// **'Switch remote'**
  String get loadingAppsSlowHintButton;

  /// No description provided for @settingsSectionAppearance.
  ///
  /// In en, this message translates to:
  /// **'Appearance'**
  String get settingsSectionAppearance;

  /// No description provided for @settingsUseSystemColor.
  ///
  /// In en, this message translates to:
  /// **'Use system color'**
  String get settingsUseSystemColor;

  /// No description provided for @settingsSeedColor.
  ///
  /// In en, this message translates to:
  /// **'Accent color'**
  String get settingsSeedColor;

  /// No description provided for @settingsTheme.
  ///
  /// In en, this message translates to:
  /// **'Theme'**
  String get settingsTheme;

  /// No description provided for @themeAuto.
  ///
  /// In en, this message translates to:
  /// **'Auto'**
  String get themeAuto;

  /// No description provided for @themeDark.
  ///
  /// In en, this message translates to:
  /// **'Dark'**
  String get themeDark;

  /// No description provided for @themeLight.
  ///
  /// In en, this message translates to:
  /// **'Light'**
  String get themeLight;

  /// No description provided for @colorDeepPurple.
  ///
  /// In en, this message translates to:
  /// **'Deep purple'**
  String get colorDeepPurple;

  /// No description provided for @colorIndigo.
  ///
  /// In en, this message translates to:
  /// **'Indigo'**
  String get colorIndigo;

  /// No description provided for @colorBlue.
  ///
  /// In en, this message translates to:
  /// **'Blue'**
  String get colorBlue;

  /// No description provided for @colorCyan.
  ///
  /// In en, this message translates to:
  /// **'Cyan'**
  String get colorCyan;

  /// No description provided for @colorTeal.
  ///
  /// In en, this message translates to:
  /// **'Teal'**
  String get colorTeal;

  /// No description provided for @colorGreen.
  ///
  /// In en, this message translates to:
  /// **'Green'**
  String get colorGreen;

  /// No description provided for @colorLime.
  ///
  /// In en, this message translates to:
  /// **'Lime'**
  String get colorLime;

  /// No description provided for @colorAmber.
  ///
  /// In en, this message translates to:
  /// **'Amber'**
  String get colorAmber;

  /// No description provided for @colorOrange.
  ///
  /// In en, this message translates to:
  /// **'Orange'**
  String get colorOrange;

  /// No description provided for @colorDeepOrange.
  ///
  /// In en, this message translates to:
  /// **'Deep orange'**
  String get colorDeepOrange;

  /// No description provided for @colorRed.
  ///
  /// In en, this message translates to:
  /// **'Red'**
  String get colorRed;

  /// No description provided for @colorPink.
  ///
  /// In en, this message translates to:
  /// **'Pink'**
  String get colorPink;

  /// No description provided for @colorPurple.
  ///
  /// In en, this message translates to:
  /// **'Purple'**
  String get colorPurple;

  /// No description provided for @colorBrown.
  ///
  /// In en, this message translates to:
  /// **'Brown'**
  String get colorBrown;

  /// No description provided for @colorBlueGrey.
  ///
  /// In en, this message translates to:
  /// **'Blue grey'**
  String get colorBlueGrey;

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

  /// No description provided for @settingsCustomColorHint.
  ///
  /// In en, this message translates to:
  /// **'Enter 6-digit hex color (e.g., FF5733)'**
  String get settingsCustomColorHint;

  /// No description provided for @settingsInvalidHexColor.
  ///
  /// In en, this message translates to:
  /// **'Invalid hex color'**
  String get settingsInvalidHexColor;

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
  /// **'Value in KiB/s or with B|K|M|G|T|P suffix or more (click for documentation)'**
  String get settingsBandwidthHelper;

  /// No description provided for @settingsDownloadsCleanup.
  ///
  /// In en, this message translates to:
  /// **'Downloads Cleanup'**
  String get settingsDownloadsCleanup;

  /// No description provided for @settingsWriteLegacyReleaseJson.
  ///
  /// In en, this message translates to:
  /// **'Write legacy release.json'**
  String get settingsWriteLegacyReleaseJson;

  /// No description provided for @settingsWriteLegacyReleaseJsonHelp.
  ///
  /// In en, this message translates to:
  /// **'Write release.json in legacy format for compatibility with QLoader'**
  String get settingsWriteLegacyReleaseJsonHelp;

  /// No description provided for @settingsCleanupDeleteAfterInstall.
  ///
  /// In en, this message translates to:
  /// **'Remove after installation'**
  String get settingsCleanupDeleteAfterInstall;

  /// No description provided for @settingsCleanupKeepOneVersion.
  ///
  /// In en, this message translates to:
  /// **'Keep one version'**
  String get settingsCleanupKeepOneVersion;

  /// No description provided for @settingsCleanupKeepTwoVersions.
  ///
  /// In en, this message translates to:
  /// **'Keep two versions'**
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

  /// No description provided for @statusAdbServerStartFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed to start ADB server'**
  String get statusAdbServerStartFailed;

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
  /// **'Device connected'**
  String get statusAdbConnected;

  /// No description provided for @statusAdbDeviceUnauthorized.
  ///
  /// In en, this message translates to:
  /// **'Device unauthorized'**
  String get statusAdbDeviceUnauthorized;

  /// No description provided for @statusAdbStateOffline.
  ///
  /// In en, this message translates to:
  /// **'Offline'**
  String get statusAdbStateOffline;

  /// No description provided for @statusAdbStateBootloader.
  ///
  /// In en, this message translates to:
  /// **'Bootloader'**
  String get statusAdbStateBootloader;

  /// No description provided for @statusAdbStateRecovery.
  ///
  /// In en, this message translates to:
  /// **'Recovery'**
  String get statusAdbStateRecovery;

  /// No description provided for @statusAdbStateNoPermissions.
  ///
  /// In en, this message translates to:
  /// **'No permissions'**
  String get statusAdbStateNoPermissions;

  /// No description provided for @statusAdbStateSideload.
  ///
  /// In en, this message translates to:
  /// **'Sideload'**
  String get statusAdbStateSideload;

  /// No description provided for @statusAdbStateAuthorizing.
  ///
  /// In en, this message translates to:
  /// **'Authorizing'**
  String get statusAdbStateAuthorizing;

  /// No description provided for @statusAdbStateUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get statusAdbStateUnknown;

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

  /// No description provided for @statusDeviceInfoWireless.
  ///
  /// In en, this message translates to:
  /// **'Device: {name}\nAddress: {address}\nSerial: {serial}'**
  String statusDeviceInfoWireless(String name, String address, String serial);

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

  /// No description provided for @dragDropDownloaderConfigTitle.
  ///
  /// In en, this message translates to:
  /// **'Downloader config'**
  String get dragDropDownloaderConfigTitle;

  /// No description provided for @dragDropDownloaderConfigHint.
  ///
  /// In en, this message translates to:
  /// **'Drop downloader.json to install or update the downloader configuration'**
  String get dragDropDownloaderConfigHint;

  /// No description provided for @deviceCasting.
  ///
  /// In en, this message translates to:
  /// **'Casting'**
  String get deviceCasting;

  /// No description provided for @deviceStartCasting.
  ///
  /// In en, this message translates to:
  /// **'Cast'**
  String get deviceStartCasting;

  /// No description provided for @castingWirelessUnsupported.
  ///
  /// In en, this message translates to:
  /// **'Casting is not supported for wireless devices yet. Connect via USB.'**
  String get castingWirelessUnsupported;

  /// No description provided for @castingRequiresDownloadTitle.
  ///
  /// In en, this message translates to:
  /// **'Meta Quest Casting required'**
  String get castingRequiresDownloadTitle;

  /// No description provided for @castingRequiresDownloadPrompt.
  ///
  /// In en, this message translates to:
  /// **'This action needs the Meta Quest Casting tool. Download it now?'**
  String get castingRequiresDownloadPrompt;

  /// No description provided for @castingToolTitle.
  ///
  /// In en, this message translates to:
  /// **'Meta Quest Casting'**
  String get castingToolTitle;

  /// No description provided for @castingToolDownloadUpdate.
  ///
  /// In en, this message translates to:
  /// **'Download / Update'**
  String get castingToolDownloadUpdate;

  /// No description provided for @castingToolInstallUpdateTitle.
  ///
  /// In en, this message translates to:
  /// **'Install / Update Casting'**
  String get castingToolInstallUpdateTitle;

  /// No description provided for @castingToolInstallUpdateDesc.
  ///
  /// In en, this message translates to:
  /// **'This will download the Meta Quest Casting tool and install it into the app data directory.'**
  String get castingToolInstallUpdateDesc;

  /// No description provided for @castingToolDownloading.
  ///
  /// In en, this message translates to:
  /// **'Downloading Casting tool...'**
  String get castingToolDownloading;

  /// No description provided for @castingToolStatusInstalled.
  ///
  /// In en, this message translates to:
  /// **'Installed'**
  String get castingToolStatusInstalled;

  /// No description provided for @castingToolStatusNotInstalled.
  ///
  /// In en, this message translates to:
  /// **'Not installed'**
  String get castingToolStatusNotInstalled;

  /// No description provided for @castingToolRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get castingToolRefresh;

  /// No description provided for @connectDeviceToInstall.
  ///
  /// In en, this message translates to:
  /// **'Connect a device to install apps'**
  String get connectDeviceToInstall;

  /// No description provided for @connectDeviceToRestore.
  ///
  /// In en, this message translates to:
  /// **'Connect a device to restore backups'**
  String get connectDeviceToRestore;

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

  /// No description provided for @diagnosticsTitle.
  ///
  /// In en, this message translates to:
  /// **'Connection Diagnostics'**
  String get diagnosticsTitle;

  /// No description provided for @diagnosticsAdbServer.
  ///
  /// In en, this message translates to:
  /// **'ADB server'**
  String get diagnosticsAdbServer;

  /// No description provided for @diagnosticsDevices.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get diagnosticsDevices;

  /// No description provided for @diagnosticsAuthorization.
  ///
  /// In en, this message translates to:
  /// **'Authorization'**
  String get diagnosticsAuthorization;

  /// No description provided for @diagnosticsActiveDevice.
  ///
  /// In en, this message translates to:
  /// **'Active device'**
  String get diagnosticsActiveDevice;

  /// No description provided for @diagnosticsAdbPath.
  ///
  /// In en, this message translates to:
  /// **'ADB path'**
  String get diagnosticsAdbPath;

  /// No description provided for @diagnosticsServerNotRunningDesc.
  ///
  /// In en, this message translates to:
  /// **'ADB server is not running. Ensure ADB is installed and accessible in PATH or set the ADB path in Settings.'**
  String get diagnosticsServerNotRunningDesc;

  /// No description provided for @diagnosticsServerStartingDesc.
  ///
  /// In en, this message translates to:
  /// **'ADB server is starting...'**
  String get diagnosticsServerStartingDesc;

  /// No description provided for @diagnosticsServerStartFailedDesc.
  ///
  /// In en, this message translates to:
  /// **'Failed to start the ADB server. Check the ADB path in Settings and view logs for details.'**
  String get diagnosticsServerStartFailedDesc;

  /// No description provided for @diagnosticsServerRunningDesc.
  ///
  /// In en, this message translates to:
  /// **'ADB server is running.'**
  String get diagnosticsServerRunningDesc;

  /// No description provided for @diagnosticsNoDevicesDesc.
  ///
  /// In en, this message translates to:
  /// **'No devices detected. Enable ADB/developer mode and connect via USB.'**
  String get diagnosticsNoDevicesDesc;

  /// No description provided for @diagnosticsDevicesAvailableDesc.
  ///
  /// In en, this message translates to:
  /// **'Devices detected ({count})'**
  String diagnosticsDevicesAvailableDesc(int count);

  /// No description provided for @diagnosticsUnauthorizedDesc.
  ///
  /// In en, this message translates to:
  /// **'Device is unauthorized. Confirm the authorization prompt on the device.'**
  String get diagnosticsUnauthorizedDesc;

  /// No description provided for @diagnosticsAuthorizedDesc.
  ///
  /// In en, this message translates to:
  /// **'Device authorized.'**
  String get diagnosticsAuthorizedDesc;

  /// No description provided for @diagnosticsConnectedDesc.
  ///
  /// In en, this message translates to:
  /// **'Device connected and ready.'**
  String get diagnosticsConnectedDesc;

  /// No description provided for @diagnosticsUnknownDesc.
  ///
  /// In en, this message translates to:
  /// **'Unknown state.'**
  String get diagnosticsUnknownDesc;

  /// No description provided for @diagnosticsConfiguredPath.
  ///
  /// In en, this message translates to:
  /// **'Configured path: {path}'**
  String diagnosticsConfiguredPath(String path);

  /// No description provided for @diagnosticsUsingSystemPath.
  ///
  /// In en, this message translates to:
  /// **'Using system PATH'**
  String get diagnosticsUsingSystemPath;

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

  /// No description provided for @deviceWirelessAdb.
  ///
  /// In en, this message translates to:
  /// **'Wireless ADB'**
  String get deviceWirelessAdb;

  /// No description provided for @deviceEnableWirelessAdb.
  ///
  /// In en, this message translates to:
  /// **'Enable ADB over Wi‑Fi'**
  String get deviceEnableWirelessAdb;

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

  /// No description provided for @detailsReviewsTitle.
  ///
  /// In en, this message translates to:
  /// **'Recent reviews'**
  String get detailsReviewsTitle;

  /// No description provided for @detailsReviewsUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Reviews are unavailable for this app.'**
  String get detailsReviewsUnavailable;

  /// No description provided for @detailsReviewsError.
  ///
  /// In en, this message translates to:
  /// **'Failed to load reviews.'**
  String get detailsReviewsError;

  /// No description provided for @detailsReviewsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No reviews available yet.'**
  String get detailsReviewsEmpty;

  /// No description provided for @detailsDeveloperResponse.
  ///
  /// In en, this message translates to:
  /// **'Developer response'**
  String get detailsDeveloperResponse;

  /// No description provided for @detailsReviewHelpful.
  ///
  /// In en, this message translates to:
  /// **'Helpful'**
  String get detailsReviewHelpful;

  /// No description provided for @detailsReviewHelpfulCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one {{count} person found this helpful} other {{count} people found this helpful}}'**
  String detailsReviewHelpfulCount(int count);

  /// No description provided for @reviewsSortBy.
  ///
  /// In en, this message translates to:
  /// **'Sort by'**
  String get reviewsSortBy;

  /// No description provided for @reviewsSortHelpful.
  ///
  /// In en, this message translates to:
  /// **'Most helpful'**
  String get reviewsSortHelpful;

  /// No description provided for @reviewsSortNewest.
  ///
  /// In en, this message translates to:
  /// **'Newest'**
  String get reviewsSortNewest;

  /// No description provided for @previous.
  ///
  /// In en, this message translates to:
  /// **'Previous'**
  String get previous;

  /// No description provided for @next.
  ///
  /// In en, this message translates to:
  /// **'Next'**
  String get next;

  /// No description provided for @reviewsReadAll.
  ///
  /// In en, this message translates to:
  /// **'Read all reviews'**
  String get reviewsReadAll;

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

  /// No description provided for @commonDownload.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get commonDownload;

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

  /// No description provided for @updateTo.
  ///
  /// In en, this message translates to:
  /// **'Update to {to}'**
  String updateTo(String to);

  /// No description provided for @downgradeAppTitle.
  ///
  /// In en, this message translates to:
  /// **'Downgrade App'**
  String get downgradeAppTitle;

  /// No description provided for @downgradeConfirmMessage.
  ///
  /// In en, this message translates to:
  /// **'Attempt to downgrade to version {versionCode}? This may cause issues.'**
  String downgradeConfirmMessage(String versionCode);

  /// No description provided for @holdShiftToDowngrade.
  ///
  /// In en, this message translates to:
  /// **'Hold Shift to downgrade to this version'**
  String get holdShiftToDowngrade;

  /// No description provided for @downgradeToThisVersion.
  ///
  /// In en, this message translates to:
  /// **'Downgrade to this version'**
  String get downgradeToThisVersion;

  /// No description provided for @holdShiftToViewVersions.
  ///
  /// In en, this message translates to:
  /// **'Hold Shift to view versions'**
  String get holdShiftToViewVersions;

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

  /// No description provided for @addToFavorites.
  ///
  /// In en, this message translates to:
  /// **'Add to favorites'**
  String get addToFavorites;

  /// No description provided for @removeFromFavorites.
  ///
  /// In en, this message translates to:
  /// **'Remove from favorites'**
  String get removeFromFavorites;

  /// No description provided for @clearFavorites.
  ///
  /// In en, this message translates to:
  /// **'Clear favorites'**
  String get clearFavorites;

  /// No description provided for @clearFavoritesTitle.
  ///
  /// In en, this message translates to:
  /// **'Clear Favorites'**
  String get clearFavoritesTitle;

  /// No description provided for @clearFavoritesConfirm.
  ///
  /// In en, this message translates to:
  /// **'Remove all apps from favorites?'**
  String get clearFavoritesConfirm;

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

  /// No description provided for @cloudStatusInstalled.
  ///
  /// In en, this message translates to:
  /// **'Installed'**
  String get cloudStatusInstalled;

  /// No description provided for @cloudStatusNewerVersion.
  ///
  /// In en, this message translates to:
  /// **'Newer version'**
  String get cloudStatusNewerVersion;

  /// No description provided for @cloudStatusOlderVersion.
  ///
  /// In en, this message translates to:
  /// **'Older version'**
  String get cloudStatusOlderVersion;

  /// No description provided for @cloudStatusTooltip.
  ///
  /// In en, this message translates to:
  /// **'Installed {installedCode} - Cloud {cloudCode}'**
  String cloudStatusTooltip(String installedCode, String cloudCode);

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

  /// No description provided for @showFavoritesOnly.
  ///
  /// In en, this message translates to:
  /// **'Show favorites only'**
  String get showFavoritesOnly;

  /// No description provided for @showingFavoritesOnly.
  ///
  /// In en, this message translates to:
  /// **'Showing favorites only'**
  String get showingFavoritesOnly;

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

  /// No description provided for @addSelectedToFavorites.
  ///
  /// In en, this message translates to:
  /// **'Favorite Selected'**
  String get addSelectedToFavorites;

  /// No description provided for @removeSelectedFromFavorites.
  ///
  /// In en, this message translates to:
  /// **'Unfavorite Selected'**
  String get removeSelectedFromFavorites;

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

  /// No description provided for @controllerStatusNotConnected.
  ///
  /// In en, this message translates to:
  /// **'Not connected'**
  String get controllerStatusNotConnected;

  /// No description provided for @controllerStatusActive.
  ///
  /// In en, this message translates to:
  /// **'Active'**
  String get controllerStatusActive;

  /// No description provided for @controllerStatusInactive.
  ///
  /// In en, this message translates to:
  /// **'Inactive'**
  String get controllerStatusInactive;

  /// No description provided for @controllerStatusDisabled.
  ///
  /// In en, this message translates to:
  /// **'Disabled'**
  String get controllerStatusDisabled;

  /// No description provided for @controllerStatusSearching.
  ///
  /// In en, this message translates to:
  /// **'Searching'**
  String get controllerStatusSearching;

  /// No description provided for @controllerStatusUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get controllerStatusUnknown;

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

  /// No description provided for @mute.
  ///
  /// In en, this message translates to:
  /// **'Mute'**
  String get mute;

  /// No description provided for @unmute.
  ///
  /// In en, this message translates to:
  /// **'Unmute'**
  String get unmute;

  /// No description provided for @close.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get close;

  /// No description provided for @pause.
  ///
  /// In en, this message translates to:
  /// **'Pause'**
  String get pause;

  /// No description provided for @checkingTrailerAvailability.
  ///
  /// In en, this message translates to:
  /// **'Checking trailer availability...'**
  String get checkingTrailerAvailability;

  /// No description provided for @trailerAvailable.
  ///
  /// In en, this message translates to:
  /// **'Trailer available'**
  String get trailerAvailable;

  /// No description provided for @noTrailer.
  ///
  /// In en, this message translates to:
  /// **'No trailer'**
  String get noTrailer;

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

  /// No description provided for @openDownloadsFolder.
  ///
  /// In en, this message translates to:
  /// **'Open Downloads Folder'**
  String get openDownloadsFolder;

  /// No description provided for @downloadsTitle.
  ///
  /// In en, this message translates to:
  /// **'Downloads'**
  String get downloadsTitle;

  /// No description provided for @deleteAllDownloads.
  ///
  /// In en, this message translates to:
  /// **'Delete all downloads'**
  String get deleteAllDownloads;

  /// No description provided for @deleteAllDownloadsTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete All Downloads'**
  String get deleteAllDownloadsTitle;

  /// No description provided for @deleteAllDownloadsConfirm.
  ///
  /// In en, this message translates to:
  /// **'Are you sure you want to delete all downloads?'**
  String get deleteAllDownloadsConfirm;

  /// No description provided for @deleteAllDownloadsResult.
  ///
  /// In en, this message translates to:
  /// **'Deleted {removed}, skipped {skipped}'**
  String deleteAllDownloadsResult(String removed, String skipped);

  /// No description provided for @deleteDownloadTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete Download'**
  String get deleteDownloadTitle;

  /// No description provided for @deleteDownloadConfirm.
  ///
  /// In en, this message translates to:
  /// **'Are you sure you want to delete \"{name}\"?'**
  String deleteDownloadConfirm(String name);

  /// No description provided for @downloadDeletedTitle.
  ///
  /// In en, this message translates to:
  /// **'Download deleted'**
  String get downloadDeletedTitle;

  /// No description provided for @noBackupsFound.
  ///
  /// In en, this message translates to:
  /// **'No backups found.'**
  String get noBackupsFound;

  /// No description provided for @noDownloadsFound.
  ///
  /// In en, this message translates to:
  /// **'No downloads found.'**
  String get noDownloadsFound;

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

  /// No description provided for @taskKindDownload.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get taskKindDownload;

  /// No description provided for @taskKindDownloadInstall.
  ///
  /// In en, this message translates to:
  /// **'Download & Install'**
  String get taskKindDownloadInstall;

  /// No description provided for @taskKindInstallApk.
  ///
  /// In en, this message translates to:
  /// **'Install APK'**
  String get taskKindInstallApk;

  /// No description provided for @taskKindInstallLocalApp.
  ///
  /// In en, this message translates to:
  /// **'Install Local App'**
  String get taskKindInstallLocalApp;

  /// No description provided for @taskKindUninstall.
  ///
  /// In en, this message translates to:
  /// **'Uninstall'**
  String get taskKindUninstall;

  /// No description provided for @taskKindBackupApp.
  ///
  /// In en, this message translates to:
  /// **'Backup App'**
  String get taskKindBackupApp;

  /// No description provided for @taskKindRestoreBackup.
  ///
  /// In en, this message translates to:
  /// **'Restore Backup'**
  String get taskKindRestoreBackup;

  /// No description provided for @taskKindDonateApp.
  ///
  /// In en, this message translates to:
  /// **'Donate App'**
  String get taskKindDonateApp;

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

  /// No description provided for @videoLink.
  ///
  /// In en, this message translates to:
  /// **'Video link'**
  String get videoLink;

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

  /// No description provided for @downgradeAppsTitle.
  ///
  /// In en, this message translates to:
  /// **'Downgrade Apps'**
  String get downgradeAppsTitle;

  /// No description provided for @downgradeMultipleConfirmMessage.
  ///
  /// In en, this message translates to:
  /// **'The following apps will be downgraded. This may cause issues.'**
  String get downgradeMultipleConfirmMessage;

  /// No description provided for @downgradeItemFormat.
  ///
  /// In en, this message translates to:
  /// **'{name} ({installedCode} → {cloudCode})'**
  String downgradeItemFormat(
      String name, String installedCode, String cloudCode);

  /// No description provided for @downloadedStatusNewerVersion.
  ///
  /// In en, this message translates to:
  /// **'Update available'**
  String get downloadedStatusNewerVersion;

  /// No description provided for @downloadedStatusToolTip.
  ///
  /// In en, this message translates to:
  /// **'Click to go to the list'**
  String get downloadedStatusToolTip;

  /// No description provided for @downloadedStatusInstalled.
  ///
  /// In en, this message translates to:
  /// **'Installed'**
  String get downloadedStatusInstalled;

  /// No description provided for @downloadedStatusInstalledNewer.
  ///
  /// In en, this message translates to:
  /// **'Installed newer'**
  String get downloadedStatusInstalledNewer;

  /// No description provided for @downloadedStatusInstalledOlder.
  ///
  /// In en, this message translates to:
  /// **'Installed older'**
  String get downloadedStatusInstalledOlder;

  /// No description provided for @emptyValue.
  ///
  /// In en, this message translates to:
  /// **'(empty)'**
  String get emptyValue;

  /// No description provided for @navDonate.
  ///
  /// In en, this message translates to:
  /// **'Donate'**
  String get navDonate;

  /// No description provided for @donateAppsDescription.
  ///
  /// In en, this message translates to:
  /// **'Select apps to donate (upload) to the community'**
  String get donateAppsDescription;

  /// No description provided for @donateShowFiltered.
  ///
  /// In en, this message translates to:
  /// **'Show filtered apps'**
  String get donateShowFiltered;

  /// No description provided for @donateHideFiltered.
  ///
  /// In en, this message translates to:
  /// **'Hide filtered apps'**
  String get donateHideFiltered;

  /// No description provided for @donateFilterReasonBlacklisted.
  ///
  /// In en, this message translates to:
  /// **'Blacklisted'**
  String get donateFilterReasonBlacklisted;

  /// No description provided for @donateFilterReasonRenamed.
  ///
  /// In en, this message translates to:
  /// **'Renamed package'**
  String get donateFilterReasonRenamed;

  /// No description provided for @donateFilterReasonSystemUnwanted.
  ///
  /// In en, this message translates to:
  /// **'System/unwanted app'**
  String get donateFilterReasonSystemUnwanted;

  /// No description provided for @donateFilterReasonAlreadyExists.
  ///
  /// In en, this message translates to:
  /// **'Already exists'**
  String get donateFilterReasonAlreadyExists;

  /// No description provided for @donateStatusNewApp.
  ///
  /// In en, this message translates to:
  /// **'New app'**
  String get donateStatusNewApp;

  /// No description provided for @donateStatusNewerVersion.
  ///
  /// In en, this message translates to:
  /// **'Newer version'**
  String get donateStatusNewerVersion;

  /// No description provided for @donateDonateButton.
  ///
  /// In en, this message translates to:
  /// **'Donate'**
  String get donateDonateButton;

  /// No description provided for @donateNoAppsAvailable.
  ///
  /// In en, this message translates to:
  /// **'No apps available for donation'**
  String get donateNoAppsAvailable;

  /// No description provided for @donateNoAppsWithFilters.
  ///
  /// In en, this message translates to:
  /// **'No apps match the current filters'**
  String get donateNoAppsWithFilters;

  /// No description provided for @donateLoadingCloudApps.
  ///
  /// In en, this message translates to:
  /// **'Loading cloud apps list...'**
  String get donateLoadingCloudApps;

  /// No description provided for @copyDisplayName.
  ///
  /// In en, this message translates to:
  /// **'Copy display name'**
  String get copyDisplayName;

  /// No description provided for @donateDownloaderNotAvailable.
  ///
  /// In en, this message translates to:
  /// **'Downloader not available'**
  String get donateDownloaderNotAvailable;

  /// No description provided for @popularity.
  ///
  /// In en, this message translates to:
  /// **'Popularity'**
  String get popularity;

  /// No description provided for @popularityDay1.
  ///
  /// In en, this message translates to:
  /// **'24h'**
  String get popularityDay1;

  /// No description provided for @popularityDay7.
  ///
  /// In en, this message translates to:
  /// **'7 days'**
  String get popularityDay7;

  /// No description provided for @popularityDay30.
  ///
  /// In en, this message translates to:
  /// **'30 days'**
  String get popularityDay30;

  /// No description provided for @popularityPercent.
  ///
  /// In en, this message translates to:
  /// **'{percent}%'**
  String popularityPercent(int percent);

  /// No description provided for @sortPopularityMost.
  ///
  /// In en, this message translates to:
  /// **'Popularity (Most popular)'**
  String get sortPopularityMost;

  /// No description provided for @sortPopularityLeast.
  ///
  /// In en, this message translates to:
  /// **'Popularity (Least popular)'**
  String get sortPopularityLeast;

  /// No description provided for @lowSpaceWarningTitle.
  ///
  /// In en, this message translates to:
  /// **'Low Storage Space'**
  String get lowSpaceWarningTitle;

  /// No description provided for @lowSpaceWarningMessage.
  ///
  /// In en, this message translates to:
  /// **'This installation will leave less than {threshold} of free space on the device (<={remaining} remaining). Do you want to continue?'**
  String lowSpaceWarningMessage(String threshold, String remaining);

  /// No description provided for @lowSpaceWarningContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue anyway'**
  String get lowSpaceWarningContinue;
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
