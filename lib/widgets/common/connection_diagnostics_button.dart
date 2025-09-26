import 'package:flutter/material.dart';
import '../../src/l10n/app_localizations.dart';
import '../dialogs/connection_diagnostics_dialog.dart';

class ConnectionDiagnosticsButton extends StatelessWidget {
  final bool compact;

  const ConnectionDiagnosticsButton({super.key, this.compact = false});

  void _openDialog(BuildContext context) {
    showDialog(
      context: context,
      builder: (context) => const ConnectionDiagnosticsDialog(),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    if (compact) {
      return Tooltip(
        message: l10n.diagnosticsTitle,
        child: IconButton(
          onPressed: () => _openDialog(context),
          icon: const Icon(Icons.troubleshoot_outlined, size: 16),
          padding: EdgeInsets.zero,
          visualDensity: VisualDensity.compact,
        ),
      );
    }

    return TextButton.icon(
      onPressed: () => _openDialog(context),
      icon: const Icon(Icons.troubleshoot_outlined),
      label: Text(l10n.diagnosticsTitle),
    );
  }
}
