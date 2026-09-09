import 'package:flutter/material.dart';

/// A filled dropdown with the same padding and focus border as settings fields.
class SettingDropdown<T> extends StatelessWidget {
  const SettingDropdown({
    super.key,
    required this.label,
    required this.initialValue,
    required this.items,
    required this.onChanged,
    this.hint,
  });

  final String label;
  final T? initialValue;
  final List<DropdownMenuItem<T>> items;
  final ValueChanged<T?>? onChanged;
  final Widget? hint;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final border = OutlineInputBorder(
      borderRadius: BorderRadius.circular(8),
      borderSide: BorderSide.none,
    );

    return Semantics(
      label: label,
      child: DropdownButtonFormField<T>(
        initialValue: initialValue,
        isExpanded: true,
        itemHeight: null,
        hint: hint,
        items: items,
        selectedItemBuilder: (context) => items
            .map((item) => Align(
                  alignment: Alignment.centerLeft,
                  child: DefaultTextStyle.merge(
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    child: item.child,
                  ),
                ))
            .toList(),
        onChanged: onChanged,
        decoration: InputDecoration(
          filled: true,
          fillColor: colors.surfaceContainerHighest.withValues(alpha: 0.55),
          isDense: true,
          contentPadding:
              const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
          border: border,
          enabledBorder: border,
          disabledBorder: border,
          focusedBorder: border.copyWith(
            borderSide: BorderSide(color: colors.primary, width: 2),
          ),
        ),
        borderRadius: BorderRadius.circular(8),
      ),
    );
  }
}
