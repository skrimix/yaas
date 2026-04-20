import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../src/l10n/app_localizations.dart';

class SelectableLinkText extends StatefulWidget {
  const SelectableLinkText({
    required this.text,
    this.style,
    this.linkStyle,
    this.launchMode = LaunchMode.externalApplication,
    super.key,
  });

  final String text;
  final TextStyle? style;
  final TextStyle? linkStyle;
  final LaunchMode launchMode;

  @override
  State<SelectableLinkText> createState() => _SelectableLinkTextState();
}

class _SelectableLinkTextState extends State<SelectableLinkText> {
  static final RegExp _urlPattern = RegExp(r'https?:\/\/\S+');

  final Map<String, TapGestureRecognizer> _linkRecognizers = {};

  @override
  void dispose() {
    for (final recognizer in _linkRecognizers.values) {
      recognizer.dispose();
    }
    super.dispose();
  }

  TapGestureRecognizer _linkRecognizerFor(String url) {
    return _linkRecognizers.putIfAbsent(
      url,
      () => TapGestureRecognizer()..onTap = () => unawaited(_openUrl(url)),
    );
  }

  Future<void> _openUrl(String url) async {
    final uri = Uri.tryParse(url);
    if (uri == null) return;

    final ok = await launchUrl(uri, mode: widget.launchMode);
    if (!ok && mounted) {
      final l10n = AppLocalizations.of(context);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(l10n.couldNotOpenUrl(url))),
      );
    }
  }

  ({String url, String trailing}) _splitUrlMatch(String value) {
    final match = RegExp(r'[.,;:!?\])}]+$').firstMatch(value);
    if (match == null || match.start == 0) {
      return (url: value, trailing: '');
    }

    return (
      url: value.substring(0, match.start),
      trailing: value.substring(match.start),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final linkStyle = widget.linkStyle ??
        widget.style?.copyWith(
          color: theme.colorScheme.primary,
          decoration: TextDecoration.underline,
        ) ??
        TextStyle(
          color: theme.colorScheme.primary,
          decoration: TextDecoration.underline,
        );
    final spans = <InlineSpan>[];
    var last = 0;

    for (final match in _urlPattern.allMatches(widget.text)) {
      if (match.start > last) {
        spans.add(TextSpan(text: widget.text.substring(last, match.start)));
      }

      final matchText = widget.text.substring(match.start, match.end);
      final parts = _splitUrlMatch(matchText);
      if (parts.url.isNotEmpty) {
        spans.add(
          TextSpan(
            text: parts.url,
            style: linkStyle,
            recognizer: _linkRecognizerFor(parts.url),
          ),
        );
      }
      if (parts.trailing.isNotEmpty) {
        spans.add(TextSpan(text: parts.trailing));
      }
      last = match.end;
    }

    if (last < widget.text.length) {
      spans.add(TextSpan(text: widget.text.substring(last)));
    }

    return SelectableText.rich(
      TextSpan(children: spans.isEmpty ? [TextSpan(text: widget.text)] : spans),
      style: widget.style,
    );
  }
}
