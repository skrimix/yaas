import 'package:flutter/material.dart';

import '../../utils/theme_utils.dart';

enum StatusTone { neutral, primary, secondary, success, warning, error }

/// Compact tonal label for item states such as install or task status.
class StatusChip extends StatelessWidget {
  const StatusChip({
    super.key,
    required this.label,
    this.icon,
    this.tone = StatusTone.neutral,
    this.tooltip,
    this.onTap,
  });

  final String label;
  final IconData? icon;
  final StatusTone tone;
  final String? tooltip;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final status = StatusColors.of(context);
    final (background, foreground) = switch (tone) {
      StatusTone.neutral => (
          scheme.surfaceContainerHighest,
          scheme.onSurfaceVariant
        ),
      StatusTone.primary => (
          scheme.primaryContainer,
          scheme.onPrimaryContainer
        ),
      StatusTone.secondary => (
          scheme.secondaryContainer,
          scheme.onSecondaryContainer
        ),
      StatusTone.success => (
          status.successContainer,
          status.onSuccessContainer
        ),
      StatusTone.warning => (
          status.warningContainer,
          status.onWarningContainer
        ),
      StatusTone.error => (scheme.errorContainer, scheme.onErrorContainer),
    };

    Widget chip = Material(
      color: background,
      shape: const StadiumBorder(),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                Icon(icon, size: 14, color: foreground),
                const SizedBox(width: 4),
              ],
              Text(
                label,
                style: theme.textTheme.labelSmall?.copyWith(
                  color: foreground,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
        ),
      ),
    );

    if (tooltip != null) {
      chip = Tooltip(
        message: tooltip,
        waitDuration: const Duration(milliseconds: 300),
        child: chip,
      );
    }
    return chip;
  }
}
