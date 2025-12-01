import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:cached_network_image/cached_network_image.dart';
import 'package:video_player/video_player.dart';
import 'package:flutter_cache_manager/flutter_cache_manager.dart';
import 'package:http/http.dart' as http;
import 'package:intl/intl.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../src/bindings/bindings.dart';
import 'package:rinf/rinf.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/device_state.dart';
import '../../providers/cloud_apps_state.dart';
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
  final void Function(String fullName, String truePackageName) onDownload;
  final Future<void> Function(BuildContext context) onInstall;

  @override
  State<CloudAppDetailsDialog> createState() => _CloudAppDetailsDialogState();
}

class _CloudAppDetailsDialogState extends State<CloudAppDetailsDialog> {
  static _ReviewSort _lastSort =
      _ReviewSort.helpful; // remember for this session
  StreamSubscription<RustSignalPack<AppDetailsResponse>>? _sub;
  StreamSubscription<RustSignalPack<AppReviewsResponse>>? _reviewsSub;
  AppDetailsResponse? _details;
  bool _loading = true;
  final ScrollController _descScrollController = ScrollController();
  List<AppReview>? _reviews;
  bool _reviewsLoading = false;
  String? _reviewsError;
  String? _currentReviewsAppId;
  int? _reviewsTotal;
  final int _pageSize = 5;
  int _pageIndex = 0;
  _ReviewSort _sort = _ReviewSort.helpful;

  @override
  void initState() {
    super.initState();
    // Initialize sort from last session choice
    _sort = _lastSort;
    _sub = AppDetailsResponse.rustSignalStream.listen((event) {
      final message = event.message;
      if (message.packageName != widget.cachedApp.app.packageName &&
          message.packageName != widget.cachedApp.app.truePackageName) {
        return;
      }

      final newAppId = message.appId;
      var shouldFetchReviews = false;

      setState(() {
        _details = message;
        _loading = false;

        if (message.notFound || newAppId == null || newAppId.isEmpty) {
          _reviews = null;
          _reviewsError = null;
          _reviewsLoading = false;
          _currentReviewsAppId = null;
        } else if (newAppId != _currentReviewsAppId) {
          _currentReviewsAppId = newAppId;
          _reviews = null;
          _reviewsError = null;
          _reviewsLoading = true;
          _pageIndex = 0;
          shouldFetchReviews = true;
        }
      });

      if (shouldFetchReviews && newAppId != null) {
        _fetchReviews();
      }
    });

    _reviewsSub = AppReviewsResponse.rustSignalStream.listen((event) {
      final message = event.message;
      if (message.appId != _currentReviewsAppId) {
        return;
      }

      setState(() {
        _reviews = message.reviews;
        _reviewsTotal = message.total;
        _reviewsError = message.error;
        _reviewsLoading = false;
      });
    });

    GetAppDetailsRequest(
      packageName: widget.cachedApp.app.truePackageName,
    ).sendSignalToRust();
  }

  @override
  void dispose() {
    _sub?.cancel();
    _reviewsSub?.cancel();
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
            : SingleChildScrollView(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SizedBox(
                      height: 270,
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          // Left: Thumbnail + hover overlay + trailer player
                          _CloudAppMedia(
                            truePackageName:
                                widget.cachedApp.app.truePackageName,
                            width: 450,
                            height: 270,
                          ),
                          const SizedBox(width: 16),
                          // Right: Details + description
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                // Size, rating, and popularity row
                                Wrap(
                                  spacing: 12,
                                  runSpacing: 4,
                                  crossAxisAlignment: WrapCrossAlignment.center,
                                  children: [
                                    Row(
                                      mainAxisSize: MainAxisSize.min,
                                      children: [
                                        const Icon(Icons.download_outlined,
                                            size: 16),
                                        const SizedBox(width: 6),
                                        Text(widget.cachedApp.formattedSize),
                                      ],
                                    ),
                                    if (showRating &&
                                        _details!.ratingAverage != null)
                                      Row(
                                        mainAxisSize: MainAxisSize.min,
                                        children: [
                                          const Icon(Icons.star_rate_rounded,
                                              size: 18, color: Colors.amber),
                                          const SizedBox(width: 4),
                                          Text(_formatRating(
                                              _details!.ratingAverage!)),
                                          if (_details!.ratingCount !=
                                              null) ...[
                                            const SizedBox(width: 4),
                                            Text('(${_details!.ratingCount})',
                                                style: textTheme.bodySmall
                                                    ?.copyWith(
                                                  color: textTheme
                                                      .bodySmall?.color
                                                      ?.withValues(alpha: 0.7),
                                                )),
                                          ],
                                        ],
                                      ),
                                    _buildPopularityChip(context),
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
                                          color:
                                              theme.colorScheme.outlineVariant,
                                        ),
                                        borderRadius: BorderRadius.circular(6),
                                      ),
                                      child: Padding(
                                        padding: const EdgeInsets.all(8.0),
                                        child: Scrollbar(
                                          controller: _descScrollController,
                                          child: SingleChildScrollView(
                                            controller: _descScrollController,
                                            child: _buildDescriptionContent(
                                              context,
                                              _details!.description!,
                                              textTheme.bodyMedium,
                                            ),
                                          ),
                                        ),
                                      ),
                                    ),
                                  ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                    const SizedBox(height: 16),
                    _buildReviewsSection(context),
                  ],
                ),
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
          onPressed: () => widget.onDownload(widget.cachedApp.app.fullName,
              widget.cachedApp.app.truePackageName),
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
                ? () => widget.onInstall(context)
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

  Widget _buildPopularityChip(BuildContext context) {
    final pop = widget.cachedApp.app.popularity;
    if (pop == null) return const SizedBox.shrink();

    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;

    final items = <({int value, String period})>[
      (value: pop.day1 ?? 0, period: l10n.popularityDay1),
      (value: pop.day7 ?? 0, period: l10n.popularityDay7),
      (value: pop.day30 ?? 0, period: l10n.popularityDay30),
    ];

    final maxValue = items.map((e) => e.value).reduce((a, b) => a > b ? a : b);
    final Color iconColor;
    if (maxValue >= 70) {
      iconColor = Colors.orange.shade700;
    } else if (maxValue >= 40) {
      iconColor = Colors.orange.shade400;
    } else {
      iconColor = scheme.onSurfaceVariant.withValues(alpha: 0.6);
    }

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(
          Icons.local_fire_department_rounded,
          size: 14,
          color: iconColor,
        ),
        const SizedBox(width: 4),
        ...items.map((item) {
          final isLast = item == items.last;
          return Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                '${item.period}: ${item.value}%',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant.withValues(alpha: 0.8),
                ),
              ),
              if (!isLast)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 6),
                  child: Text(
                    '•',
                    style: TextStyle(
                      color: scheme.onSurfaceVariant.withValues(alpha: 0.4),
                    ),
                  ),
                ),
            ],
          );
        }),
      ],
    );
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

  // Parses the description, replacing Oculus-style media markdown
  // - Video: ![{"height":720,"type":"video","width":1280}](https://...mp4)
  //   Renders a localized "Video link" button
  // - Image: ![{"height":732,"type":"image","width":2160}](https://...png)
  //   Renders the image inline (click to open externally)
  Widget _buildDescriptionContent(
    BuildContext context,
    String description,
    TextStyle? style,
  ) {
    final parts = _splitDescriptionWithVideoLinks(description);
    final children = <Widget>[];

    for (final part in parts) {
      if (part.isVideo) {
        children.add(
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 4.0),
            child: Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                icon: const Icon(Icons.play_circle_outline),
                label: Text(AppLocalizations.of(context).videoLink),
                onPressed: () => _openExternalUrl(context, part.url!),
              ),
            ),
          ),
        );
      } else if (part.isImage) {
        children.add(
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 8.0),
            child: _InlineNetworkImage(
              url: part.url!,
              onTap: () => _openExternalUrl(context, part.url!),
            ),
          ),
        );
      } else if (part.text != null && part.text!.isNotEmpty) {
        children.add(SelectableText(part.text!, style: style));
      }
    }

    if (children.isEmpty) {
      return SelectableText(description, style: style);
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: children,
    );
  }

  Future<void> _openExternalUrl(BuildContext context, String url) async {
    final l10n = AppLocalizations.of(context);
    final uri = Uri.parse(url);
    final ok = await launchUrl(uri, mode: LaunchMode.externalApplication);
    if (!ok && context.mounted) {
      final msg = l10n.couldNotOpenUrl(url);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(msg)),
      );
    }
  }

  // Simple split that extracts media markdown and keeps surrounding text.
  List<_DescPart> _splitDescriptionWithVideoLinks(String input) {
    final regex = RegExp(r'!\[(.*?)\]\((https?:\/\/[^\s)]+)\)');
    final result = <_DescPart>[];
    int last = 0;
    for (final m in regex.allMatches(input)) {
      if (m.start > last) {
        result.add(_DescPart.text(input.substring(last, m.start)));
      }
      final alt = m.group(1) ?? '';
      final url = m.group(2) ?? '';
      final kind = _classifyMarkdown(alt, url);
      if (kind == _MediaKind.video) {
        result.add(_DescPart.video(url));
      } else if (kind == _MediaKind.image) {
        result.add(_DescPart.image(url));
      } else {
        // Not a video pattern; keep original literal markdown text.
        result.add(_DescPart.text(input.substring(m.start, m.end)));
      }
      last = m.end;
    }
    if (last < input.length) {
      result.add(_DescPart.text(input.substring(last)));
    }
    return result;
  }

  _MediaKind _classifyMarkdown(String alt, String url) {
    final altL = alt.toLowerCase();
    final urlL = url.toLowerCase();
    if (urlL.endsWith('.mp4') || urlL.contains('.mp4?')) {
      return _MediaKind.video;
    }
    if (altL.contains('type') && altL.contains('video')) {
      return _MediaKind.video;
    }
    if (urlL.endsWith('.png') ||
        urlL.endsWith('.jpg') ||
        urlL.endsWith('.jpeg') ||
        urlL.endsWith('.gif') ||
        urlL.endsWith('.webp') ||
        urlL.contains('.png?') ||
        urlL.contains('.jpg?') ||
        urlL.contains('.jpeg?') ||
        urlL.contains('.gif?') ||
        urlL.contains('.webp?') ||
        (altL.contains('type') && altL.contains('image'))) {
      return _MediaKind.image;
    }
    return _MediaKind.other;
  }

  Widget _buildReviewsSection(BuildContext context) {
    final details = _details;
    if (details == null || details.notFound) {
      return const SizedBox.shrink();
    }

    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;
    final appId = details.appId;

    Widget body;
    if (appId == null || appId.isEmpty) {
      body = Text(
        l10n.detailsReviewsUnavailable,
        style: textTheme.bodySmall,
      );
    } else if (_reviewsLoading) {
      body = const SizedBox(
        height: 120,
        child: Center(child: CircularProgressIndicator()),
      );
    } else if (_reviewsError != null) {
      body = Text(
        _reviewsError!,
        style: textTheme.bodySmall?.copyWith(
          color: theme.colorScheme.error,
        ),
      );
    } else {
      final reviews = _reviews ?? const <AppReview>[];
      final reviewFallback =
          details.displayName ?? widget.cachedApp.app.appName;
      if (reviews.isEmpty && (_reviewsTotal ?? 0) == 0) {
        body = Text(
          l10n.detailsReviewsEmpty,
          style: textTheme.bodySmall,
        );
      } else {
        body = Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Sorting selector
            Row(
              children: [
                Text(l10n.reviewsSortBy, style: textTheme.bodySmall),
                const SizedBox(width: 8),
                DropdownButton<_ReviewSort>(
                  value: _sort,
                  onChanged: (v) {
                    if (v != null && v != _sort) {
                      setState(() {
                        _sort = v;
                        _lastSort = v; // remember selection
                        _pageIndex = 0;
                        _reviewsLoading = true;
                      });
                      _fetchReviews();
                    }
                  },
                  items: [
                    DropdownMenuItem(
                      value: _ReviewSort.helpful,
                      child: Text(l10n.reviewsSortHelpful),
                    ),
                    DropdownMenuItem(
                      value: _ReviewSort.newest,
                      child: Text(l10n.reviewsSortNewest),
                    ),
                  ],
                ),
                const Spacer(),
                if (appId.isNotEmpty)
                  TextButton.icon(
                    icon: const Icon(Icons.open_in_new),
                    label: Text(l10n.reviewsReadAll),
                    onPressed: () => _openExternalUrl(
                        context, 'https://www.meta.com/experiences/$appId'),
                  ),
              ],
            ),
            const SizedBox(height: 8),
            if (reviews.isNotEmpty) ...[
              ListView.separated(
                shrinkWrap: true,
                physics: const NeverScrollableScrollPhysics(),
                itemCount: reviews.length,
                itemBuilder: (context, index) => _ReviewTile(
                    review: reviews[index], fallbackAuthor: reviewFallback),
                separatorBuilder: (_, __) => const SizedBox(height: 12),
              ),
              const SizedBox(height: 8),
            ],
            _buildReviewsPager(context),
          ],
        );
      }
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          l10n.detailsReviewsTitle,
          style: textTheme.titleMedium,
        ),
        const SizedBox(height: 8),
        body,
      ],
    );
  }

  // more state helpers
  void _fetchReviews() {
    final appId = _currentReviewsAppId;
    if (appId == null) return;
    final sortBy = _sort == _ReviewSort.helpful ? 'helpful' : 'newest';
    final offset = _pageIndex * _pageSize;
    GetAppReviewsRequest(
      appId: appId,
      limit: _pageSize,
      offset: offset,
      sortBy: sortBy,
    ).sendSignalToRust();
  }

  Widget _buildReviewsPager(BuildContext context) {
    final total = _reviewsTotal ?? 0;
    if (total <= _pageSize) return const SizedBox.shrink();
    final totalPages = (total / _pageSize).ceil();
    final pageNo = _pageIndex + 1;

    List<int?> pageModel;
    if (totalPages <= 7) {
      pageModel = List<int?>.generate(totalPages, (i) => i + 1);
    } else if (pageNo <= 4) {
      pageModel = [1, 2, 3, 4, 5, null, totalPages];
    } else if (pageNo >= totalPages - 3) {
      pageModel = [
        1,
        null,
        totalPages - 4,
        totalPages - 3,
        totalPages - 2,
        totalPages - 1,
        totalPages
      ];
    } else {
      pageModel = [1, null, pageNo - 1, pageNo, pageNo + 1, null, totalPages];
    }

    final buttons = <Widget>[];
    buttons.add(IconButton(
      tooltip: AppLocalizations.of(context).previous,
      icon: const Icon(Icons.chevron_left),
      onPressed: _pageIndex > 0
          ? () {
              setState(() {
                _pageIndex -= 1;
                _reviewsLoading = true;
              });
              _fetchReviews();
            }
          : null,
    ));

    for (final entry in pageModel) {
      if (entry == null) {
        buttons.add(const Padding(
          padding: EdgeInsets.symmetric(horizontal: 4),
          child: Text('…'),
        ));
      } else {
        final isCurrent = entry == pageNo;
        final btn = isCurrent
            ? FilledButton.tonal(
                onPressed: null,
                child: Text('$entry'),
              )
            : TextButton(
                onPressed: () {
                  setState(() {
                    _pageIndex = entry - 1;
                    _reviewsLoading = true;
                  });
                  _fetchReviews();
                },
                child: Text('$entry'),
              );
        buttons.add(Padding(
            padding: const EdgeInsets.symmetric(horizontal: 2), child: btn));
      }
    }

    buttons.add(IconButton(
      tooltip: AppLocalizations.of(context).next,
      icon: const Icon(Icons.chevron_right),
      onPressed: pageNo < totalPages
          ? () {
              setState(() {
                _pageIndex += 1;
                _reviewsLoading = true;
              });
              _fetchReviews();
            }
          : null,
    ));

    return Row(children: buttons);
  }
}

enum _ReviewSort { helpful, newest }

enum _MediaKind { video, image, other }

class _DescPart {
  final String? text;
  final String? url;
  final bool isVideo;
  final bool isImage;

  _DescPart._(this.text, this.url, this.isVideo, this.isImage);

  factory _DescPart.text(String t) => _DescPart._(t, null, false, false);
  factory _DescPart.video(String u) => _DescPart._(null, u, true, false);
  factory _DescPart.image(String u) => _DescPart._(null, u, false, true);
}

class _InlineNetworkImage extends StatelessWidget {
  const _InlineNetworkImage({required this.url, this.onTap});

  final String url;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final borderRadius = BorderRadius.circular(6);
    final placeholder = Container(
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: borderRadius,
      ),
      height: 180,
      child: const Center(child: CircularProgressIndicator(strokeWidth: 2)),
    );

    final error = Container(
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: borderRadius,
      ),
      height: 180,
      child: const Center(child: Icon(Icons.broken_image_outlined, size: 40)),
    );

    final img = CachedNetworkImage(
      imageUrl: url,
      placeholder: (_, __) => placeholder,
      errorWidget: (_, __, ___) => error,
      imageBuilder: (context, provider) => ClipRRect(
        borderRadius: borderRadius,
        child: Container(
          constraints: const BoxConstraints(maxHeight: 360),
          color: Colors.black12,
          child: GestureDetector(
            onTap: onTap,
            child: Image(
              image: provider,
              fit: BoxFit.contain,
              width: double.infinity,
            ),
          ),
        ),
      ),
    );

    return MouseRegion(
      cursor: onTap != null ? SystemMouseCursors.click : MouseCursor.defer,
      child: img,
    );
  }
}

class _ReviewTile extends StatelessWidget {
  const _ReviewTile({required this.review, this.fallbackAuthor});

  final AppReview review;
  final String? fallbackAuthor;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;
    final authorName = review.authorDisplayName?.trim();
    final authorAlias = review.authorAlias?.trim();

    String? aliasPretty;
    if (authorAlias != null && authorAlias.isNotEmpty) {
      aliasPretty = authorAlias.startsWith('@') ? authorAlias : '@$authorAlias';
    }

    String? authorCombined;
    if ((authorName != null && authorName.isNotEmpty) &&
        (aliasPretty != null)) {
      authorCombined = '$authorName ($aliasPretty)';
    } else if (authorName != null && authorName.isNotEmpty) {
      authorCombined = authorName;
    } else if (aliasPretty != null) {
      authorCombined = aliasPretty;
    }

    final title = (review.reviewTitle?.trim().isNotEmpty == true)
        ? review.reviewTitle!.trim()
        : (authorCombined ?? fallbackAuthor ?? '');

    final rawDate = review.date;
    final parsedDate = rawDate != null ? DateTime.tryParse(rawDate) : null;
    final subtitleParts = <String>[
      if (review.reviewTitle != null && (authorCombined != null))
        authorCombined,
      if (parsedDate != null)
        DateFormat.yMMMd().add_jm().format(parsedDate.toLocal()),
    ];

    final subtitle = subtitleParts.join(' • ');
    final description = review.reviewDescription?.trim().isEmpty ?? true
        ? null
        : review.reviewDescription!.replaceAll('\r\n', '\n');

    final score = review.score;
    final scoreText = score == null
        ? null
        : (score % 1 == 0
            ? score.toInt().toString()
            : score.toStringAsFixed(1));

    final helpful = review.reviewHelpfulCount;
    final devResp = review.developerResponse;

    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border.all(color: theme.colorScheme.outlineVariant),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                if (scoreText != null)
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const Icon(Icons.star_rate_rounded,
                          size: 16, color: Colors.amber),
                      const SizedBox(width: 4),
                      Text('$scoreText/5', style: textTheme.bodyMedium),
                      const SizedBox(width: 12),
                    ],
                  ),
                Expanded(
                  child: Text(
                    title,
                    style: textTheme.titleSmall,
                  ),
                ),
              ],
            ),
            if (subtitle.isNotEmpty) ...[
              const SizedBox(height: 6),
              Text(
                subtitle,
                style: textTheme.bodySmall?.copyWith(
                  color: textTheme.bodySmall?.color?.withValues(alpha: 0.7),
                ),
              ),
            ],
            if (description != null) ...[
              const SizedBox(height: 8),
              Text(
                description,
                style: textTheme.bodyMedium,
              ),
            ],
            if (helpful != null && helpful > 0) ...[
              const SizedBox(height: 8),
              Text(
                l10n.detailsReviewHelpfulCount(helpful),
                style:
                    textTheme.bodySmall?.copyWith(fontStyle: FontStyle.italic),
              ),
            ],
            if (devResp != null) ...[
              const SizedBox(height: 8),
              Theme(
                data: theme.copyWith(dividerColor: Colors.transparent),
                child: ExpansionTile(
                  tilePadding: EdgeInsets.zero,
                  childrenPadding: EdgeInsets.zero,
                  initiallyExpanded: false,
                  title: Row(
                    children: [
                      const Icon(Icons.reply, size: 18),
                      const SizedBox(width: 6),
                      Text(l10n.detailsDeveloperResponse,
                          style: textTheme.titleSmall),
                    ],
                  ),
                  children: [
                    Align(
                      alignment: Alignment.centerLeft,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            devResp.body,
                            style: textTheme.bodyMedium,
                          ),
                          if (devResp.date != null) ...[
                            const SizedBox(height: 6),
                            Text(
                              _formatEpochSeconds(context, devResp.date!),
                              style: textTheme.bodySmall?.copyWith(
                                color: textTheme.bodySmall?.color
                                    ?.withValues(alpha: 0.7),
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

String _formatEpochSeconds(BuildContext context, int seconds) {
  try {
    final dt = DateTime.fromMillisecondsSinceEpoch(seconds * 1000, isUtc: true)
        .toLocal();
    return formatDateTime(context, dt) ?? '';
  } catch (_) {
    return '';
  }
}

class _CloudAppMedia extends StatefulWidget {
  const _CloudAppMedia({
    required this.truePackageName,
    required this.width,
    required this.height,
  });

  final String truePackageName;
  final double width;
  final double height;

  @override
  State<_CloudAppMedia> createState() => _CloudAppMediaState();
}

class _CloudAppMediaState extends State<_CloudAppMedia> {
  bool _hovered = false;
  bool _playing = false;
  bool _initializingVideo = false;
  VideoPlayerController? _controller;
  CacheManager? _cacheManager;
  String? _cacheDirPath;
  bool _checkingTrailer = false;
  bool _trailerAvailable = false;
  bool _muted = true;
  String? _checkedUrl;

  @override
  void initState() {
    super.initState();
    // Cache manager initialized when Rust provides cache path via provider
  }

  void _ensureCacheManager(String path) {
    if (_cacheDirPath == path && _cacheManager != null) return;
    final cacheDir = Directory(path);
    final cacheInfoFile = File('$path${Platform.pathSeparator}cache_info.json');
    if (!cacheDir.existsSync()) {
      cacheDir.createSync(recursive: true);
    }
    _cacheDirPath = path;
    _cacheManager = CacheManager(
      Config(
        'app_media_cache',
        stalePeriod: const Duration(days: 30),
        maxNrOfCacheObjects: 200,
        repo: JsonCacheInfoRepository.withFile(cacheInfoFile),
        fileSystem: IOFileSystem(path),
      ),
    );
  }

  Future<void> _startVideo(String url) async {
    if (_initializingVideo) return;
    setState(() {
      _initializingVideo = true;
    });
    final controller = VideoPlayerController.networkUrl(Uri.parse(url));
    try {
      await controller.initialize();
      await controller.setVolume(_muted ? 0.0 : 1.0);
      await controller.play();
      if (!mounted) return;
      setState(() {
        _controller = controller;
        _playing = true;
      });
    } catch (_) {
      // On failure, revert to thumbnail state.
      await controller.dispose();
      if (!mounted) return;
      setState(() {
        _controller = null;
        _playing = false;
      });
    } finally {
      if (mounted) {
        setState(() {
          _initializingVideo = false;
        });
      }
    }
  }

  Future<void> _stopVideo() async {
    final c = _controller;
    if (c != null) {
      // await c.pause();
      await c.dispose();
    }
    if (!mounted) return;
    setState(() {
      _controller = null;
      _playing = false;
    });
  }

  @override
  void dispose() {
    _controller?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final media = context.watch<CloudAppsState>();
    final thumbUrl = media.thumbnailUrlFor(widget.truePackageName);
    final trailerUrl = media.trailerUrlFor(widget.truePackageName);

    final borderRadius = BorderRadius.circular(8);

    final cachePath = media.mediaCacheDir;
    final canLoadMedia = cachePath != null;
    if (canLoadMedia) {
      _ensureCacheManager(cachePath);
    }
    if (_checkedUrl != trailerUrl && !_checkingTrailer) {
      _checkTrailerAvailability(trailerUrl);
    }

    Widget child;
    if (_playing && _controller != null && _controller!.value.isInitialized) {
      child = ClipRRect(
        borderRadius: borderRadius,
        child: Stack(
          children: [
            Positioned.fill(
              child: FittedBox(
                fit: BoxFit.cover,
                child: SizedBox(
                  width: _controller!.value.size.width,
                  height: _controller!.value.size.height,
                  child: VideoPlayer(_controller!),
                ),
              ),
            ),
            // Controls overlay: Pause/Play, Sound, Close
            Positioned(
              top: 8,
              right: 8,
              child: Container(
                decoration: BoxDecoration(
                  color: Colors.black.withValues(alpha: 0.5),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    IconButton(
                      tooltip: l10n.pause,
                      icon: Icon(
                        _controller!.value.isPlaying
                            ? Icons.pause
                            : Icons.play_arrow,
                      ),
                      color: Colors.white,
                      onPressed: () async {
                        if (_controller!.value.isPlaying) {
                          await _controller!.pause();
                        } else {
                          await _controller!.play();
                        }
                        if (mounted) setState(() {});
                      },
                    ),
                    IconButton(
                      tooltip: _muted ? l10n.unmute : l10n.mute,
                      icon: Icon(_muted ? Icons.volume_off : Icons.volume_up),
                      color: Colors.white,
                      onPressed: () async {
                        _muted = !_muted;
                        await _controller!.setVolume(_muted ? 0.0 : 1.0);
                        if (mounted) setState(() {});
                      },
                    ),
                    IconButton(
                      tooltip: l10n.close,
                      icon: const Icon(Icons.close),
                      color: Colors.white,
                      onPressed: _stopVideo,
                    ),
                  ],
                ),
              ),
            ),
            // Seekbar at bottom
            Positioned(
              left: 0,
              right: 0,
              bottom: 0,
              child: _VideoSeekbar(controller: _controller!),
            ),
          ],
        ),
      );
    } else {
      // Thumbnail with hover overlay
      child = MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          onTap: (!_trailerAvailable || _initializingVideo)
              ? null
              : () {
                  _startVideo(trailerUrl);
                },
          child: ClipRRect(
            borderRadius: borderRadius,
            child: Stack(
              fit: StackFit.expand,
              children: [
                CachedNetworkImage(
                  imageUrl: thumbUrl,
                  cacheManager: _cacheManager,
                  fit: BoxFit.cover,
                  placeholder: (context, url) =>
                      _mediaPlaceholder(context, loading: true),
                  errorWidget: (context, url, error) =>
                      _mediaPlaceholder(context),
                ),
                // Small availability indicator badge
                Positioned(
                  left: 8,
                  top: 8,
                  child: Tooltip(
                    message: _checkingTrailer
                        ? l10n.checkingTrailerAvailability
                        : (_trailerAvailable
                            ? l10n.trailerAvailable
                            : l10n.noTrailer),
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: Colors.black.withValues(alpha: 0.5),
                        borderRadius: BorderRadius.circular(14),
                      ),
                      child: Padding(
                        padding: const EdgeInsets.all(6.0),
                        child: _checkingTrailer
                            ? const SizedBox(
                                width: 21,
                                height: 21,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                  valueColor: AlwaysStoppedAnimation<Color>(
                                      Colors.white),
                                ),
                              )
                            : Icon(
                                _trailerAvailable
                                    ? Icons.play_circle_outline
                                    : Icons.videocam_off,
                                color: Colors.white,
                                size: 24,
                              ),
                      ),
                    ),
                  ),
                ),
                // Loading indicator when starting video
                if (_initializingVideo)
                  Positioned.fill(
                    child: Container(
                      color: Colors.black.withValues(alpha: 0.25),
                      child: const Center(
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                    ),
                  ),
                AnimatedOpacity(
                  opacity: _hovered && _trailerAvailable && !_initializingVideo
                      ? 1.0
                      : 0.0,
                  duration: const Duration(milliseconds: 150),
                  child: IgnorePointer(
                    ignoring:
                        !(_hovered && _trailerAvailable && !_initializingVideo),
                    child: Container(
                      color: Colors.black.withValues(alpha: 0.45),
                      child: Center(
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: const [
                            Icon(
                              Icons.play_circle_fill,
                              size: 64,
                              color: Colors.white,
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    }

    return Container(
      width: widget.width,
      height: widget.height,
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: borderRadius,
      ),
      clipBehavior: Clip.antiAlias,
      child: child,
    );
  }

  Future<bool> _urlExists(String url) async {
    try {
      final uri = Uri.parse(url);
      final head = await http.head(uri);
      if (head.statusCode == 200) return true;
      if (head.statusCode == 405 || head.statusCode == 501) {
        final get = await http.get(uri, headers: const {'range': 'bytes=0-0'});
        return get.statusCode == 200 || get.statusCode == 206;
      }
      return false;
    } catch (_) {
      return false;
    }
  }

  Future<void> _checkTrailerAvailability(String trailerUrl) async {
    if (_checkingTrailer) return;
    setState(() => _checkingTrailer = true);
    final available = await _urlExists(trailerUrl);
    if (!mounted) return;
    setState(() {
      _trailerAvailable = available;
      _checkingTrailer = false;
      _checkedUrl = trailerUrl;
    });
  }

  Widget _mediaPlaceholder(BuildContext context, {bool loading = false}) {
    final color = Theme.of(context).colorScheme.surfaceContainerHighest;
    return Container(
      color: color,
      child: Center(
        child: loading
            ? const CircularProgressIndicator(strokeWidth: 2)
            : const Icon(Icons.folder_off_outlined, size: 48),
      ),
    );
  }
}

class _VideoSeekbar extends StatefulWidget {
  const _VideoSeekbar({required this.controller});

  final VideoPlayerController controller;

  @override
  State<_VideoSeekbar> createState() => _VideoSeekbarState();
}

class _VideoSeekbarState extends State<_VideoSeekbar> {
  bool _dragging = false;
  double _dragValue = 0.0;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onVideoUpdate);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onVideoUpdate);
    super.dispose();
  }

  void _onVideoUpdate() {
    if (!_dragging && mounted) {
      setState(() {});
    }
  }

  String _formatDuration(Duration d) {
    final minutes = d.inMinutes.remainder(60).toString().padLeft(2, '0');
    final seconds = d.inSeconds.remainder(60).toString().padLeft(2, '0');
    if (d.inHours > 0) {
      return '${d.inHours}:$minutes:$seconds';
    }
    return '$minutes:$seconds';
  }

  @override
  Widget build(BuildContext context) {
    final value = widget.controller.value;
    final duration = value.duration;
    final position = value.position;

    final totalMs = duration.inMilliseconds.toDouble();
    final positionMs = position.inMilliseconds.toDouble();
    final progress = totalMs > 0 ? (positionMs / totalMs).clamp(0.0, 1.0) : 0.0;

    return Container(
      decoration: BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            Colors.transparent,
            Colors.black.withValues(alpha: 0.7),
          ],
        ),
      ),
      padding: const EdgeInsets.only(left: 12, right: 12, top: 16, bottom: 8),
      child: Row(
        children: [
          Text(
            _formatDuration(position),
            style: const TextStyle(color: Colors.white, fontSize: 12),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: SliderTheme(
              data: SliderTheme.of(context).copyWith(
                trackHeight: 4,
                thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 6),
                overlayShape: const RoundSliderOverlayShape(overlayRadius: 12),
                activeTrackColor: Colors.white,
                inactiveTrackColor: Colors.white.withValues(alpha: 0.3),
                thumbColor: Colors.white,
                overlayColor: Colors.white.withValues(alpha: 0.2),
              ),
              child: Slider(
                value: _dragging ? _dragValue : progress,
                onChangeStart: (v) {
                  _dragging = true;
                  _dragValue = v;
                },
                onChanged: (v) {
                  setState(() {
                    _dragValue = v;
                  });
                },
                onChangeEnd: (v) async {
                  _dragging = false;
                  final newPosition = Duration(
                    milliseconds: (v * totalMs).round(),
                  );
                  await widget.controller.seekTo(newPosition);
                },
              ),
            ),
          ),
          const SizedBox(width: 8),
          Text(
            _formatDuration(duration),
            style: const TextStyle(color: Colors.white, fontSize: 12),
          ),
        ],
      ),
    );
  }
}
