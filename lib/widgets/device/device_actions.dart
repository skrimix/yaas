import 'package:flutter/material.dart';
import '../../src/bindings/bindings.dart';
import '../common/animated_adb_button.dart';

class DeviceActionsCard extends StatelessWidget {
  const DeviceActionsCard({super.key});

  void _send(String key, AdbCommand command) {
    AdbRequest(command: command, commandKey: key).sendSignalToRust();
  }

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 350,
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(16.0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('Device Actions',
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 12),

              // Proximity sensor
              Row(
                children: [
                  const Icon(Icons.sensors),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text('Proximity Sensor',
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  AnimatedAdbButton(
                    icon: Icons.sensors_off,
                    tooltip: 'Disable proximity sensor',
                    commandType: AdbCommandType.proximitySensorSet,
                    commandKey: 'disable',
                    onPressed: () => _send('disable',
                        const AdbCommandSetProximitySensor(value: false)),
                  ),
                  const SizedBox(width: 8),
                  AnimatedAdbButton(
                    icon: Icons.sensors,
                    tooltip: 'Enable proximity sensor',
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
                    child: Text('Guardian',
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  AnimatedAdbButton(
                    icon: Icons.pause_circle_filled,
                    tooltip: 'Suspend Guardian',
                    commandType: AdbCommandType.guardianPausedSet,
                    commandKey: 'suspend',
                    onPressed: () => _send('suspend',
                        const AdbCommandSetGuardianPaused(value: true)),
                  ),
                  const SizedBox(width: 8),
                  AnimatedAdbButton(
                    icon: Icons.play_circle_fill,
                    tooltip: 'Resume Guardian',
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
