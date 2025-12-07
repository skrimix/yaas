import 'package:flutter/material.dart';

/// A widget that shows a context menu on right-click or long-press.
class ContextMenuRegion extends StatelessWidget {
  const ContextMenuRegion({
    super.key,
    required this.menuItems,
    required this.child,
    this.onPrimaryTap,
  });

  /// Builder function that creates menu items on demand.
  ///
  /// Each [PopupMenuItem] should have a [VoidCallback] as its value,
  /// which will be executed when the item is selected.
  final List<PopupMenuEntry<VoidCallback>> Function(BuildContext context)
      menuItems;

  final Widget child;
  final VoidCallback? onPrimaryTap;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onSecondaryTapUp: (details) => _showMenu(context, details.globalPosition),
      onLongPressStart: (details) => _showMenu(context, details.globalPosition),
      onTap: onPrimaryTap,
      child: child,
    );
  }

  Future<void> _showMenu(BuildContext context, Offset position) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final result = await showMenu<VoidCallback>(
      context: context,
      position: RelativeRect.fromRect(
        Rect.fromLTWH(position.dx, position.dy, 0, 0),
        Offset.zero & overlay.size,
      ),
      items: menuItems(context),
    );
    result?.call();
  }
}
