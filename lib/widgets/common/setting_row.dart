import 'package:flutter/material.dart';

/// A setting label with an optional description and a trailing control.
///
/// Controls move below the label in narrow windows. Use [fullWidth] for paths
/// and [compact] for controls such as switches that fit beside a wrapped label.
class SettingRow extends StatelessWidget {
  const SettingRow({
    super.key,
    required this.label,
    required this.control,
    this.description,
    this.footer,
    this.fullWidth = false,
    this.compact = false,
    this.enabled = true,
  });

  final String label;
  final Widget control;
  final Widget? description;
  final Widget? footer;
  final bool fullWidth;
  final bool compact;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final labelWidget = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          style: theme.textTheme.bodyLarge?.copyWith(
            color: enabled ? null : theme.disabledColor,
          ),
        ),
        if (description != null) ...[
          const SizedBox(height: 4),
          DefaultTextStyle.merge(
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
            child: description!,
          ),
        ],
      ],
    );

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final stacked = fullWidth ||
              (!compact &&
                  constraints.maxWidth <
                      600 * MediaQuery.textScalerOf(context).scale(1));
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (stacked) ...[
                labelWidget,
                const SizedBox(height: 10),
                control,
              ] else
                Row(
                  children: [
                    Expanded(child: labelWidget),
                    const SizedBox(width: 24),
                    if (compact)
                      control
                    else
                      SizedBox(
                        width: (constraints.maxWidth * 0.46).clamp(0, 320),
                        child: control,
                      ),
                  ],
                ),
              if (footer != null) ...[
                const SizedBox(height: 10),
                footer!,
              ],
            ],
          );
        },
      ),
    );
  }
}
