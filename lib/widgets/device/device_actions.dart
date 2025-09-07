import 'package:flutter/material.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../common/animated_adb_button.dart';

class DeviceActionsCard extends StatelessWidget {
  const DeviceActionsCard({super.key});

  void _send(String key, AdbCommand command) {
    AdbRequest(command: command, commandKey: key).sendSignalToRust();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return SizedBox(
      width: 350,
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(16.0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(l10n.deviceActions,
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 12),

              // Proximity sensor
              Row(
                children: [
                  const Icon(Icons.sensors),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(l10n.deviceProximitySensor,
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  AnimatedAdbButton(
                    icon: Icons.sensors_off,
                    tooltip: l10n.disableProximitySensor,
                    commandType: AdbCommandType.proximitySensorSet,
                    commandKey: 'disable',
                    onPressed: () => _send('disable',
                        const AdbCommandSetProximitySensor(value: false)),
                  ),
                  const SizedBox(width: 8),
                  AnimatedAdbButton(
                    icon: Icons.sensors,
                    tooltip: l10n.enableProximitySensor,
                    commandType: AdbCommandType.proximitySensorSet,
                    commandKey: 'enable',
                    onPressed: () => _send('enable',
                        const AdbCommandSetProximitySensor(value: true)),
                  ),
                ],
              ),

              const SizedBox(height: 16),

              // Guardian
              // TODO: Convert into a stateful toggle
              Row(
                children: [
                  const Icon(Icons.security),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(l10n.deviceGuardian,
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  AnimatedAdbButton(
                    icon: Icons.pause_circle_filled,
                    tooltip: l10n.guardianSuspend,
                    commandType: AdbCommandType.guardianPausedSet,
                    commandKey: 'suspend',
                    onPressed: () => _send('suspend',
                        const AdbCommandSetGuardianPaused(value: true)),
                  ),
                  const SizedBox(width: 8),
                  AnimatedAdbButton(
                    icon: Icons.play_circle_fill,
                    tooltip: l10n.guardianResume,
                    commandType: AdbCommandType.guardianPausedSet,
                    commandKey: 'resume',
                    onPressed: () => _send('resume',
                        const AdbCommandSetGuardianPaused(value: false)),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
