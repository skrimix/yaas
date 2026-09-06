import 'package:flutter/material.dart';
import 'package:flutter_svg/svg.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart' hide Unit;
import '../../src/l10n/app_localizations.dart';
import '../device/device_actions.dart';
import '../common/no_device_connected_indicator.dart';

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final device = context.watch<DeviceState>();
    if (!device.isConnected) return const NoDeviceConnectedIndicator();

    return SingleChildScrollView(
      padding: const EdgeInsets.all(24),
      child: Align(
        alignment: Alignment.topCenter,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 1120),
          child: LayoutBuilder(
            builder: (context, constraints) {
              final overview = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _DeviceOverview(device: device),
                  const SizedBox(height: 20),
                  _BatteryOverview(device: device),
                ],
              );
              final controls = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const DeviceActionsCard(),
                  const SizedBox(height: 20),
                  _StorageOverview(device: device),
                ],
              );

              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(AppLocalizations.of(context).navHome,
                      style: Theme.of(context).textTheme.headlineMedium),
                  const SizedBox(height: 24),
                  if (constraints.maxWidth >= 960)
                    Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Expanded(child: overview),
                        const SizedBox(width: 20),
                        SizedBox(width: 340, child: controls),
                      ],
                    )
                  else ...[
                    overview,
                    const SizedBox(height: 20),
                    controls,
                  ],
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

class _DeviceOverview extends StatelessWidget {
  const _DeviceOverview({required this.device});

  final DeviceState device;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final l10n = AppLocalizations.of(context);
    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(24),
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [
            scheme.primaryContainer.withValues(alpha: 0.45),
            scheme.surfaceContainerLow,
          ],
        ),
        border: Border.all(color: scheme.primary.withValues(alpha: 0.12)),
      ),
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(Icons.check_circle_outline, size: 16, color: scheme.primary),
              const SizedBox(width: 8),
              Expanded(
                child: Text(l10n.statusAdbConnected,
                    style: theme.textTheme.labelLarge?.copyWith(
                      color: scheme.primary,
                    )),
              ),
              _DeviceMenuButton(),
            ],
          ),
          const SizedBox(height: 8),
          LayoutBuilder(
            builder: (context, constraints) {
              final compact = constraints.maxWidth < 540;
              final details = Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(device.deviceName,
                      style: theme.textTheme.headlineMedium?.copyWith(
                        fontWeight: FontWeight.w600,
                      )),
                  const SizedBox(height: 12),
                  Text(l10n.homeSerialNumber,
                      style: theme.textTheme.labelMedium?.copyWith(
                        color: scheme.onSurfaceVariant,
                      )),
                  const SizedBox(height: 4),
                  SelectableText(device.deviceTrueSerial,
                      style: theme.textTheme.bodyMedium),
                  const SizedBox(height: 20),
                  Row(
                    children: [
                      Icon(device.isWireless ? Icons.wifi : Icons.usb,
                          size: 18, color: scheme.onSurfaceVariant),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          device.isWireless
                              ? l10n.settingsConnectionWireless
                              : [
                                  l10n.settingsConnectionUsb,
                                  if (device.usbSpeed != null) device.usbSpeed!,
                                ].join(' · '),
                          style: theme.textTheme.bodyMedium?.copyWith(
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                    ],
                  ),
                ],
              );
              final artwork = Container(
                height: 220,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  gradient: RadialGradient(colors: [
                    scheme.primary.withValues(alpha: 0.12),
                    scheme.primary.withValues(alpha: 0),
                  ]),
                ),
                child: Image.asset(
                  'assets/png/headset/${device.productName}.png',
                  width: 260,
                  height: 220,
                  fit: BoxFit.contain,
                  excludeFromSemantics: true,
                  errorBuilder: (context, error, stackTrace) => Image.asset(
                    'assets/png/headset/unknown.png',
                    width: 260,
                    height: 220,
                    fit: BoxFit.contain,
                    excludeFromSemantics: true,
                  ),
                ),
              );
              if (compact) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [artwork, const SizedBox(height: 16), details],
                );
              }
              return Row(
                children: [
                  Expanded(child: details),
                  const SizedBox(width: 16),
                  Expanded(child: artwork),
                ],
              );
            },
          ),
        ],
      ),
    );
  }
}

class _BatteryOverview extends StatelessWidget {
  const _BatteryOverview({required this.device});

  final DeviceState device;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final cards = [
      _BatteryCard(
        title: l10n.headset,
        asset: 'headset',
        level: device.batteryLevel,
        isCharging: device.isCharging == true,
      ),
      _BatteryCard(
        title: l10n.leftController,
        asset: 'controller_l',
        level: device.leftController?.batteryLevel,
        status: device.controllerStatusString(context, device.leftController),
        isActive: device.leftController?.status is ControllerStatusActive,
      ),
      _BatteryCard(
        title: l10n.rightController,
        asset: 'controller_r',
        level: device.rightController?.batteryLevel,
        status: device.controllerStatusString(context, device.rightController),
        isActive: device.rightController?.status is ControllerStatusActive,
      ),
    ];
    return LayoutBuilder(builder: (context, constraints) {
      if (constraints.maxWidth < 480) {
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (var i = 0; i < cards.length; i++) ...[
              if (i > 0) const SizedBox(height: 12),
              cards[i],
            ],
          ],
        );
      }
      return IntrinsicHeight(
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (var i = 0; i < cards.length; i++) ...[
              if (i > 0) const SizedBox(width: 12),
              Expanded(child: cards[i]),
            ],
          ],
        ),
      );
    });
  }
}

class _BatteryCard extends StatelessWidget {
  const _BatteryCard({
    required this.title,
    required this.asset,
    required this.level,
    this.status,
    this.isCharging = false,
    this.isActive = true,
  });

  final String title;
  final String asset;
  final int? level;
  final String? status;
  final bool isCharging;
  final bool isActive;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final l10n = AppLocalizations.of(context);
    final value = level?.clamp(0, 100);
    final color = !isActive || value == null
        ? scheme.outline
        : value <= 20
            ? scheme.error
            : scheme.primary;
    final batteryText = value == null ? '—' : '$value%';
    return Tooltip(
      message: [
        title,
        if (status != null) '${l10n.statusLabel}: $status',
        '${l10n.batteryLabel}: $batteryText'
            '${isCharging ? ' (${l10n.chargingLabel})' : ''}',
      ].join('\n'),
      child: Card(
        margin: EdgeInsets.zero,
        elevation: 0,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  SvgPicture.asset(
                    'assets/svg/$asset.svg',
                    width: 22,
                    height: 22,
                    colorFilter: ColorFilter.mode(
                        scheme.onSurfaceVariant, BlendMode.srcIn),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                      child: Text(title, style: theme.textTheme.labelLarge)),
                ],
              ),
              const SizedBox(height: 20),
              Row(
                children: [
                  Flexible(
                    child: Text(batteryText,
                        style: theme.textTheme.headlineMedium?.copyWith(
                          color: isActive ? scheme.onSurface : scheme.outline,
                        )),
                  ),
                  if (isCharging) ...[
                    const SizedBox(width: 4),
                    Icon(Icons.bolt,
                        key: const ValueKey('headset-charging-badge'),
                        size: 20,
                        color: scheme.primary),
                  ],
                ],
              ),
              const SizedBox(height: 4),
              Text(
                isCharging ? l10n.chargingLabel : status ?? l10n.batteryLabel,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 16),
              if (value != null)
                LinearProgressIndicator(
                  value: value / 100,
                  semanticsLabel: '$title: ${l10n.batteryLabel}',
                  minHeight: 5,
                  borderRadius: BorderRadius.circular(3),
                  color: color,
                  backgroundColor: scheme.surfaceContainerHighest,
                )
              else
                const SizedBox(height: 5),
            ],
          ),
        ),
      ),
    );
  }
}

class _StorageOverview extends StatelessWidget {
  const _StorageOverview({required this.device});

  final DeviceState device;

  String _formatSize(int bytes) => FileSize.fromBytes(bytes).toString(
        unit: Unit.auto(size: bytes, baseType: BaseType.metric),
        decimals: 1,
      );

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final l10n = AppLocalizations.of(context);
    final space = device.spaceInfo;
    final total = space?.total.toInt() ?? 0;
    final available = (space?.available.toInt() ?? 0).clamp(0, total);
    final usedFraction = total > 0 ? 1 - available / total : 0.0;
    return Card(
      margin: EdgeInsets.zero,
      elevation: 0,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(Icons.storage_rounded, size: 20),
                const SizedBox(width: 10),
                Text(l10n.settingsSectionStorage,
                    style: theme.textTheme.titleMedium),
              ],
            ),
            const SizedBox(height: 20),
            Text(total > 0 ? _formatSize(available) : '—',
                style: theme.textTheme.headlineMedium),
            const SizedBox(height: 4),
            Text(l10n.homeFreeSpace,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                )),
            const SizedBox(height: 16),
            if (total > 0)
              LinearProgressIndicator(
                value: usedFraction,
                semanticsLabel: l10n.detailsStorageUsage,
                minHeight: 6,
                borderRadius: BorderRadius.circular(3),
                color: usedFraction >= 0.9 ? theme.colorScheme.error : null,
                backgroundColor: theme.colorScheme.surfaceContainerHighest,
              )
            else
              const SizedBox(height: 6),
            const SizedBox(height: 10),
            Text(
              total > 0
                  ? l10n.storageTooltip(
                      _formatSize(available), _formatSize(total))
                  : l10n.deviceStorageStatusUnknown,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _DeviceMenuButton extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return PopupMenuButton<String>(
      tooltip: l10n.deviceActionsTooltip,
      onSelected: (value) async {
        switch (value) {
          case 'powerOff':
            _confirmAndSend(
              context,
              title: l10n.powerOffDevice,
              message: l10n.powerOffConfirm,
              command: const AdbCommandReboot(value: RebootMode.powerOff),
            );
            break;
          case 'reboot':
            _showRebootOptions(context);
            break;
        }
      },
      itemBuilder: (context) => [
        PopupMenuItem(value: 'powerOff', child: Text(l10n.powerOffMenu)),
        PopupMenuItem(value: 'reboot', child: Text(l10n.rebootMenu)),
      ],
    );
  }

  void _confirmAndSend(BuildContext context,
      {required String title,
      required String message,
      required AdbCommand command}) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(title),
        content: Text(message),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: Text(AppLocalizations.of(context).commonCancel)),
          FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: Text(AppLocalizations.of(context).commonConfirm)),
        ],
      ),
    );
    if (!context.mounted) return;
    if (confirmed == true) {
      AdbRequest(command: command, commandKey: '').sendSignalToRust();
    }
  }

  void _showRebootOptions(BuildContext context) async {
    final option = await showDialog<String>(
      context: context,
      builder: (context) => SimpleDialog(
        title: Text(AppLocalizations.of(context).rebootOptions),
        children: [
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'normal'),
            child: Text(AppLocalizations.of(context).rebootNormal),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'bootloader'),
            child: Text(AppLocalizations.of(context).rebootBootloader),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'recovery'),
            child: Text(AppLocalizations.of(context).rebootRecovery),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'fastboot'),
            child: Text(AppLocalizations.of(context).rebootFastboot),
          ),
        ],
      ),
    );

    if (!context.mounted) return;
    if (option == null) return;

    switch (option) {
      case 'normal':
        _confirmAndSend(
          context,
          title: AppLocalizations.of(context).rebootDevice,
          message: AppLocalizations.of(context).rebootNowConfirm,
          command: const AdbCommandReboot(value: RebootMode.normal),
        );
        break;
      case 'bootloader':
        _confirmAndSend(
          context,
          title: AppLocalizations.of(context).rebootToBootloader,
          message: AppLocalizations.of(context).rebootToBootloaderConfirm,
          command: const AdbCommandReboot(value: RebootMode.bootloader),
        );
        break;
      case 'recovery':
        _confirmAndSend(
          context,
          title: AppLocalizations.of(context).rebootToRecovery,
          message: AppLocalizations.of(context).rebootToRecoveryConfirm,
          command: const AdbCommandReboot(value: RebootMode.recovery),
        );
        break;
      case 'fastboot':
        _confirmAndSend(
          context,
          title: AppLocalizations.of(context).rebootToFastboot,
          message: AppLocalizations.of(context).rebootToFastbootConfirm,
          command: const AdbCommandReboot(value: RebootMode.fastboot),
        );
        break;
    }
  }
}
