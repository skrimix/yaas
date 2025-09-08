import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
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
