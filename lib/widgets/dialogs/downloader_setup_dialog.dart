import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';

import '../../providers/settings_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

class DownloaderSetupDialog extends StatefulWidget {
  const DownloaderSetupDialog({super.key});

  @override
  State<DownloaderSetupDialog> createState() => _DownloaderSetupDialogState();
}

class _DownloaderSetupDialogState extends State<DownloaderSetupDialog> {
  final TextEditingController _urlController = TextEditingController();
  StreamSubscription<RustSignalPack<DownloaderConfigInstallResult>>?
      _installSub;
  bool _isAdding = false;
  String? _errorText;

  @override
  void initState() {
    super.initState();
    _installSub =
        DownloaderConfigInstallResult.rustSignalStream.listen(_onInstallResult);
  }

  @override
  void dispose() {
    _installSub?.cancel();
    _urlController.dispose();
    super.dispose();
  }

  bool _isValidUrl(String value) {
    final url = value.trim();
    if (url.isEmpty) return false;
    final lower = url.toLowerCase();
    if (!lower.startsWith('http://') && !lower.startsWith('https://')) {
      return false;
    }
    return Uri.tryParse(url)?.hasAbsolutePath ?? false;
  }

  void _onAddPressed() {
    final l10n = AppLocalizations.of(context);
    final url = _urlController.text.trim();
    if (!_isValidUrl(url)) {
      setState(() {
        _errorText = l10n.downloaderConfigUrlInvalid;
      });
      return;
    }
    setState(() {
      _isAdding = true;
      _errorText = null;
    });
    InstallDownloaderConfigFromUrlRequest(url: url).sendSignalToRust();
  }

  void _onInstallResult(
    RustSignalPack<DownloaderConfigInstallResult> pack,
  ) {
    if (!_isAdding) return;
    final result = pack.message;
    if (!mounted) return;

    if (result.success) {
      setState(() {
        _isAdding = false;
        _errorText = null;
        _urlController.clear();
      });
    } else {
      final l10n = AppLocalizations.of(context);
      setState(() {
        _isAdding = false;
        _errorText = result.error?.isNotEmpty == true
            ? result.error
            : l10n.downloaderConfigInstallFailed;
      });
    }
  }

  Widget _buildSourceTile(
    BuildContext context,
    InstalledDownloaderConfig source,
    String? selectedId,
    bool enabled,
  ) {
    final selected = source.id == selectedId;
    final theme = Theme.of(context);
    final colorScheme = theme.colorScheme;

    return Card(
      margin: const EdgeInsets.only(bottom: 10),
      color: selected
          ? colorScheme.primaryContainer.withValues(alpha: 0.55)
          : colorScheme.surfaceContainerHighest.withValues(alpha: 0.35),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: BorderSide(
          color: selected ? colorScheme.primary : colorScheme.outlineVariant,
        ),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(14),
        onTap: enabled
            ? () =>
                context.read<SettingsState>().selectDownloaderSource(source.id)
            : null,
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Radio<String>(value: source.id),
              const SizedBox(width: 8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      source.displayName,
                      style: theme.textTheme.titleSmall,
                    ),
                    if (source.description.isNotEmpty) ...[
                      const SizedBox(height: 4),
                      Text(
                        source.description,
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                    const SizedBox(height: 10),
                    Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 10,
                        vertical: 6,
                      ),
                      decoration: BoxDecoration(
                        color: colorScheme.surface.withValues(alpha: 0.9),
                        borderRadius: BorderRadius.circular(999),
                      ),
                      child: Text(
                        AppLocalizations.of(context)
                            .downloaderConfigId(source.id),
                        style: theme.textTheme.labelSmall,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final canAdd = !_isAdding && _isValidUrl(_urlController.text);

    return Consumer<SettingsState>(
      builder: (context, settingsState, _) {
        final sources = settingsState.downloaderSources;
        final selectedId = settingsState.downloaderConfigId;
        final busy = _isAdding || settingsState.isDownloaderSourcesRefreshing;

        return AlertDialog(
          title: Row(
            children: [
              Expanded(child: Text(l10n.downloaderConfigFromUrlTitle)),
              SizedBox(
                width: 36,
                height: 36,
                child: settingsState.isDownloaderSourcesRefreshing
                    ? const Padding(
                        padding: EdgeInsets.all(8),
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : IconButton(
                        tooltip: l10n.downloaderSourcesRefreshTooltip,
                        onPressed: busy
                            ? null
                            : settingsState.refreshDownloaderSources,
                        icon: const Icon(Icons.refresh),
                      ),
              ),
            ],
          ),
          content: SizedBox(
            width: 560,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  l10n.downloaderConfigFromUrlDescription,
                  style: Theme.of(context).textTheme.bodyMedium,
                ),
                if (settingsState.downloaderSourcesError != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    settingsState.downloaderSourcesError!,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Theme.of(context).colorScheme.error,
                        ),
                  ),
                ],
                const SizedBox(height: 16),
                Text(
                  l10n.downloaderSourcesListTitle,
                  style: Theme.of(context).textTheme.titleSmall,
                ),
                const SizedBox(height: 10),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 280),
                  child: sources.isEmpty
                      ? Container(
                          width: double.infinity,
                          padding: const EdgeInsets.all(16),
                          decoration: BoxDecoration(
                            color: Theme.of(context)
                                .colorScheme
                                .surfaceContainerHighest
                                .withValues(alpha: 0.35),
                            borderRadius: BorderRadius.circular(14),
                          ),
                          child: Text(
                            l10n.downloaderSourcesEmpty,
                            style: Theme.of(context).textTheme.bodyMedium,
                          ),
                        )
                      : RadioGroup<String>(
                          groupValue: selectedId,
                          onChanged: (value) {
                            if (busy || value == null) return;
                            settingsState.selectDownloaderSource(value);
                          },
                          child: SingleChildScrollView(
                            child: Column(
                              children: [
                                for (final source in sources)
                                  _buildSourceTile(
                                    context,
                                    source,
                                    selectedId,
                                    !busy,
                                  ),
                              ],
                            ),
                          ),
                        ),
                ),
                const SizedBox(height: 16),
                Text(
                  l10n.downloaderSourcesAddTitle,
                  style: Theme.of(context).textTheme.titleSmall,
                ),
                const SizedBox(height: 10),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _urlController,
                        enabled: !busy,
                        autofocus: sources.isEmpty,
                        decoration: InputDecoration(
                          labelText: l10n.downloaderConfigUrlLabel,
                          border: const OutlineInputBorder(),
                          errorText: _errorText,
                        ),
                        onChanged: (_) {
                          if (_errorText != null) {
                            setState(() {
                              _errorText = null;
                            });
                          } else {
                            setState(() {});
                          }
                        },
                      ),
                    ),
                    const SizedBox(width: 12),
                    FilledButton.icon(
                      onPressed: canAdd ? _onAddPressed : null,
                      icon: _isAdding
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.add),
                      label: Text(
                        _isAdding
                            ? l10n.downloaderConfigInstalling
                            : l10n.downloaderConfigInstallButton,
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(l10n.commonClose),
            ),
          ],
        );
      },
    );
  }
}
