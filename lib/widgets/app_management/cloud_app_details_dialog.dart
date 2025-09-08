import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../src/bindings/bindings.dart';
import 'package:rinf/rinf.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';
import '../../utils/utils.dart';
import 'cloud_app_list.dart';

class CloudAppDetailsDialog extends StatefulWidget {
  const CloudAppDetailsDialog({
    super.key,
    required this.cachedApp,
    required this.onDownload,
    required this.onInstall,
  });

  final CachedAppData cachedApp;
  final void Function(String fullName) onDownload;
  final void Function(String fullName) onInstall;

  @override
  State<CloudAppDetailsDialog> createState() => _CloudAppDetailsDialogState();
}

class _CloudAppDetailsDialogState extends State<CloudAppDetailsDialog> {
  StreamSubscription<RustSignalPack<AppDetailsResponse>>? _sub;
  AppDetailsResponse? _details;
  bool _loading = true;
  final ScrollController _descScrollController = ScrollController();

  @override
  void initState() {
    super.initState();
    _sub = AppDetailsResponse.rustSignalStream.listen((event) {
      final message = event.message;
      if (message.packageName == widget.cachedApp.app.packageName) {
        setState(() {
          _details = message;
          _loading = false;
        });
      }
    });

    GetAppDetailsRequest(packageName: widget.cachedApp.app.packageName)
        .sendSignalToRust();
  }

  @override
  void dispose() {
    _sub?.cancel();
    _descScrollController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;

    final effectiveTitle =
        _details?.displayName ?? widget.cachedApp.app.appName;
    final showRating = _details != null && !_details!.notFound;

    return AlertDialog(
      title: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            effectiveTitle,
            style: textTheme.titleLarge,
            overflow: TextOverflow.ellipsis,
          ),
          const SizedBox(height: 4),
          Text(
            widget.cachedApp.app.fullName,
            style: textTheme.bodySmall?.copyWith(
              color: textTheme.bodySmall?.color?.withValues(alpha: 0.7),
            ),
            overflow: TextOverflow.ellipsis,
          ),
        ],
      ),
      content: SizedBox(
        width: 900,
        child: _loading
            ? const SizedBox(
                height: 120,
                child: Center(child: CircularProgressIndicator()),
              )
            : Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Left: Media placeholder
                  Tooltip(
                    message: l10n.underConstruction,
                    child: Container(
                      width: 450,
                      height: 270,
                      decoration: BoxDecoration(
                        color: theme.colorScheme.surfaceContainerHighest,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: const Center(
                        child: Icon(Icons.ondemand_video, size: 48),
                      ),
                    ),
                  ),
                  const SizedBox(width: 16),
                  // Right: Details + description
                  Expanded(
                    child: SizedBox(
                      height: 270,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          // Size and rating row
                          Wrap(
                            spacing: 12,
                            runSpacing: 4,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  const Icon(Icons.download_outlined, size: 16),
                                  const SizedBox(width: 6),
                                  Text(widget.cachedApp.formattedSize),
                                ],
                              ),
                              if (showRating && _details!.ratingAverage != null)
                                Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    const Icon(Icons.star_rate_rounded,
                                        size: 18, color: Colors.amber),
                                    const SizedBox(width: 4),
                                    Text(_formatRating(
                                        _details!.ratingAverage!)),
                                    if (_details!.ratingCount != null) ...[
                                      const SizedBox(width: 4),
                                      Text('(${_details!.ratingCount})',
                                          style: textTheme.bodySmall?.copyWith(
                                            color: textTheme.bodySmall?.color
                                                ?.withValues(alpha: 0.7),
                                          )),
                                    ],
                                  ],
                                ),
                            ],
                          ),
                          const SizedBox(height: 8),
                          if (_details?.error != null)
                            Padding(
                              padding: const EdgeInsets.only(bottom: 8.0),
                              child: Text(
                                _details!.error!,
                                style: textTheme.bodySmall?.copyWith(
                                  color: theme.colorScheme.error,
                                ),
                              ),
                            ),
                          if (showRating && _details?.description != null)
                            Expanded(
                              child: DecoratedBox(
                                decoration: BoxDecoration(
                                  border: Border.all(
                                    color: theme.colorScheme.outlineVariant,
                                  ),
                                  borderRadius: BorderRadius.circular(6),
                                ),
                                child: Padding(
                                  padding: const EdgeInsets.all(8.0),
                                  child: Scrollbar(
                                    controller: _descScrollController,
                                    child: SingleChildScrollView(
                                      controller: _descScrollController,
                                      child: SelectableText(
                                        _details!.description!,
                                        style: textTheme.bodyMedium,
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                            ),
                        ],
                      ),
                    ),
                  ),
                ],
              ),
      ),
      actions: [
        TextButton(
          onPressed: () {
            final text = _buildCopyBuffer(l10n, effectiveTitle);
            copyToClipboard(context, text);
          },
          child: Text(l10n.commonCopy),
        ),
        TextButton(
          onPressed: () => widget.onDownload(widget.cachedApp.app.fullName),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.download, size: 18),
              const SizedBox(width: 6),
              Text(l10n.downloadToComputer),
            ],
          ),
        ),
        Consumer<DeviceState>(builder: (context, deviceState, _) {
          return FilledButton.icon(
            onPressed: deviceState.isConnected
                ? () => widget.onInstall(widget.cachedApp.app.fullName)
                : null,
            icon: const Icon(Icons.install_mobile),
            label: Text(deviceState.isConnected
                ? l10n.downloadAndInstall
                : l10n.downloadAndInstallNotConnected),
          );
        }),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.commonClose),
        ),
      ],
    );
  }

  String _formatRating(double rating) {
    return rating.toStringAsFixed(2);
  }

  String _buildCopyBuffer(AppLocalizations l10n, String effectiveTitle) {
    final buf = StringBuffer();
    buf.writeln(effectiveTitle);
    buf.writeln(widget.cachedApp.app.fullName);
    if (_details != null && !_details!.notFound) {
      if (_details!.ratingAverage != null) {
        buf.writeln(
            '${l10n.detailsRating} ${_formatRating(_details!.ratingAverage!)}'
            '${_details!.ratingCount != null ? ' (${_details!.ratingCount})' : ''}');
      }
      if (_details!.description != null) {
        buf.writeln('\n');
        buf.writeln(_details!.description);
      }
    }
    return buf.toString();
  }
}
