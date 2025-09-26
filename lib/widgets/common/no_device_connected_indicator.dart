import 'package:flutter/material.dart';
import '../../src/l10n/app_localizations.dart';
import 'connection_diagnostics_button.dart';

class NoDeviceConnectedIndicator extends StatelessWidget {
  final bool showDiagnosticsButton;
  final bool centered;
  final Axis direction;
  final TextStyle? textStyle;
  final double spacing;

  const NoDeviceConnectedIndicator({
    super.key,
    this.showDiagnosticsButton = true,
    this.centered = true,
    this.direction = Axis.vertical,
    this.textStyle,
    this.spacing = 8.0,
  });

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final children = <Widget>[
      Text(
        l10n.noDeviceConnected,
        style: textStyle ?? const TextStyle(fontSize: 18),
        textAlign: centered ? TextAlign.center : null,
      ),
      if (showDiagnosticsButton) ...[
        SizedBox(
            height: direction == Axis.vertical ? spacing : 0,
            width: direction == Axis.horizontal ? spacing : 0),
        const ConnectionDiagnosticsButton(),
      ],
    ];

    final content = direction == Axis.vertical
        ? Column(mainAxisSize: MainAxisSize.min, children: children)
        : Row(mainAxisSize: MainAxisSize.min, children: children);

    return centered ? Center(child: content) : content;
  }
}
