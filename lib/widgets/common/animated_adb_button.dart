import 'package:flutter/material.dart';
import '../../src/bindings/bindings.dart';

class AnimatedAdbButton extends StatefulWidget {
  final IconData icon;
  final String tooltip;
  final AdbCommandType commandType;
  // Identifier to correlate completion events (was: packageName)
  final String commandKey;
  final VoidCallback onPressed;
  final Color? iconColor;

  const AnimatedAdbButton({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.commandType,
    required this.commandKey,
    required this.onPressed,
    this.iconColor,
  });

  @override
  State<AnimatedAdbButton> createState() => _AnimatedAdbButtonState();
}

class _AnimatedAdbButtonState extends State<AnimatedAdbButton>
    with TickerProviderStateMixin {
  late AnimationController _controller;
  late Animation<double> _scale;

  bool _isProcessing = false;
  bool _showSuccess = false;

  @override
  void initState() {
    super.initState();

    _controller = AnimationController(
      duration: const Duration(milliseconds: 300),
      vsync: this,
    );
    _scale = Tween<double>(begin: 1.0, end: 1.2).animate(
      CurvedAnimation(parent: _controller, curve: Curves.elasticOut),
    );

    AdbCommandCompletedEvent.rustSignalStream.listen((event) {
      final signal = event.message;
      if (signal.commandType == widget.commandType &&
          signal.commandKey == widget.commandKey) {
        _handleCommandCompleted(signal.success);
      }
    });
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _handleCommandCompleted(bool success) {
    if (!mounted) return;

    setState(() {
      _isProcessing = false;
      _showSuccess = success;
    });

    if (success) {
      _controller.forward().then((_) {
        Future.delayed(const Duration(milliseconds: 300), () {
          if (mounted) {
            _controller.reverse().then((_) {
              if (mounted) {
                setState(() {
                  _showSuccess = false;
                });
              }
            });
          }
        });
      });
    }
  }

  void _onPressed() {
    if (_isProcessing || _showSuccess) return;

    setState(() {
      _isProcessing = true;
    });

    widget.onPressed();

    // Fallback: stop processing after 10 seconds
    Future.delayed(const Duration(seconds: 10), () {
      if (_isProcessing && mounted) {
        setState(() {
          _isProcessing = false;
        });
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return ScaleTransition(
      scale: _scale,
      child: IconButton(
        icon: AnimatedSwitcher(
          duration: const Duration(milliseconds: 200),
          child: _showSuccess
              ? Icon(
                  Icons.check,
                  key: const Key('success'),
                  color: Colors.green,
                )
              : _isProcessing
                  ? SizedBox(
                      key: const Key('loading'),
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        valueColor: AlwaysStoppedAnimation<Color>(
                          Theme.of(context).colorScheme.primary,
                        ),
                      ),
                    )
                  : Icon(
                      widget.icon,
                      key: const Key('idle'),
                      color: widget.iconColor,
                    ),
        ),
        tooltip: widget.tooltip,
        onPressed: _onPressed,
      ),
    );
  }
}
