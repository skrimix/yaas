import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:provider/provider.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../providers/app_update_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../../utils/utils.dart';
import 'setting_dropdown.dart';
import 'setting_row.dart';

class AppUpdateSection extends StatelessWidget {
  const AppUpdateSection({super.key});

  Future<void> _openLink(BuildContext context, String url,
      {String? baseUrl}) async {
    try {
      final uri =
          baseUrl == null ? Uri.parse(url) : Uri.parse(baseUrl).resolve(url);
      if ((uri.scheme == 'https' || uri.scheme == 'http') &&
          await launchUrl(uri, mode: LaunchMode.externalApplication)) {
        return;
      }
    } catch (_) {
      // Report malformed URLs and platform launch failures in the same way.
    }
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        content: Text(AppLocalizations.of(context).couldNotOpenUrl(url)),
      ));
    }
  }

  @override
  Widget build(BuildContext context) {
    final update = context.watch<AppUpdateState>();
    final state = update.snapshot;
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final release = state?.release;
    final phase = state?.phase;
    final status = switch (phase) {
      null => l10n.updatesLoading,
      AppUpdatePhase.idle => l10n.updatesIdle,
      AppUpdatePhase.checking => l10n.updatesChecking,
      AppUpdatePhase.available => l10n.updatesAvailable,
      AppUpdatePhase.upToDate => l10n.updatesUpToDate,
      AppUpdatePhase.downloading => l10n.updatesDownloading,
      AppUpdatePhase.ready => l10n.updatesReady,
      AppUpdatePhase.preparing => l10n.updatesPreparing,
      AppUpdatePhase.awaitingExit => l10n.updatesAwaitingExit,
    };
    final busy = phase == null ||
        phase == AppUpdatePhase.checking ||
        phase == AppUpdatePhase.downloading ||
        phase == AppUpdatePhase.preparing ||
        phase == AppUpdatePhase.awaitingExit ||
        update.requestPending;
    final total = release?.packageSize.toInt() ?? 0;
    final received = state?.receivedBytes.toInt() ?? 0;
    final progress = phase == AppUpdatePhase.downloading && total > 0
        ? (received / total).clamp(0.0, 1.0)
        : null;

    return Card(
      margin: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 0),
            child: Text(l10n.updatesTitle, style: theme.textTheme.titleLarge),
          ),
          SettingRow(
            label: l10n.updatesChannel,
            description: Text(l10n.updatesChannelHint),
            enabled: update.canChangePreferences,
            control: SettingDropdown<UpdateChannel>(
              key: ValueKey((update.channel, update.savingPreferences)),
              label: l10n.updatesChannel,
              initialValue: update.channel,
              items: [
                DropdownMenuItem(
                    value: UpdateChannel.stable,
                    child: Text(l10n.buildChannelStable)),
                DropdownMenuItem(
                    value: UpdateChannel.nightly,
                    child: Text(l10n.buildChannelNightly)),
              ],
              onChanged: update.canChangePreferences
                  ? (value) => update.savePreferences(channel: value)
                  : null,
            ),
          ),
          SettingRow(
            label: l10n.updatesCheckOnStartup,
            compact: true,
            enabled: update.canChangePreferences,
            control: Switch(
              value: update.checkUpdatesOnStartup,
              onChanged: update.canChangePreferences
                  ? (value) => update.savePreferences(checkOnStartup: value)
                  : null,
            ),
          ),
          const Divider(height: 1, indent: 16, endIndent: 16),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (update.savingPreferences) ...[
                  Text(l10n.updatesSaving),
                  const SizedBox(height: 8),
                ],
                if (update.saveError != null) ...[
                  SelectableText(l10n.settingsSaveError(update.saveError!),
                      style: TextStyle(color: theme.colorScheme.error)),
                  const SizedBox(height: 8),
                ],
                Text(status, style: theme.textTheme.titleMedium),
                if (busy) ...[
                  const SizedBox(height: 12),
                  LinearProgressIndicator(value: progress),
                  if (phase == AppUpdatePhase.downloading) ...[
                    const SizedBox(height: 8),
                    Text('${formatSize(received, 1)} / ${formatSize(total, 1)}'
                        '${progress == null ? '' : ' · ${(progress * 100).floor()}%'}'),
                  ],
                ],
                if (release != null) ...[
                  const SizedBox(height: 16),
                  SelectableText(
                      'YAAS ${release.version}+${release.buildNumber}',
                      style: theme.textTheme.titleMedium),
                  const SizedBox(height: 4),
                  Text(
                      '${release.channel == UpdateChannel.stable ? l10n.buildChannelStable : l10n.buildChannelNightly}'
                      ' · ${formatSize(total, 1)}'),
                  if (release.channel == UpdateChannel.nightly)
                    Text(l10n.ciBuildIdentity(release.runNumber.toString(),
                        release.runAttempt.toString())),
                  const SizedBox(height: 8),
                  TextButton.icon(
                    onPressed: () => _openLink(context, release.releaseUrl),
                    icon: const Icon(Icons.open_in_new, size: 18),
                    label: Text(l10n.updatesReleaseLink),
                  ),
                  if (release.notes.trim().isNotEmpty)
                    _UpdateDetails(
                      key: ValueKey(release.candidateId),
                      label: l10n.updatesReleaseNotes,
                      icon: Icons.notes_rounded,
                      child: MarkdownBody(
                        data: release.notes,
                        selectable: true,
                        onTapLink: (text, href, title) {
                          if (href != null) {
                            _openLink(context, href,
                                baseUrl: release.releaseUrl);
                          }
                        },
                      ),
                    ),
                ],
                if (state?.installationUnavailableReason != null) ...[
                  const SizedBox(height: 16),
                  Text(l10n.updatesUnavailable,
                      style: theme.textTheme.titleSmall),
                  const SizedBox(height: 4),
                  SelectableText(state!.installationUnavailableReason!,
                      style: theme.textTheme.bodyMedium?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant)),
                ],
                if (state?.error != null || state?.errorKind != null) ...[
                  const SizedBox(height: 16),
                  Text(_errorSummary(l10n, state!.errorKind),
                      style: TextStyle(color: theme.colorScheme.error)),
                  if (state.error != null) ...[
                    const SizedBox(height: 8),
                    _UpdateDetails(
                      label: l10n.updatesErrorDetails,
                      icon: Icons.info_outline_rounded,
                      child: SelectableText(state.error!),
                    ),
                  ],
                ],
                if (phase == AppUpdatePhase.ready) ...[
                  const SizedBox(height: 16),
                  Text(l10n.updatesRestartHint),
                ],
                const SizedBox(height: 16),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    if (phase == null ||
                        phase == AppUpdatePhase.idle ||
                        phase == AppUpdatePhase.upToDate ||
                        phase == AppUpdatePhase.available ||
                        phase == AppUpdatePhase.ready)
                      OutlinedButton.icon(
                        onPressed: update.canCheck ? update.check : null,
                        icon: const Icon(Icons.refresh),
                        label: Text(phase == AppUpdatePhase.available ||
                                phase == AppUpdatePhase.ready
                            ? l10n.updatesCheckAgain
                            : l10n.updatesCheck),
                      ),
                    if (phase == AppUpdatePhase.available)
                      FilledButton.icon(
                        onPressed: update.canDownload ? update.download : null,
                        icon: const Icon(Icons.download),
                        label: Text(l10n.updatesDownload),
                      ),
                    if (phase == AppUpdatePhase.ready)
                      FilledButton.icon(
                        onPressed: update.canInstall ? update.install : null,
                        icon: const Icon(Icons.restart_alt),
                        label: Text(l10n.updatesInstall),
                      ),
                    if (phase == AppUpdatePhase.checking ||
                        phase == AppUpdatePhase.downloading ||
                        phase == AppUpdatePhase.preparing)
                      OutlinedButton(
                        onPressed: update.canCancel ? update.cancel : null,
                        child: Text(l10n.commonCancel),
                      ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  String _errorSummary(AppLocalizations l10n, AppUpdateErrorKind? kind) =>
      switch (kind) {
        AppUpdateErrorKind.network => l10n.updatesNetworkError,
        AppUpdateErrorKind.noRelease => l10n.updatesNoReleaseError,
        AppUpdateErrorKind.incompleteRelease =>
          l10n.updatesIncompleteReleaseError,
        AppUpdateErrorKind.invalidMetadata => l10n.updatesInvalidMetadataError,
        AppUpdateErrorKind.noPackage => l10n.updatesNoPackageError,
        AppUpdateErrorKind.integrity => l10n.updatesIntegrityError,
        AppUpdateErrorKind.installation => l10n.updatesInstallationError,
        AppUpdateErrorKind.invalidRequest => l10n.updatesInvalidRequestError,
        AppUpdateErrorKind.recoveryRequired => l10n.updatesRecoveryError,
        null => l10n.updatesUnknownError,
      };
}

class _UpdateDetails extends StatefulWidget {
  const _UpdateDetails({
    super.key,
    required this.label,
    required this.icon,
    required this.child,
  });

  final String label;
  final IconData icon;
  final Widget child;

  @override
  State<_UpdateDetails> createState() => _UpdateDetailsState();
}

class _UpdateDetailsState extends State<_UpdateDetails> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final duration = MediaQuery.disableAnimationsOf(context)
        ? Duration.zero
        : const Duration(milliseconds: 200);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Semantics(
          expanded: _expanded,
          child: TextButton(
            onPressed: () => setState(() => _expanded = !_expanded),
            style: TextButton.styleFrom(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              shape: const StadiumBorder(),
              splashFactory: NoSplash.splashFactory,
            ).copyWith(
              overlayColor: WidgetStateProperty.resolveWith((states) =>
                  states.contains(WidgetState.hovered) ||
                          states.contains(WidgetState.focused)
                      ? colors.primary.withValues(alpha: 0.08)
                      : Colors.transparent),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(widget.icon, size: 18),
                const SizedBox(width: 8),
                Flexible(child: Text(widget.label)),
                const SizedBox(width: 6),
                AnimatedRotation(
                  turns: _expanded ? 0.5 : 0,
                  duration: duration,
                  curve: Curves.easeInOut,
                  child: const Icon(Icons.expand_more_rounded, size: 18),
                ),
              ],
            ),
          ),
        ),
        AnimatedSize(
          duration: duration,
          curve: Curves.easeInOut,
          alignment: Alignment.topLeft,
          child: SizedBox(
            width: double.infinity,
            child: _expanded
                ? Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: widget.child,
                  )
                : const SizedBox.shrink(),
          ),
        ),
      ],
    );
  }
}
