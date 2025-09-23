import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:cached_network_image/cached_network_image.dart';
import 'package:video_player/video_player.dart';
import 'package:flutter_cache_manager/flutter_cache_manager.dart';
import 'package:http/http.dart' as http;
import 'package:intl/intl.dart';

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
  final void Function(String fullName) onDownload;
  final void Function(String fullName) onInstall;

  @override
  State<CloudAppDetailsDialog> createState() => _CloudAppDetailsDialogState();
}

class _CloudAppDetailsDialogState extends State<CloudAppDetailsDialog> {
  StreamSubscription<RustSignalPack<AppDetailsResponse>>? _sub;
  StreamSubscription<RustSignalPack<AppReviewsResponse>>? _reviewsSub;
  AppDetailsResponse? _details;
  bool _loading = true;
  final ScrollController _descScrollController = ScrollController();
  List<VrdbReview>? _reviews;
  bool _reviewsLoading = false;
  String? _reviewsError;
  String? _currentReviewsAppId;

  @override
  void initState() {
    super.initState();
    _sub = AppDetailsResponse.rustSignalStream.listen((event) {
      final message = event.message;
      if (message.packageName != widget.cachedApp.app.packageName) {
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
          shouldFetchReviews = true;
        }
      });

      if (shouldFetchReviews && newAppId != null) {
        GetAppReviewsRequest(appId: newAppId).sendSignalToRust();
      }
    });

    _reviewsSub = AppReviewsResponse.rustSignalStream.listen((event) {
      final message = event.message;
      if (message.appId != _currentReviewsAppId) {
        return;
      }

      setState(() {
        _reviews = message.reviews;
        _reviewsError = message.error;
        _reviewsLoading = false;
      });
    });

    GetAppDetailsRequest(packageName: widget.cachedApp.app.packageName)
        .sendSignalToRust();
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
                            packageName: widget.cachedApp.app.packageName,
                            width: 450,
                            height: 270,
                          ),
                          const SizedBox(width: 16),
                          // Right: Details + description
                          Expanded(
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
      final reviews = _reviews ?? const <VrdbReview>[];
      final reviewFallback =
          details.displayName ?? widget.cachedApp.app.appName;
      if (reviews.isEmpty) {
        body = Text(
          l10n.detailsReviewsEmpty,
          style: textTheme.bodySmall,
        );
      } else {
        // TODO: Add pagination for additional review pages.
        body = ListView.separated(
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          itemCount: reviews.length,
          itemBuilder: (context, index) => _ReviewTile(
              review: reviews[index], fallbackAuthor: reviewFallback),
          separatorBuilder: (_, __) => const SizedBox(height: 12),
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
}

class _ReviewTile extends StatelessWidget {
  const _ReviewTile({required this.review, this.fallbackAuthor});

  final VrdbReview review;
  final String? fallbackAuthor;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;
    final authorName = review.authorDisplayName?.trim();
    final title = (review.title?.trim().isEmpty ?? true)
        ? (authorName?.isNotEmpty == true ? authorName! : fallbackAuthor ?? '')
        : review.title!.trim();

    final rawDate = review.date;
    final parsedDate = rawDate != null ? DateTime.tryParse(rawDate) : null;
    final subtitleParts = <String>[
      if (review.title != null && authorName?.isNotEmpty == true) authorName!,
      if (parsedDate != null)
        DateFormat.yMMMd().add_jm().format(parsedDate.toLocal()),
    ];

    final subtitle = subtitleParts.join(' • ');
    final description = review.description?.trim().isEmpty ?? true
        ? null
        : review.description!.replaceAll('\r\n', '\n');

    final score = review.score;
    final scoreText = score == null
        ? null
        : (score % 1 == 0
            ? score.toInt().toString()
            : score.toStringAsFixed(1));

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
          ],
        ),
      ),
    );
  }
}

class _CloudAppMedia extends StatefulWidget {
  const _CloudAppMedia({
    required this.packageName,
    required this.width,
    required this.height,
  });

  final String packageName;
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
    final thumbUrl = media.thumbnailUrlFor(widget.packageName);
    final trailerUrl = media.trailerUrlFor(widget.packageName);

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
                // FIXME: this should disappear after click (when video is loading)
                AnimatedOpacity(
                  opacity: _hovered && _trailerAvailable ? 1.0 : 0.0,
                  duration: const Duration(milliseconds: 150),
                  child: IgnorePointer(
                    ignoring: !(_hovered && _trailerAvailable),
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
