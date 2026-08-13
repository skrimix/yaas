import 'dart:async';

import 'package:flutter/material.dart';
import 'package:rinf/rinf.dart';

import '../src/bindings/bindings.dart';

/// Tracks the native casting session state and live stats from the Rust core.
class CastingState extends ChangeNotifier {
  NativeCastingState _state = NativeCastingState.idle;
  String? _url;
  String? _error;
  NativeCastingStats? _stats;

  late final StreamSubscription<RustSignalPack<NativeCastingStateChanged>>
      _stateSubscription;
  late final StreamSubscription<RustSignalPack<NativeCastingStats>>
      _statsSubscription;

  CastingState() {
    _stateSubscription =
        NativeCastingStateChanged.rustSignalStream.listen((event) {
      final message = event.message;
      _state = message.state;
      _url = message.url;
      _error = message.error;
      if (_state != NativeCastingState.streaming) {
        _stats = null;
      }
      notifyListeners();
    });
    _statsSubscription = NativeCastingStats.rustSignalStream.listen((event) {
      _stats = event.message;
      notifyListeners();
    });

    // Resync with the Rust core in case a session is already running.
    const GetNativeCastingStateRequest().sendSignalToRust();
  }

  @override
  void dispose() {
    _stateSubscription.cancel();
    _statsSubscription.cancel();
    super.dispose();
  }

  NativeCastingState get state => _state;

  /// URL of the live Matroska HTTP stream; set once the session has started.
  String? get url => _url;
  String? get error => _error;
  NativeCastingStats? get stats => _stats;

  bool get isActive => _state != NativeCastingState.idle;

  void startCasting() {
    if (isActive) return;
    const StartNativeCastingRequest(audio: true, fps: 60).sendSignalToRust();
  }

  void stopCasting() {
    if (!isActive) return;
    const StopNativeCastingRequest().sendSignalToRust();
  }
}
