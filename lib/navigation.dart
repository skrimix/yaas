import 'package:flutter/material.dart';

import 'src/l10n/app_localizations.dart';
import 'widgets/screens/download_apps_screen.dart';
import 'widgets/screens/local_sideload_screen.dart';
import 'widgets/screens/manage_apps_screen.dart';
import 'widgets/screens/backups_screen.dart';
import 'widgets/screens/downloads_screen.dart';
import 'widgets/screens/home_screen.dart';
import 'widgets/screens/logs_screen.dart';
import 'widgets/screens/settings_screen.dart';
import 'widgets/screens/about_screen.dart';
import 'widgets/screens/donate_apps_screen.dart';

typedef AppPageLabelBuilder = String Function(AppLocalizations l10n);
typedef AppPageContentBuilder = Widget Function();

class AppPageDefinition {
  const AppPageDefinition({
    required this.key,
    required this.icon,
    required AppPageLabelBuilder labelBuilder,
    required AppPageContentBuilder contentBuilder,
  })  : _labelBuilder = labelBuilder,
        _contentBuilder = contentBuilder;

  final String key;
  final IconData icon;
  final AppPageLabelBuilder _labelBuilder;
  final AppPageContentBuilder _contentBuilder;

  String label(AppLocalizations l10n) => _labelBuilder(l10n);

  NavigationRailDestination toNavigationDestination(AppLocalizations l10n) {
    return NavigationRailDestination(
      icon: Icon(icon),
      label: Text(label(l10n)),
    );
  }

  Widget buildContent() => _contentBuilder();
}

class AppPageRegistry {
  AppPageRegistry._();

  static final List<AppPageDefinition> pages = List.unmodifiable(_buildPages());
  static final List<String> pageKeys =
      List.unmodifiable(pages.map((page) => page.key));

  static List<AppPageDefinition> _buildPages() {
    late final List<AppPageDefinition> pages;
    pages = [
      AppPageDefinition(
        key: 'home',
        icon: Icons.home,
        labelBuilder: (l10n) => l10n.navHome,
        contentBuilder: () => const HomeScreen(),
      ),
      AppPageDefinition(
        key: 'manage',
        icon: Icons.apps,
        labelBuilder: (l10n) => l10n.navManage,
        contentBuilder: () => const ManageAppsScreen(),
      ),
      AppPageDefinition(
        key: 'download',
        icon: Icons.cloud_download,
        labelBuilder: (l10n) => l10n.navDownload,
        contentBuilder: () => const DownloadAppsScreen(),
      ),
      AppPageDefinition(
        key: 'downloads',
        icon: Icons.download_done_outlined,
        labelBuilder: (l10n) => l10n.navDownloads,
        contentBuilder: () => const DownloadsScreen(),
      ),
      AppPageDefinition(
        key: 'sideload',
        icon: Icons.arrow_circle_down,
        labelBuilder: (l10n) => l10n.navSideload,
        contentBuilder: () => const LocalSideloadScreen(),
      ),
      AppPageDefinition(
        key: 'backups',
        icon: Icons.archive,
        labelBuilder: (l10n) => l10n.navBackups,
        contentBuilder: () => const BackupsScreen(),
      ),
      AppPageDefinition(
        key: 'donate',
        icon: Icons.cloud_upload,
        labelBuilder: (l10n) => l10n.navDonate,
        contentBuilder: () => const DonateAppsScreen(),
      ),
      AppPageDefinition(
        key: 'settings',
        icon: Icons.settings,
        labelBuilder: (l10n) => l10n.navSettings,
        contentBuilder: () => const SettingsScreen(),
      ),
      AppPageDefinition(
        key: 'logs',
        icon: Icons.terminal,
        labelBuilder: (l10n) => l10n.navLogs,
        contentBuilder: () => const LogsScreen(),
      ),
      AppPageDefinition(
        key: 'about',
        icon: Icons.info,
        labelBuilder: (l10n) => l10n.navAbout,
        contentBuilder: () => const AboutScreen(),
      ),
    ];
    return pages;
  }
}
