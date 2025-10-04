import 'dart:io' show Platform;
import 'dart:async';
import 'dart:math' as math;
import 'package:flutter/material.dart';
import 'package:rinf/rinf.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../common/animated_adb_button.dart';
import '../../providers/device_state.dart';
import 'package:provider/provider.dart';

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

              // Casting (Windows only)
              if (Platform.isWindows) ...[
                const SizedBox(height: 16),
                _CastingRow(onStart: () => _handleCast(context)),
                const SizedBox(height: 8),
                const _CastingProgress(),
              ],
            ],
          ),
        ),
      ),
    );
  }

  static Future<void> _handleCast(BuildContext context) async {
    final l10n = AppLocalizations.of(context);
    final device = context.read<DeviceState>();
    if (!device.isConnected) return;

    // Quick reject for wireless devices
    if (device.isWireless) {
      await showDialog(
        context: context,
        builder: (ctx) => AlertDialog(
          title: Text(l10n.commonError),
          content: Text(l10n.castingWirelessUnsupported),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx),
                child: Text(l10n.commonClose)),
          ],
        ),
      );
      return;
    }

    // Check if installed
    final statusFuture = CastingStatusChanged.rustSignalStream.first;
    const GetCastingStatusRequest().sendSignalToRust();
    final status = (await statusFuture).message;
    if (!context.mounted) return;
    if (status.installed != true) {
      final confirm = await showDialog<bool>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: Text(l10n.castingRequiresDownloadTitle),
          content: Text(l10n.castingRequiresDownloadPrompt),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx, false),
                child: Text(l10n.commonCancel)),
            FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(l10n.commonDownload),
            ),
          ],
        ),
      );
      if (confirm == true) {
        const DownloadCastingBundleRequest(url: null).sendSignalToRust();
        // Auto-launch when installation finishes
        CastingStatusChanged.rustSignalStream
            .firstWhere((e) => e.message.installed == true)
            .then((_) {
          AdbRequest(
                  command: const AdbCommandStartCasting(),
                  commandKey: 'cast')
              .sendSignalToRust();
        });
        if (!context.mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(l10n.castingToolDownloading)),
        );
      }
      return;
    }

    AdbRequest(command: const AdbCommandStartCasting(), commandKey: 'cast')
        .sendSignalToRust();
  }
}

class _CastingRow extends StatelessWidget {
  final VoidCallback onStart;
  const _CastingRow({required this.onStart});

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Row(
      children: [
        const Icon(Icons.cast),
        const SizedBox(width: 8),
        Expanded(
          child: Text(l10n.deviceCasting,
              style: Theme.of(context).textTheme.titleSmall),
        ),
        FilledButton.icon(
          onPressed: onStart,
          icon: const Icon(Icons.play_arrow),
          label: Text(l10n.deviceStartCasting),
        ),
      ],
    );
  }
}

class _CastingProgress extends StatelessWidget {
  const _CastingProgress();

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return StreamBuilder<RustSignalPack<CastingStatusChanged>>(
      stream: CastingStatusChanged.rustSignalStream,
      builder: (context, statusSnap) {
        final installed = statusSnap.data?.message.installed == true;
        if (installed) return const SizedBox.shrink();
        return StreamBuilder<RustSignalPack<CastingDownloadProgress>>(
          stream: CastingDownloadProgress.rustSignalStream,
          builder: (context, snapshot) {
            final prog = snapshot.data?.message;
            if (prog == null) return const SizedBox.shrink();
            final total = prog.total?.toInt().toDouble();
            final received = prog.received.toInt().toDouble();
            final value = total == null || total == 0
                ? null
                : math.min(1.0, math.max(0.0, received / total));
            if (value == 1.0) return const SizedBox.shrink();
            final percent = value == null ? null : (value * 100).round();
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                LinearProgressIndicator(value: value),
                const SizedBox(height: 4),
                Text(
                  percent == null
                      ? l10n.castingToolDownloading
                      : '${l10n.castingToolDownloading} ($percent%)',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            );
          },
        );
      },
    );
  }
}
