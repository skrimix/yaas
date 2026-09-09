import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../providers/app_state.dart';
import '../../providers/app_update_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

class AppUpdateBanner extends StatelessWidget {
  const AppUpdateBanner({super.key});

  @override
  Widget build(BuildContext context) {
    final update = context.watch<AppUpdateState>();
    if (!update.showBanner) return const SizedBox.shrink();
    final state = update.snapshot!;
    final release = state.release!;
    final l10n = AppLocalizations.of(context);
    final actions = [
      TextButton(
        onPressed: () => context.read<AppState>().requestNavigationTo('about'),
        child: Text(l10n.updatesView),
      ),
      IconButton(
        tooltip: l10n.updatesDismiss,
        onPressed: update.dismissBanner,
        icon: const Icon(Icons.close),
      ),
    ];
    return LayoutBuilder(builder: (context, constraints) {
      final inline = constraints.maxWidth >=
          700 * MediaQuery.textScalerOf(context).scale(1);
      return MaterialBanner(
        forceActionsBelow: !inline,
        leading: const Icon(Icons.system_update_alt),
        content: Text(
            '${state.phase == AppUpdatePhase.ready ? l10n.updatesReady : l10n.updatesAvailable}'
            ' · YAAS ${release.version}+${release.buildNumber}'),
        actions: inline
            ? [Row(mainAxisSize: MainAxisSize.min, children: actions)]
            : actions,
      );
    });
  }
}
