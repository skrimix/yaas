import 'package:flutter/material.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:provider/provider.dart';

import '../../providers/app_state.dart';
import '../../src/l10n/app_localizations.dart';
import '../../utils/utils.dart';

class AboutScreen extends StatefulWidget {
  const AboutScreen({super.key});

  @override
  State<AboutScreen> createState() => _AboutScreenState();
}

class _AboutScreenState extends State<AboutScreen> {
  late Future<PackageInfo> _pkgInfo;

  @override
  void initState() {
    super.initState();
    _pkgInfo = PackageInfo.fromPlatform();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final core = context.watch<AppState>().coreVersionInfo;
    final commitHash = core?.gitCommitHash ?? core?.gitCommitHashShort ?? '';
    final dirtySuffix = core?.gitDirty == true ? ' (dirty)' : '';
    final channel = switch (core?.releaseChannel) {
      'stable' => l10n.buildChannelStable,
      'nightly' => l10n.buildChannelNightly,
      _ => l10n.buildChannelDevelopment,
    };
    final ciBuild = core?.runNumber != null && core?.runAttempt != null
        ? l10n.ciBuildIdentity(core!.runNumber!, core.runAttempt!)
        : null;

    return SingleChildScrollView(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l10n.navAbout, style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 8),
          FutureBuilder<PackageInfo>(
            future: _pkgInfo,
            builder: (context, snapshot) {
              final version = core?.appVersion ??
                  snapshot.data?.version ??
                  l10n.aboutUnknown;
              final build =
                  core?.buildNumber ?? snapshot.data?.buildNumber ?? '';
              return Text('YAAS $version${build.isNotEmpty ? "+$build" : ''}');
            },
          ),
          const SizedBox(height: 4),
          if (core != null) ...[
            Text(ciBuild == null ? channel : '$channel · $ciBuild'),
            const SizedBox(height: 4),
            Row(
              children: [
                Text('${l10n.aboutCommit} '),
                if (commitHash.isNotEmpty)
                  Flexible(
                    child: buildCopyableText(
                      context,
                      '${core.gitCommitHashShort ?? commitHash}$dirtySuffix',
                      copyText: '$commitHash$dirtySuffix',
                      tooltipMessage: l10n.clickToCopyFullSha,
                      style: const TextStyle(fontFamily: 'monospace'),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  )
                else
                  Text(l10n.aboutUnknown,
                      style: const TextStyle(fontFamily: 'monospace')),
              ],
            ),
            const SizedBox(height: 16),
            Text(l10n.aboutCoreDetails,
                style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(height: 8),
            Text(l10n.aboutBuiltAt(core.builtTimeUtc)),
            const SizedBox(height: 4),
            Text(l10n.aboutBuildProfile(core.profile)),
            const SizedBox(height: 4),
            Text(l10n.aboutCompiler(core.rustcVersion)),
          ] else
            Text(l10n.aboutBuildInfoLoading),
        ],
      ),
    );
  }
}
