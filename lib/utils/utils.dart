import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:system_date_time_format/system_date_time_format.dart';
import 'package:toastification/toastification.dart';
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

String? formatDateTime(BuildContext context, DateTime dateTime) {
  final dateFormat = SystemDateTimeFormat.of(context);
  final datePattern = dateFormat.datePattern;
  final timePattern = 'HH:mm';
  if (datePattern == null) {
    return null;
  }
  return DateFormat('$datePattern $timePattern').format(dateTime);
}
