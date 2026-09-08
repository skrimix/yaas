import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:system_date_time_format/system_date_time_format.dart';
import 'package:toastification/toastification.dart';
import 'package:proper_filesize/proper_filesize.dart' as filesize;
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

void copyToClipboard(BuildContext context, String text,
    {String? title, Duration? autoCloseDuration, String? description}) {
  Clipboard.setData(ClipboardData(text: text));
  toastification.show(
    type: ToastificationType.success,
    style: ToastificationStyle.flat,
    title: title != null
        ? Text(title)
        : Text(AppLocalizations.of(context).copiedToClipboard),
    description: description != null ? Text(description) : null,
    autoCloseDuration: autoCloseDuration ?? const Duration(seconds: 2),
    backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
    borderSide: BorderSide.none,
    alignment: Alignment.bottomRight,
  );
}

Widget buildCopyableText(
  BuildContext context,
  String text, {
  bool showTooltip = true,
  bool showIconOnHover = false,
  String? copyText,
  String? tooltipMessage,
  TextStyle? style,
  int? maxLines,
  TextOverflow? overflow,
}) {
  final value = copyText ?? text;
  final child = _CopyableTextAction(
    showIconOnHover: showIconOnHover,
    onTap: () => copyToClipboard(context, value, description: value),
    child: Text(
      text,
      style: style,
      maxLines: maxLines,
      overflow: overflow,
    ),
  );

  if (!showTooltip) return child;

  return Tooltip(
    message: tooltipMessage ?? AppLocalizations.of(context).clickToCopy,
    waitDuration: const Duration(milliseconds: 300),
    child: child,
  );
}

class _CopyableTextAction extends StatefulWidget {
  const _CopyableTextAction({
    required this.showIconOnHover,
    required this.onTap,
    required this.child,
  });

  final bool showIconOnHover;
  final VoidCallback onTap;
  final Widget child;

  @override
  State<_CopyableTextAction> createState() => _CopyableTextActionState();
}

class _CopyableTextActionState extends State<_CopyableTextAction> {
  bool _hovered = false;
  bool _focused = false;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Material(
      type: MaterialType.transparency,
      textStyle: DefaultTextStyle.of(context).style,
      child: InkWell(
        borderRadius: BorderRadius.circular(4),
        hoverColor: colors.onSurface.withValues(alpha: 0.06),
        focusColor: colors.onSurface.withValues(alpha: 0.1),
        onHover: (hovered) => setState(() => _hovered = hovered),
        onFocusChange: (focused) => setState(() => _focused = focused),
        onTap: widget.onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(child: widget.child),
              const SizedBox(width: 6),
              AnimatedOpacity(
                opacity:
                    !widget.showIconOnHover || _hovered || _focused ? 1 : 0,
                duration: const Duration(milliseconds: 120),
                child:
                    Icon(Icons.copy, size: 14, color: colors.onSurfaceVariant),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

String? formatDateTime(BuildContext context, DateTime dateTime) {
  final dateFormat = SystemDateTimeFormat.of(context);
  final datePattern = dateFormat.datePattern;
  final timePattern = 'HH:mm';
  if (datePattern == null) {
    return null;
  }
  return DateFormat('$datePattern $timePattern').format(dateTime);
}

/// Shows a confirmation dialog when attempting to downgrade an app.
/// Returns true if the user confirms, false otherwise.
Future<bool> showDowngradeConfirmDialog(
  BuildContext context,
  InstalledPackage installed,
  CloudApp target,
) async {
  final l10n = AppLocalizations.of(context);
  final res = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(l10n.downgradeAppTitle),
      content: Text(l10n.downgradeConfirmMessage('${target.versionCode}')),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(l10n.commonCancel),
        ),
        FilledButton(
          style: ButtonStyle(
            backgroundColor:
                WidgetStatePropertyAll(Theme.of(context).colorScheme.error),
            foregroundColor:
                WidgetStatePropertyAll(Theme.of(context).colorScheme.onError),
          ),
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(l10n.commonConfirm),
        ),
      ],
    ),
  );
  return res ?? false;
}

String formatSize(int bytes, int decimals) {
  return filesize.FileSize.fromBytes(bytes).toString(
    unit: filesize.Unit.auto(
      size: bytes,
      baseType: filesize.BaseType.metric,
    ),
    decimals: decimals,
  );
}
