import 'package:flutter/material.dart';

/// Shared metrics for the settings list so tiles line up across sections.
class SettingsMetrics {
  static const double sectionSpacing = 20.0;
  static const double groupRadius = 20.0;
  static const double tileHorizontalPadding = 20.0;
  static const double tileVerticalPadding = 14.0;
  static const double iconSize = 22.0;
  static const double iconGap = 18.0;

  /// Left inset that aligns secondary content with the tile title.
  static const double contentIndent = iconSize + iconGap;

  /// Width limits for the dropdown used by [SettingChoiceTile].
  static const double menuMinWidth = 140.0;
  static const double menuMaxWidth = 280.0;
}

/// A labeled group of settings tiles.
///
/// The label sits above a rounded container that holds [children].
class SettingsSection extends StatelessWidget {
  const SettingsSection({
    super.key,
    required this.title,
    required this.children,
    this.icon,
  });

  final String title;
  final IconData? icon;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.only(left: 20, right: 20, bottom: 10),
          child: Row(
            children: [
              if (icon != null) ...[
                Icon(icon, size: 18, color: theme.colorScheme.primary),
                const SizedBox(width: 8),
              ],
              Expanded(
                child: Text(
                  title,
                  style: theme.textTheme.titleSmall?.copyWith(
                    color: theme.colorScheme.primary,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
        ),
        Material(
          color: theme.colorScheme.surfaceContainerLow,
          borderRadius: BorderRadius.circular(SettingsMetrics.groupRadius),
          clipBehavior: Clip.antiAlias,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const SizedBox(height: 4),
              ...children,
              const SizedBox(height: 4),
            ],
          ),
        ),
      ],
    );
  }
}

/// A single settings row: leading icon, title, optional supporting text and a
/// trailing control.
///
/// Use [content] for controls that need the full row width (text fields,
/// segmented buttons) and [footer] for status messages below the row.
class SettingTile extends StatelessWidget {
  const SettingTile({
    super.key,
    required this.icon,
    required this.title,
    this.value,
    this.description,
    this.trailing,
    this.content,
    this.footer,
    this.onTap,
    this.enabled = true,
    this.stretchTrailing = false,
  });

  final IconData icon;
  final String title;

  /// Current value shown under the title, styled for quick scanning.
  final String? value;

  /// Supporting text explaining the setting.
  final Widget? description;
  final Widget? trailing;
  final Widget? content;
  final Widget? footer;
  final VoidCallback? onTap;
  final bool enabled;

  /// Lets [trailing] take a share of the row width instead of only its
  /// intrinsic width. Used by controls that should shrink on narrow windows.
  final bool stretchTrailing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final titleColor = enabled ? colors.onSurface : theme.disabledColor;
    final mutedColor =
        enabled ? colors.onSurfaceVariant : theme.disabledColor.withAlpha(120);

    final body = Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: SettingsMetrics.tileHorizontalPadding,
        vertical: SettingsMetrics.tileVerticalPadding,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Icon(icon, size: SettingsMetrics.iconSize, color: mutedColor),
              const SizedBox(width: SettingsMetrics.iconGap),
              Expanded(
                flex: 3,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      style: theme.textTheme.bodyLarge
                          ?.copyWith(color: titleColor),
                    ),
                    if (value != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        value!,
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: enabled ? colors.primary : theme.disabledColor,
                        ),
                      ),
                    ],
                    if (description != null) ...[
                      const SizedBox(height: 4),
                      DefaultTextStyle.merge(
                        style: theme.textTheme.bodySmall
                            ?.copyWith(color: mutedColor),
                        child: description!,
                      ),
                    ],
                  ],
                ),
              ),
              if (trailing != null) ...[
                const SizedBox(width: 16),
                if (stretchTrailing)
                  Expanded(
                    flex: 2,
                    child: Align(
                      alignment: Alignment.centerRight,
                      child: trailing!,
                    ),
                  )
                else
                  trailing!,
              ],
            ],
          ),
          if (content != null)
            Padding(
              padding: const EdgeInsets.only(
                left: SettingsMetrics.contentIndent,
                top: 12,
              ),
              child: content!,
            ),
          if (footer != null)
            Padding(
              padding: const EdgeInsets.only(
                left: SettingsMetrics.contentIndent,
                top: 10,
              ),
              child: footer!,
            ),
        ],
      ),
    );

    if (onTap == null || !enabled) {
      return MergeSemantics(child: body);
    }
    return MergeSemantics(
      child: InkWell(onTap: onTap, child: body),
    );
  }
}

/// A settings row with a trailing switch. The whole row toggles the value.
class SettingSwitchTile extends StatelessWidget {
  const SettingSwitchTile({
    super.key,
    required this.icon,
    required this.title,
    required this.value,
    required this.onChanged,
    this.description,
    this.enabled = true,
  });

  final IconData icon;
  final String title;
  final bool value;
  final ValueChanged<bool>? onChanged;
  final String? description;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final active = enabled && onChanged != null;
    return SettingTile(
      icon: icon,
      title: title,
      description: description == null ? null : Text(description!),
      enabled: active,
      onTap: active ? () => onChanged!(!value) : null,
      trailing: Switch(
        value: value,
        onChanged: active ? onChanged : null,
      ),
    );
  }
}

/// One option in a [SettingChoiceTile].
class SettingChoice<T> {
  const SettingChoice({required this.value, required this.label, this.leading});

  final T value;
  final String label;

  /// Optional visual shown before the label, such as a color swatch.
  final Widget? leading;
}

/// A settings row with a dropdown menu to pick one of [choices].
///
/// The dropdown shows the selected label, so the value stays visible without
/// opening the menu. It sits at the end of the row and moves below the title
/// when the row gets too narrow for both.
class SettingChoiceTile<T> extends StatelessWidget {
  const SettingChoiceTile({
    super.key,
    required this.icon,
    required this.title,
    required this.value,
    required this.choices,
    required this.onChanged,
    this.description,
    this.placeholder,
    this.footer,
    this.trailing,
  });

  final IconData icon;
  final String title;
  final T? value;
  final List<SettingChoice<T>> choices;
  final ValueChanged<T>? onChanged;
  final String? description;

  /// Shown instead of a value label when nothing is selected.
  final String? placeholder;
  final Widget? footer;

  /// Extra control placed before the dropdown, such as a refresh button.
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final enabled = onChanged != null && choices.isNotEmpty;
    final selected = choices.where((c) => c.value == value).firstOrNull;

    return LayoutBuilder(
      builder: (context, constraints) {
        // The extra control gets a fixed slot so the dropdown width can be
        // derived from the space that is left.
        const slot = 40.0;
        const gap = 8.0;
        final reserved = trailing == null ? 0.0 : slot + gap;

        // Width left for the title row after the tile padding and the icon.
        final rowWidth = constraints.maxWidth -
            SettingsMetrics.tileHorizontalPadding * 2 -
            SettingsMetrics.contentIndent;
        // The trailing slot gets two fifths of the row, see [SettingTile].
        final inlineWidth = (rowWidth - 16) * 0.4 - reserved;
        final inline = inlineWidth >= SettingsMetrics.menuMinWidth;
        final width = inline
            ? inlineWidth.clamp(
                SettingsMetrics.menuMinWidth,
                SettingsMetrics.menuMaxWidth,
              )
            : (rowWidth - reserved)
                .clamp(SettingsMetrics.menuMinWidth, double.infinity);

        final menu = _SettingDropdown<T>(
          width: width,
          enabled: enabled,
          selected: selected,
          placeholder: placeholder,
          label: title,
          choices: choices,
          onChanged: onChanged,
        );

        Widget control = menu;
        if (trailing != null) {
          final extra = SizedBox(
            width: slot,
            height: slot,
            child: Center(child: trailing!),
          );
          control = Row(
            mainAxisSize: MainAxisSize.min,
            children: inline
                ? [extra, const SizedBox(width: gap), menu]
                : [menu, const SizedBox(width: gap), extra],
          );
        }

        return SettingTile(
          icon: icon,
          title: title,
          description: description == null ? null : Text(description!),
          enabled: enabled,
          footer: footer,
          stretchTrailing: inline,
          trailing: inline ? control : null,
          content: inline
              ? null
              : Align(alignment: Alignment.centerLeft, child: control),
        );
      },
    );
  }
}

/// The dropdown used by [SettingChoiceTile], styled like the settings fields.
class _SettingDropdown<T> extends StatelessWidget {
  const _SettingDropdown({
    required this.width,
    required this.enabled,
    required this.selected,
    required this.placeholder,
    required this.label,
    required this.choices,
    required this.onChanged,
  });

  final double width;
  final bool enabled;
  final SettingChoice<T>? selected;
  final String? placeholder;
  final String label;
  final List<SettingChoice<T>> choices;
  final ValueChanged<T>? onChanged;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final border = OutlineInputBorder(
      borderRadius: BorderRadius.circular(12),
      borderSide: BorderSide.none,
    );

    return Semantics(
      label: label,
      child: DropdownMenu<T>(
        width: width,
        enabled: enabled,
        selectOnly: true,
        menuHeight: 320,
        initialSelection: selected?.value,
        leadingIcon: selected?.leading,
        hintText: placeholder,
        textStyle: theme.textTheme.bodyMedium,
        inputDecorationTheme: InputDecorationThemeData(
          filled: true,
          fillColor: enabled
              ? colors.surfaceContainerHighest.withValues(alpha: 0.7)
              : colors.surfaceContainerHighest.withValues(alpha: 0.35),
          isDense: true,
          contentPadding: const EdgeInsets.symmetric(horizontal: 14),
          constraints: const BoxConstraints(minHeight: 44),
          hintStyle: theme.textTheme.bodyMedium
              ?.copyWith(color: colors.onSurfaceVariant),
          border: border,
          enabledBorder: border,
          disabledBorder: border,
          focusedBorder: border.copyWith(
            borderSide: BorderSide(color: colors.primary, width: 2),
          ),
        ),
        menuStyle: MenuStyle(
          shape: WidgetStatePropertyAll(
            RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
          ),
          visualDensity: VisualDensity.compact,
        ),
        onSelected: (value) {
          if (value != null) onChanged?.call(value);
        },
        dropdownMenuEntries: [
          for (final choice in choices)
            DropdownMenuEntry<T>(
              value: choice.value,
              label: choice.label,
              leadingIcon: choice.leading,
            ),
        ],
      ),
    );
  }
}

/// Filled input decoration used by settings text fields.
InputDecoration settingsInputDecoration(
  BuildContext context, {
  String? hintText,
  String? errorText,
  String? prefixText,
  Widget? suffixIcon,
}) {
  final colors = Theme.of(context).colorScheme;
  final border = OutlineInputBorder(
    borderRadius: BorderRadius.circular(12),
    borderSide: BorderSide.none,
  );
  return InputDecoration(
    hintText: hintText,
    errorText: errorText,
    errorMaxLines: 2,
    prefixText: prefixText,
    suffixIcon: suffixIcon,
    filled: true,
    fillColor: colors.surfaceContainerHighest.withValues(alpha: 0.7),
    isDense: true,
    contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
    border: border,
    enabledBorder: border,
    disabledBorder: border,
    focusedBorder: border.copyWith(
      borderSide: BorderSide(color: colors.primary, width: 2),
    ),
  );
}
