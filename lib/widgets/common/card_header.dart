import 'package:flutter/material.dart';

/// Card title with a tonal leading icon.
///
/// Use it as the first child of a card so sections look consistent across
/// screens.
class CardHeader extends StatelessWidget {
  const CardHeader(
      {super.key, required this.icon, required this.title, this.trailing});

  final IconData icon;
  final String title;

  /// Optional action shown at the end of the header row.
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      children: [
        Container(
          width: 36,
          height: 36,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: theme.colorScheme.secondaryContainer,
            shape: BoxShape.circle,
          ),
          child: Icon(icon,
              size: 20, color: theme.colorScheme.onSecondaryContainer),
        ),
        const SizedBox(width: 12),
        Expanded(child: Text(title, style: theme.textTheme.titleMedium)),
        if (trailing != null) trailing!,
      ],
    );
  }
}
