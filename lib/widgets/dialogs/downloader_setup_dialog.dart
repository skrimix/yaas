import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:rinf/rinf.dart';

import '../../providers/settings_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../common/selectable_link_text.dart';

class DownloaderSetupDialog extends StatefulWidget {
  const DownloaderSetupDialog({super.key});

  @override
  State<DownloaderSetupDialog> createState() => _DownloaderSetupDialogState();
}

class _DownloaderSetupDialogState extends State<DownloaderSetupDialog> {
  final TextEditingController _urlController = TextEditingController();
  StreamSubscription<RustSignalPack<DownloaderConfigInstallResult>>?
      _installSub;
  StreamSubscription<RustSignalPack<DownloaderSourceRemovedResult>>? _removeSub;
  bool _isAdding = false;
  String? _removingSourceId;
  String? _hoveredSourceId;
  bool _showAddSection = false;
  String? _errorText;

  InputDecoration _buildUrlInputDecoration(AppLocalizations l10n) {
    final errorText = _errorText;

    if (errorText == null || errorText.isEmpty) {
      return InputDecoration(
        labelText: l10n.downloaderConfigUrlLabel,
        border: const OutlineInputBorder(),
      );
    }

    final decoration = InputDecoration(
      labelText: l10n.downloaderConfigUrlLabel,
      border: const OutlineInputBorder(),
      errorMaxLines: 4,
    );

    return decoration.copyWith(
      error: Tooltip(
        message: errorText,
        waitDuration: const Duration(milliseconds: 400),
        child: Text(
          errorText,
          maxLines: 4,
          overflow: TextOverflow.ellipsis,
        ),
      ),
    );
  }

  @override
  void initState() {
    super.initState();
    _installSub =
        DownloaderConfigInstallResult.rustSignalStream.listen(_onInstallResult);
    _removeSub =
        DownloaderSourceRemovedResult.rustSignalStream.listen(_onRemoveResult);
  }

  @override
  void dispose() {
    _installSub?.cancel();
    _removeSub?.cancel();
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
        _showAddSection = false;
        _errorText = null;
        _urlController.clear();
      });
    } else {
      final l10n = AppLocalizations.of(context);
      setState(() {
        _isAdding = false;
        _showAddSection = true;
        _errorText = result.error?.isNotEmpty == true
            ? result.error
            : l10n.downloaderConfigInstallFailed;
      });
    }
  }

  void _onRemoveResult(
    RustSignalPack<DownloaderSourceRemovedResult> pack,
  ) {
    if (!mounted || _removingSourceId != pack.message.configId) return;
    setState(() {
      _removingSourceId = null;
    });
  }

  Future<void> _confirmRemoveSource(InstalledDownloaderConfig source) async {
    final l10n = AppLocalizations.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.downloaderSourceRemoveTitle),
        content: Text(l10n.downloaderSourceRemoveConfirm(source.displayName)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(l10n.remove),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;

    setState(() {
      _removingSourceId = source.id;
    });
    context.read<SettingsState>().removeDownloaderSource(source.id);
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
    final isRemoving = _removingSourceId == source.id;
    final showRemoveAction = isRemoving || _hoveredSourceId == source.id;

    return Material(
      color: selected
          ? colorScheme.secondaryContainer.withValues(alpha: 0.45)
          : Colors.transparent,
      borderRadius: BorderRadius.circular(12),
      child: InkWell(
        borderRadius: BorderRadius.circular(12),
        onHover: (hovering) {
          if (_hoveredSourceId == source.id && !hovering) {
            setState(() {
              _hoveredSourceId = null;
            });
          } else if (_hoveredSourceId != source.id && hovering) {
            setState(() {
              _hoveredSourceId = source.id;
            });
          }
        },
        onTap: enabled && !selected
            ? () {
                context.read<SettingsState>().selectDownloaderSource(source.id);
                Navigator.of(context).pop();
              }
            : null,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 10),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Radio<String>(value: source.id),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      source.displayName,
                      style: theme.textTheme.titleSmall,
                    ),
                    const SizedBox(height: 2),
                    Text(
                      source.id,
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: colorScheme.onSurfaceVariant,
                        fontStyle: FontStyle.italic,
                      ),
                    ),
                    if (source.description.isNotEmpty) ...[
                      const SizedBox(height: 6),
                      SelectableLinkText(
                        text: source.description,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 8),
              Visibility(
                visible: showRemoveAction,
                child: IgnorePointer(
                  ignoring: !showRemoveAction,
                  child: IconButton(
                    tooltip: AppLocalizations.of(context)
                        .downloaderSourceRemoveTooltip(source.displayName),
                    onPressed: enabled && !isRemoving
                        ? () => _confirmRemoveSource(source)
                        : null,
                    constraints: const BoxConstraints.tightFor(
                      width: 36,
                      height: 36,
                    ),
                    padding: const EdgeInsets.all(6),
                    iconSize: 24,
                    icon: isRemoving
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.delete_outline),
                  ),
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
        final busy = _isAdding ||
            _removingSourceId != null ||
            settingsState.isDownloaderSourcesRefreshing;
        final showAddSection =
            _showAddSection || sources.isEmpty || _errorText != null;

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
                        padding: const EdgeInsets.all(6),
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
                const SizedBox(height: 12),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 280),
                  child: sources.isEmpty
                      ? Padding(
                          padding: const EdgeInsets.symmetric(vertical: 12),
                          child: Text(
                            l10n.downloaderSourcesEmpty,
                            style: Theme.of(context)
                                .textTheme
                                .bodyMedium
                                ?.copyWith(
                                  color: Theme.of(context)
                                      .colorScheme
                                      .onSurfaceVariant,
                                ),
                          ),
                        )
                      : RadioGroup<String>(
                          groupValue: selectedId,
                          onChanged: (value) {
                            if (busy || value == null) return;
                            settingsState.selectDownloaderSource(value);
                            Navigator.of(context).pop();
                          },
                          child: SingleChildScrollView(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                for (var i = 0; i < sources.length; i++) ...[
                                  _buildSourceTile(
                                    context,
                                    sources[i],
                                    selectedId,
                                    !busy,
                                  ),
                                  if (i != sources.length - 1)
                                    const SizedBox(height: 4),
                                ],
                              ],
                            ),
                          ),
                        ),
                ),
                const SizedBox(height: 12),
                const Divider(height: 1),
                const SizedBox(height: 8),
                InkWell(
                  borderRadius: BorderRadius.circular(8),
                  onTap: (busy && !showAddSection) || sources.isEmpty
                      ? null
                      : () {
                          setState(() {
                            _showAddSection = !showAddSection;
                          });
                        },
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 8,
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          l10n.downloaderSourcesAddTitle,
                          style: Theme.of(context)
                              .textTheme
                              .bodySmall
                              ?.copyWith(
                                color: Theme.of(context).colorScheme.primary,
                                fontWeight: FontWeight.w600,
                              ),
                        ),
                        const SizedBox(width: 4),
                        AnimatedRotation(
                          turns: showAddSection ? 0.5 : 0,
                          duration: const Duration(milliseconds: 180),
                          child: Icon(
                            Icons.expand_more,
                            size: 18,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                AnimatedSize(
                  duration: const Duration(milliseconds: 180),
                  curve: Curves.easeInOut,
                  child: showAddSection
                      ? Padding(
                          padding: const EdgeInsets.only(top: 4, bottom: 4),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Expanded(
                                child: TextField(
                                  controller: _urlController,
                                  enabled: !busy,
                                  autofocus: sources.isEmpty,
                                  decoration: _buildUrlInputDecoration(l10n),
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
                              Padding(
                                padding: const EdgeInsets.only(top: 8),
                                child: FilledButton(
                                  onPressed: canAdd ? _onAddPressed : null,
                                  child: Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      if (_isAdding) ...[
                                        const SizedBox(
                                          width: 16,
                                          height: 16,
                                          child: CircularProgressIndicator(
                                            strokeWidth: 2,
                                          ),
                                        ),
                                        const SizedBox(width: 8),
                                      ],
                                      Text(
                                        _isAdding
                                            ? l10n.downloaderConfigInstalling
                                            : l10n
                                                .downloaderConfigInstallButton,
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                            ],
                          ),
                        )
                      : const SizedBox.shrink(),
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
