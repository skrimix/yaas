import 'package:flutter/material.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart';

/// An animated refresh button that shows a spinning icon while refreshing
/// and a checkmark when the device state updates.
class AnimatedRefreshButton extends StatefulWidget {
  final DeviceState deviceState;
  final String tooltip;
  final double size;
  final double iconSize;

  const AnimatedRefreshButton({
    super.key,
    required this.deviceState,
    required this.tooltip,
    this.size = 23,
    this.iconSize = 16,
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
  DateTime? _lastDeviceUpdate;

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

    // Listen for device updates to trigger success animation
    widget.deviceState.addListener(_onDeviceStateChanged);
    _lastDeviceUpdate = DateTime.now();
  }

  @override
  void dispose() {
    widget.deviceState.removeListener(_onDeviceStateChanged);
    _rotationController.dispose();
    _successController.dispose();
    super.dispose();
  }

  void _onDeviceStateChanged() {
    if (_isRefreshing && widget.deviceState.device != null) {
      final now = DateTime.now();
      // Trigger success animation if device was updated shortly after refresh
      if (_lastDeviceUpdate != null &&
          now.difference(_lastDeviceUpdate!).inSeconds <= 5) {
        _showSuccess();
      }
    }
  }

  void _onRefreshPressed() {
    if (_isRefreshing || _showCheckmark) return;

    setState(() {
      _isRefreshing = true;
      _showCheckmark = false;
    });

    _rotationController.repeat();
    _lastDeviceUpdate = DateTime.now();

    AdbRequest(command: const AdbCommandRefreshDevice(), commandKey: '')
        .sendSignalToRust();

    // Fallback: stop spinning after 5 seconds
    Future.delayed(const Duration(seconds: 5), () {
      if (_isRefreshing && mounted) {
        _stopRefreshing();
      }
    });
  }

  void _showSuccess() {
    if (!mounted) return;

    _rotationController.stop();
    setState(() {
      _isRefreshing = false;
      _showCheckmark = true;
    });

    _successController.forward().then((_) {
      Future.delayed(const Duration(milliseconds: 800), () {
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
