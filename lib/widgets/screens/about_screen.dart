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
    final appState = context.watch<AppState>();
    final backend = appState.backendVersionInfo;

    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l10n.navAbout, style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 8),
          FutureBuilder<PackageInfo>(
            future: _pkgInfo,
            builder: (context, snapshot) {
              final version = snapshot.data?.version ?? 'unknown';
              final build = snapshot.data?.buildNumber ?? '';
              final text = 'YAAS $version${build.isNotEmpty ? "+$build" : ''}';
              return Row(
                children: [
                  Text(text),
                  // const SizedBox(width: 6),
                  // IconButton(
                  //   tooltip: 'Copy version',
                  //   icon: const Icon(Icons.copy, size: 16),
                  //   onPressed: () => copyToClipboard(context, text,
                  //       title: 'Version copied', description: text),
                  // ),
                ],
              );
            },
          ),
          const SizedBox(height: 4),
          if (backend != null) ...[
            Row(
              children: [
                Text('Backend v${backend.backendVersion}'),
                const SizedBox(width: 6),
                // IconButton(
                //   tooltip: 'Copy backend version',
                //   icon: const Icon(Icons.copy, size: 16),
                //   onPressed: () => copyToClipboard(
                //     context,
                //     'v${backend.backendVersion}',
                //     title: 'Version copied',
                //     description: 'v${backend.backendVersion}',
                //   ),
                // ),
                // const SizedBox(width: 8),
                const Text('• commit '),
                Tooltip(
                  message: 'Copy full SHA',
                  child: GestureDetector(
                    onTap: () {
                      final full = (backend.gitCommitHash ??
                              backend.gitCommitHashShort ??
                              '') +
                          (backend.gitDirty ? ' (dirty)' : '');
                      if (full.isEmpty) return;
                      copyToClipboard(
                        context,
                        full,
                        description: full,
                      );
                    },
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: Theme.of(context)
                            .colorScheme
                            .surfaceContainerHighest,
                        borderRadius: BorderRadius.circular(4),
                      ),
                      child: Padding(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 6, vertical: 2),
                        child: Text(
                          '${backend.gitCommitHashShort ?? 'unknown'}${backend.gitDirty ? ' (dirty)' : ''}',
                          style: const TextStyle(fontFamily: 'monospace'),
                        ),
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 6),
                // IconButton(
                //   tooltip: 'Copy full SHA',
                //   icon: const Icon(Icons.copy, size: 16),
                //   onPressed: () {
                //     final full = backend.gitCommitHash ??
                //         backend.gitCommitHashShort ??
                //         '';
                //     if (full.isEmpty) return;
                //     copyToClipboard(
                //       context,
                //       full,
                //       title: 'Commit copied',
                //       description: full,
                //     );
                //   },
                // ),
              ],
            ),
            const SizedBox(height: 4),
            Text(
                'Built ${backend.builtTimeUtc} • ${backend.profile} • ${backend.rustcVersion}'),
          ] else ...[
            const Text('Backend: loading…'),
          ],
          const SizedBox(height: 8),
          // Text(
          //   'Tip: click the commit to copy the full SHA.',
          //   style: Theme.of(context).textTheme.bodySmall?.copyWith(
          //         color: Theme.of(context).colorScheme.onSurfaceVariant,
          //       ),
          // ),
        ],
      ),
    );
  }
}
