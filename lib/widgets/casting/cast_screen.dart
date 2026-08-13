import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:media_kit/media_kit.dart';
import 'package:media_kit_video/media_kit_video.dart';
import 'package:provider/provider.dart';

import '../../providers/casting_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

/// Full-screen native casting view with an embedded low-latency player.
///
/// The player connects to the local Matroska HTTP stream served by the Rust
/// core. Closing this view stops the casting session.
class CastScreen extends StatefulWidget {
  const CastScreen({super.key});

  static Future<void> open(BuildContext context) {
    return Navigator.of(context, rootNavigator: true).push(
      MaterialPageRoute<void>(
        builder: (_) => const CastScreen(),
        fullscreenDialog: true,
      ),
    );
  }

  @override
  State<CastScreen> createState() => _CastScreenState();
}

class _CastScreenState extends State<CastScreen> {
  Player? _player;
  VideoController? _videoController;
  String? _openedUrl;
  CastingState? _casting;
  StreamSubscription<String>? _errorSubscription;

  /// Whether the session was observed leaving the idle state at least once.
  /// The route is pushed while the start request is still in flight, so an
  /// initial idle state must not close it.
  bool _sessionSeen = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _casting = context.read<CastingState>()..addListener(_onCastingChanged);
      _onCastingChanged();
    });
  }

  void _onCastingChanged() {
    final casting = _casting;
    if (casting == null || !mounted) return;
    if (casting.state == NativeCastingState.idle) {
      // Session ended or failed; the backend surfaces errors via toasts.
      if (_sessionSeen || casting.error != null) {
        Navigator.of(context).maybePop();
      }
      return;
    }
    _sessionSeen = true;
    final url = casting.url;
    if (url != null && _openedUrl != url) {
      _openedUrl = url;
      _openPlayer(url);
    }
  }

  Future<void> _openPlayer(String url) async {
    final player = Player();
    // Low-latency live playback options, mirroring the reference casting tool.
    final platform = player.platform;
    if (platform is NativePlayer) {
      await platform.command(const ['apply-profile', 'low-latency']);
      await platform.setProperty('demuxer-readahead-secs', '0');
      await platform.setProperty('demuxer-lavf-format', 'matroska');
      await platform.setProperty('correct-pts', 'yes');
      await platform.setProperty('framedrop', 'vo');
      await platform.setProperty('video-sync', 'audio');
      await platform.setProperty('interpolation', 'no');
      await platform.setProperty('audio-buffer', '0.04');
      await platform.setProperty('cache', 'no');
      // Give us time to connect (and sometimes reconnect).
      await platform.setProperty('network-timeout', '300');
    }
    _errorSubscription = player.stream.error.listen((error) {
      debugPrint('Cast player error: $error');
    });
    if (!mounted) {
      await player.dispose();
      return;
    }
    setState(() {
      _player = player;
      _videoController = VideoController(player);
    });
    await player.open(Media(url));
  }

  void _close() {
    final casting = context.read<CastingState>();
    Navigator.of(context).pop();
    casting.stopCasting();
  }

  @override
  void dispose() {
    _casting?.removeListener(_onCastingChanged);
    _errorSubscription?.cancel();
    _player?.dispose();
    // Ensure the session stops when the view is dismissed by any means.
    _casting?.stopCasting();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final casting = context.watch<CastingState>();
    final streaming = casting.state == NativeCastingState.streaming;
    final videoController = _videoController;

    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.escape): _close},
      child: Focus(
        autofocus: true,
        child: Scaffold(
          backgroundColor: Colors.black,
          body: Stack(
            fit: StackFit.expand,
            children: [
              if (videoController != null)
                Video(
                  controller: videoController,
                  controls: NoVideoControls,
                  fit: BoxFit.contain,
                ),
              if (!streaming)
                Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const CircularProgressIndicator(),
                      const SizedBox(height: 16),
                      Text(
                        casting.state == NativeCastingState.reconnecting
                            ? l10n.castReconnecting
                            : l10n.castConnecting,
                        style: const TextStyle(color: Colors.white70),
                      ),
                    ],
                  ),
                ),
              if (streaming && casting.stats != null)
                Positioned(
                  left: 12,
                  top: 12,
                  child: _StatsOverlay(stats: casting.stats!),
                ),
              Positioned(
                right: 8,
                top: 8,
                child: IconButton(
                  icon: const Icon(Icons.close),
                  color: Colors.white,
                  tooltip: l10n.commonClose,
                  onPressed: _close,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _StatsOverlay extends StatelessWidget {
  final NativeCastingStats stats;

  const _StatsOverlay({required this.stats});

  @override
  Widget build(BuildContext context) {
    final fps = stats.fps.toStringAsFixed(0);
    final buffer = stats.bufferAgeMs.toStringAsFixed(0);
    final latency = stats.latencyMs.toStringAsFixed(0);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: Colors.black54,
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(
        '$fps fps · buf $buffer ms · lat $latency ms',
        style: const TextStyle(
          color: Colors.white70,
          fontSize: 12,
          fontFamily: 'monospace',
        ),
      ),
    );
  }
}
