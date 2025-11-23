import 'package:flutter/material.dart';

class ContextMenuRegion extends StatelessWidget {
  const ContextMenuRegion({
    super.key,
    required this.menuChildren,
    required this.child,
    this.onPrimaryTap,
  });

  final List<Widget> menuChildren;
  final Widget child;
  final VoidCallback? onPrimaryTap;

  @override
  Widget build(BuildContext context) {
    return MenuAnchor(
      menuChildren: menuChildren,
      builder: (context, controller, _) {
        return GestureDetector(
          onSecondaryTapUp: (details) {
            controller.open(position: details.localPosition);
          },
          onLongPress: () {
            controller.open();
          },
          onTapUp: (_) {
            onPrimaryTap?.call();
            controller.close();
          },
          child: child,
        );
      },
    );
  }
}
