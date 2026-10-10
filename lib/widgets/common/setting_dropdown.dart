import 'package:flutter/material.dart';

/// A dropdown with the same padding as settings fields.
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
        decoration: const InputDecoration(
          isDense: true,
          contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 12),
        ),
        borderRadius: BorderRadius.circular(12),
      ),
    );
  }
}
