import 'dart:async';

import 'package:flutter/material.dart';
import 'package:rinf/rinf.dart';

import '../../src/bindings/bindings.dart';

/// An animated refresh button that shows a spinning icon while refreshing
/// and a checkmark when the device state updates.
class AnimatedRefreshButton extends StatefulWidget {
  final String tooltip;
  final AdbCommand command;
  final AdbCommandKind commandType;
  final String commandKey;
  final double size;
  final double iconSize;
  final Stream<RustSignalPack<AdbCommandCompletedEvent>>? completionEvents;
  final ValueChanged<AdbRequest>? requestSender;

  const AnimatedRefreshButton({
    super.key,
    required this.tooltip,
    required this.command,
    required this.commandType,
    required this.commandKey,
    this.size = 23,
    this.iconSize = 16,
    this.completionEvents,
    this.requestSender,
  });

  @override
  State<AnimatedRefreshButton> createState() => _AnimatedRefreshButtonState();
}

class _AnimatedRefreshButtonState extends State<AnimatedRefreshButton>
    with TickerProviderStateMixin {
  late AnimationController _rotationController;
  late AnimationController _successController;
  late Animation<double> _rotation;
  late Animation<double> _checkmarkScale;

  bool _isRefreshing = false;
  bool _showCheckmark = false;
  StreamSubscription<RustSignalPack<AdbCommandCompletedEvent>>? _subscription;
  Timer? _fallbackTimer;
  Timer? _successTimer;

  @override
  void initState() {
    super.initState();

    // Animation for spinning refresh icon
    _rotationController = AnimationController(
      duration: const Duration(milliseconds: 1000),
      vsync: this,
    );
    _rotation = Tween<double>(begin: 0, end: 1).animate(_rotationController);

    // Animation for checkmark appearance
    _successController = AnimationController(
      duration: const Duration(milliseconds: 300),
      vsync: this,
    );
    _checkmarkScale = Tween<double>(begin: 0, end: 1).animate(
      CurvedAnimation(parent: _successController, curve: Curves.elasticOut),
    );

    _subscription =
        (widget.completionEvents ?? AdbCommandCompletedEvent.rustSignalStream)
            .listen((event) {
      final signal = event.message;
      if (signal.commandType == widget.commandType &&
          signal.commandKey == widget.commandKey &&
          _isRefreshing) {
        if (signal.success) {
          _showSuccess();
        } else {
          _stopRefreshing();
        }
      }
    });
  }

  @override
  void dispose() {
    _subscription?.cancel();
    _fallbackTimer?.cancel();
    _successTimer?.cancel();
    _rotationController.dispose();
    _successController.dispose();
    super.dispose();
  }

  void _onRefreshPressed() {
    if (_isRefreshing || _showCheckmark) return;

    setState(() {
      _isRefreshing = true;
      _showCheckmark = false;
    });

    _rotationController.repeat();
    final request =
        AdbRequest(command: widget.command, commandKey: widget.commandKey);
    final sender = widget.requestSender;
    if (sender == null) {
      request.sendSignalToRust();
    } else {
      sender(request);
    }

    // Fallback: stop spinning after 5 seconds
    _fallbackTimer?.cancel();
    _fallbackTimer = Timer(const Duration(seconds: 5), () {
      if (_isRefreshing && mounted) {
        _stopRefreshing();
      }
    });
  }

  void _showSuccess() {
    if (!mounted) return;

    _fallbackTimer?.cancel();
    _rotationController.stop();
    setState(() {
      _isRefreshing = false;
      _showCheckmark = true;
    });

    _successController.forward().then((_) {
      _successTimer?.cancel();
      _successTimer = Timer(const Duration(milliseconds: 800), () {
        if (mounted) {
          _successController.reverse().then((_) {
            if (mounted) {
              setState(() {
                _showCheckmark = false;
              });
            }
          });
        }
      });
    });
  }

  void _stopRefreshing() {
    if (!mounted) return;

    _fallbackTimer?.cancel();
    _rotationController.stop();
    setState(() {
      _isRefreshing = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: SizedBox(
        width: widget.size,
        height: widget.size,
        child: IconButton(
          onPressed: _onRefreshPressed,
          icon: AnimatedSwitcher(
            duration: const Duration(milliseconds: 200),
            child: _showCheckmark
                ? ScaleTransition(
                    key: const Key('checkmark'),
                    scale: _checkmarkScale,
                    child: Icon(
                      Icons.check,
                      color: Colors.green,
                      size: widget.iconSize,
                    ),
                  )
                : _isRefreshing
                    ? RotationTransition(
                        key: const Key('spinning'),
                        turns: _rotation,
                        child: Icon(
                          Icons.refresh,
                          size: widget.iconSize,
                        ),
                      )
                    : Icon(
                        Icons.refresh,
                        key: const Key('idle'),
                        size: widget.iconSize,
                      ),
          ),
          padding: EdgeInsets.zero,
          iconSize: widget.iconSize,
        ),
      ),
    );
  }
}
