import 'dart:io' show Platform;
import 'dart:math' as math;
import 'package:flutter/material.dart';
import 'package:rinf/rinf.dart';
import '../../providers/adb_state.dart';
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

              // Proximity sensor toggle
              const _ProximityToggle(),

              const SizedBox(height: 16),

              // Guardian toggle
              const _GuardianToggle(),

              const SizedBox(height: 16),

              // Wireless ADB (when not already enabled)
              Builder(
                builder: (context) {
                  final device = context.watch<DeviceState>();
                  final adb = context.watch<AdbStateProvider>();
                  if (!device.isConnected ||
                      device.isWireless ||
                      // Check that we don't have an active wireless connection for this device already
                      adb.availableDevices.any((d) =>
                          d.isWireless &&
                          d.trueSerial == device.deviceTrueSerial &&
                          d.state == AdbBriefState.device)) {
                    return const SizedBox.shrink();
                  }
                  return Row(
                    children: [
                      const Icon(Icons.wifi_tethering),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(l10n.deviceWirelessAdb,
                            style: Theme.of(context).textTheme.titleSmall),
                      ),
                      AnimatedAdbButton(
                        icon: Icons.wifi,
                        tooltip: l10n.deviceEnableWirelessAdb,
                        commandType: AdbCommandKind.wirelessAdbEnable,
                        commandKey: 'enable-wireless',
                        onPressed: () => _send('enable-wireless',
                            const AdbCommandEnableWirelessAdb()),
                      ),
                    ],
                  );
                },
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
        const DownloadCastingBundleRequest().sendSignalToRust();
        // Auto-launch when installation finishes
        CastingStatusChanged.rustSignalStream
            .firstWhere((e) => e.message.installed == true)
            .then((_) {
          AdbRequest(
                  command: const AdbCommandStartCasting(), commandKey: 'cast')
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

/// Guardian toggle that shows the current state and allows toggling
/// ON = Guardian active, OFF = Guardian suspended
class _GuardianToggle extends StatefulWidget {
  const _GuardianToggle();

  @override
  State<_GuardianToggle> createState() => _GuardianToggleState();
}

class _GuardianToggleState extends State<_GuardianToggle> {
  /// The expected active state we're waiting for (null if not updating)
  bool? _pendingActiveState;

  void _toggle(bool newActiveState) {
    if (_pendingActiveState != null) return;
    setState(() => _pendingActiveState = newActiveState);
    // newActiveState == true means Guardian should be active (not paused)
    AdbRequest(
      command: AdbCommandSetGuardianPaused(value: !newActiveState),
      commandKey: 'guardian',
    ).sendSignalToRust();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final device = context.watch<DeviceState>();
    final guardianPaused = device.guardianPaused;

    // guardianActive = !guardianPaused
    final bool? guardianActive =
        guardianPaused != null ? !guardianPaused : null;

    // Clear pending state once actual state matches expected
    if (_pendingActiveState != null && guardianActive == _pendingActiveState) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) setState(() => _pendingActiveState = null);
      });
    }

    final isUpdating = _pendingActiveState != null;
    final isActive = guardianActive ?? true;
    final theme = Theme.of(context);

    return Row(
      children: [
        const Icon(Icons.security),
        const SizedBox(width: 8),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(l10n.deviceGuardian, style: theme.textTheme.titleSmall),
              Text(
                isActive
                    ? l10n.guardianStatusActive
                    : l10n.guardianStatusSuspended,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
            ],
          ),
        ),
        if (isUpdating)
          const SizedBox(
            width: 60,
            height: 40,
            child: Center(
              child: SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            ),
          )
        else
          Switch.adaptive(
            value: isActive,
            onChanged: guardianActive != null ? _toggle : null,
            activeTrackColor:
                theme.colorScheme.primaryContainer.withValues(alpha: 0.6),
            activeThumbColor: theme.colorScheme.primary.withValues(alpha: 0.8),
            inactiveTrackColor: theme.colorScheme.surfaceContainerHighest,
            inactiveThumbColor: theme.colorScheme.outline,
          ),
      ],
    );
  }
}

/// Proximity sensor toggle that shows the current state and allows enabling/disabling.
class _ProximityToggle extends StatefulWidget {
  const _ProximityToggle();

  @override
  State<_ProximityToggle> createState() => _ProximityToggleState();
}

class _ProximityToggleState extends State<_ProximityToggle> {
  /// The expected disabled state we're waiting for (null if not updating)
  bool? _pendingDisabledState;

  void _enableSensor() {
    if (_pendingDisabledState != null) return;
    setState(() => _pendingDisabledState = false);
    AdbRequest(
      command:
          const AdbCommandSetProximitySensor(enabled: true, durationMs: null),
      commandKey: 'proximity',
    ).sendSignalToRust();
  }

  void _disableSensor(int durationMs) {
    if (_pendingDisabledState != null) return;
    setState(() => _pendingDisabledState = true);
    AdbRequest(
      command: AdbCommandSetProximitySensor(
        enabled: false,
        // 0 means no limit, pass null to Rust
        durationMs:
            durationMs > 0 ? Uint64.fromBigInt(BigInt.from(durationMs)) : null,
      ),
      commandKey: 'proximity',
    ).sendSignalToRust();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final device = context.watch<DeviceState>();
    final proximityDisabled = device.proximityDisabled;

    // Clear pending state once actual state matches expected
    if (_pendingDisabledState != null &&
        proximityDisabled == _pendingDisabledState) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) setState(() => _pendingDisabledState = null);
      });
    }

    final isUpdating = _pendingDisabledState != null;
    final sensorDisabled = proximityDisabled == true;
    final theme = Theme.of(context);

    return Row(
      children: [
        Icon(sensorDisabled ? Icons.sensors_off : Icons.sensors),
        const SizedBox(width: 8),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(l10n.deviceProximitySensor,
                  style: theme.textTheme.titleSmall),
              Text(
                proximityDisabled == null
                    ? l10n.proximityStatusUnknown
                    : sensorDisabled
                        ? l10n.proximityStatusDisabled
                        : l10n.proximityStatusEnabled,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
            ],
          ),
        ),
        if (isUpdating)
          const SizedBox(
            width: 60,
            height: 20,
            child: Center(
              child: SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            ),
          )
        else if (sensorDisabled)
          FilledButton.tonal(
            onPressed: _enableSensor,
            child: Text(l10n.proximityEnable),
          )
        else
          // Sensor is enabled or unknown
          PopupMenuButton<int>(
            enabled: proximityDisabled != null,
            tooltip: l10n.disableProximitySensor,
            onSelected: _disableSensor,
            itemBuilder: (context) => [
              PopupMenuItem(
                value: 2 * 60 * 60 * 1000, // 2 hours
                child: Text(l10n.proximityDisable2h),
              ),
              PopupMenuItem(
                value: 4 * 60 * 60 * 1000, // 4 hours
                child: Text(l10n.proximityDisable4h),
              ),
              PopupMenuItem(
                value: 8 * 60 * 60 * 1000, // 8 hours
                child: Text(l10n.proximityDisable8h),
              ),
              PopupMenuItem(
                value: 0, // No limit
                child: Text(l10n.proximityDisableNoLimit),
              ),
            ],
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              decoration: BoxDecoration(
                color: theme.colorScheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(l10n.disable, style: theme.textTheme.labelLarge),
                  const SizedBox(width: 4),
                  Icon(Icons.arrow_drop_down,
                      size: 18, color: theme.colorScheme.onSurfaceVariant),
                ],
              ),
            ),
          ),
      ],
    );
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
