import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/svg.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart' hide Unit;
import '../../src/l10n/app_localizations.dart';
import '../device/device_actions.dart';
import '../common/card_header.dart';
import '../common/no_device_connected_indicator.dart';

/// Radii and gaps shared by the home screen cards.
class _HomeMetrics {
  static const double gap = 16;
  static const double heroRadius = 28;
  static const double cardRadius = 20;
  static const double cardPadding = 20;
  static const double wideBreakpoint = 960;
}

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final device = context.watch<DeviceState>();
    if (!device.isConnected) return const NoDeviceConnectedIndicator();

    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(24, 16, 24, 32),
      child: Align(
        alignment: Alignment.topCenter,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 1200),
          child: LayoutBuilder(
            builder: (context, constraints) {
              final overview = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _DeviceHeroCard(device: device),
                  const SizedBox(height: _HomeMetrics.gap),
                  _BatteryCard(device: device),
                ],
              );
              final controls = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const DeviceActionsCard(),
                  const SizedBox(height: _HomeMetrics.gap),
                  _StorageCard(device: device),
                ],
              );

              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Padding(
                    padding: const EdgeInsets.only(left: 4, bottom: 16),
                    child: Text(AppLocalizations.of(context).navHome,
                        style: Theme.of(context).textTheme.headlineMedium),
                  ),
                  if (constraints.maxWidth >= _HomeMetrics.wideBreakpoint)
                    Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Expanded(child: overview),
                        const SizedBox(width: _HomeMetrics.gap),
                        SizedBox(width: 360, child: controls),
                      ],
                    )
                  else ...[
                    overview,
                    const SizedBox(height: _HomeMetrics.gap),
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

/// Small label pill used for connection details on the hero card.
class _InfoPill extends StatelessWidget {
  const _InfoPill({
    required this.icon,
    required this.label,
    required this.foreground,
    required this.background,
  });

  final IconData icon;
  final String label;
  final Color foreground;
  final Color background;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(100),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 16, color: foreground),
          const SizedBox(width: 6),
          Flexible(
            child: Text(
              label,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.labelLarge?.copyWith(color: foreground),
            ),
          ),
        ],
      ),
    );
  }
}

class _DeviceHeroCard extends StatelessWidget {
  const _DeviceHeroCard({required this.device});

  final DeviceState device;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final l10n = AppLocalizations.of(context);
    final onHero = scheme.onSecondaryContainer;
    final mutedOnHero = onHero.withValues(alpha: 0.72);
    final connection = device.isWireless
        ? l10n.settingsConnectionWireless
        : [
            l10n.settingsConnectionUsb,
            if (device.usbSpeed != null) device.usbSpeed!,
          ].join(' · ');

    return Card.filled(
      margin: EdgeInsets.zero,
      color: scheme.secondaryContainer,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(_HomeMetrics.heroRadius),
      ),
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: _InfoPill(
                      icon: Icons.check_circle,
                      label: l10n.statusAdbConnected,
                      foreground: onHero,
                      background: scheme.surface.withValues(alpha: 0.45),
                    ),
                  ),
                ),
                _DeviceMenuButton(color: onHero),
              ],
            ),
            const SizedBox(height: 16),
            LayoutBuilder(
              builder: (context, constraints) {
                final compact = constraints.maxWidth < 540;
                final details = Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(device.deviceName,
                        style: theme.textTheme.headlineMedium
                            ?.copyWith(color: onHero)),
                    const SizedBox(height: 16),
                    Text(l10n.homeSerialNumber,
                        style: theme.textTheme.labelMedium
                            ?.copyWith(color: mutedOnHero)),
                    Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Flexible(
                          child: SelectableText(
                            device.deviceTrueSerial,
                            style: theme.textTheme.bodyLarge
                                ?.copyWith(color: onHero),
                          ),
                        ),
                        const SizedBox(width: 4),
                        IconButton(
                          visualDensity: VisualDensity.compact,
                          iconSize: 18,
                          tooltip: l10n.commonCopy,
                          color: mutedOnHero,
                          onPressed: () => _copySerial(context),
                          icon: const Icon(Icons.copy_rounded),
                        ),
                      ],
                    ),
                    const SizedBox(height: 16),
                    _InfoPill(
                      icon: device.isWireless ? Icons.wifi : Icons.usb,
                      label: connection,
                      foreground: onHero,
                      background: scheme.surface.withValues(alpha: 0.45),
                    ),
                  ],
                );
                final artwork = _HeadsetArtwork(device: device);
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
      ),
    );
  }

  void _copySerial(BuildContext context) {
    Clipboard.setData(ClipboardData(text: device.deviceTrueSerial));
    final messenger = ScaffoldMessenger.maybeOf(context);
    messenger?.showSnackBar(
      SnackBar(content: Text(AppLocalizations.of(context).copiedToClipboard)),
    );
  }
}

class _HeadsetArtwork extends StatelessWidget {
  const _HeadsetArtwork({required this.device});

  final DeviceState device;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      height: 200,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        gradient: RadialGradient(colors: [
          scheme.surface.withValues(alpha: 0.35),
          scheme.surface.withValues(alpha: 0),
        ]),
      ),
      child: Image.asset(
        'assets/png/headset/${device.productName}.png',
        width: 260,
        height: 200,
        fit: BoxFit.contain,
        excludeFromSemantics: true,
        errorBuilder: (context, error, stackTrace) => Image.asset(
          'assets/png/headset/unknown.png',
          width: 260,
          height: 200,
          fit: BoxFit.contain,
          excludeFromSemantics: true,
        ),
      ),
    );
  }
}

/// Battery levels for the headset and both controllers, grouped in one card.
class _BatteryCard extends StatelessWidget {
  const _BatteryCard({required this.device});

  final DeviceState device;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final entries = [
      (
        title: l10n.headset,
        asset: 'headset',
        level: device.batteryLevel,
        status: null,
        isCharging: device.isCharging == true,
        isActive: true,
      ),
      (
        title: l10n.leftController,
        asset: 'controller_l',
        level: device.leftController?.batteryLevel,
        status: device.controllerStatusString(context, device.leftController),
        isCharging: false,
        isActive: device.leftController?.status is ControllerStatusActive,
      ),
      (
        title: l10n.rightController,
        asset: 'controller_r',
        level: device.rightController?.batteryLevel,
        status: device.controllerStatusString(context, device.rightController),
        isCharging: false,
        isActive: device.rightController?.status is ControllerStatusActive,
      ),
    ];

    return Card(
      margin: EdgeInsets.zero,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(_HomeMetrics.cardRadius),
      ),
      child: Padding(
        padding: const EdgeInsets.all(_HomeMetrics.cardPadding),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            CardHeader(
                icon: Icons.battery_full_rounded, title: l10n.batteryLabel),
            const SizedBox(height: 16),
            LayoutBuilder(
              builder: (context, constraints) {
                final sideBySide = constraints.maxWidth >= 520;
                final tiles = [
                  for (final entry in entries)
                    _BatteryTile(
                      title: entry.title,
                      asset: entry.asset,
                      level: entry.level,
                      status: entry.status,
                      isCharging: entry.isCharging,
                      isActive: entry.isActive,
                    ),
                ];
                if (!sideBySide) {
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (var i = 0; i < tiles.length; i++) ...[
                        if (i > 0) const Divider(height: 24),
                        tiles[i],
                      ],
                    ],
                  );
                }
                return IntrinsicHeight(
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (var i = 0; i < tiles.length; i++) ...[
                        if (i > 0) const VerticalDivider(width: 33),
                        Expanded(child: tiles[i]),
                      ],
                    ],
                  ),
                );
              },
            ),
          ],
        ),
      ),
    );
  }
}

class _BatteryTile extends StatelessWidget {
  const _BatteryTile({
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
    final available = isActive && value != null;
    final indicatorColor = !available
        ? scheme.outlineVariant
        : value <= 20
            ? scheme.error
            : scheme.primary;
    final subtitle =
        isCharging ? l10n.chargingLabel : status ?? l10n.batteryLabel;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            SvgPicture.asset(
              'assets/svg/$asset.svg',
              width: 20,
              height: 20,
              colorFilter: ColorFilter.mode(
                  available ? scheme.onSurfaceVariant : scheme.outline,
                  BlendMode.srcIn),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                title,
                style: theme.textTheme.labelLarge
                    ?.copyWith(color: scheme.onSurfaceVariant),
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Flexible(
              child: Text(
                value == null ? '—' : '$value%',
                style: theme.textTheme.headlineSmall?.copyWith(
                  color: available ? scheme.onSurface : scheme.outline,
                ),
              ),
            ),
            if (isCharging)
              Padding(
                padding: const EdgeInsets.only(left: 4),
                child: Icon(Icons.bolt,
                    key: const ValueKey('headset-charging-badge'),
                    size: 20,
                    color: scheme.primary),
              ),
          ],
        ),
        const SizedBox(height: 10),
        LinearProgressIndicator(
          value: available ? value / 100 : 0,
          semanticsLabel: '$title: ${l10n.batteryLabel}',
          minHeight: 6,
          borderRadius: BorderRadius.circular(3),
          color: indicatorColor,
          backgroundColor: scheme.surfaceContainerHighest,
        ),
        const SizedBox(height: 8),
        Text(
          subtitle,
          style: theme.textTheme.bodySmall
              ?.copyWith(color: scheme.onSurfaceVariant),
          overflow: TextOverflow.ellipsis,
        ),
      ],
    );
  }
}

class _StorageCard extends StatelessWidget {
  const _StorageCard({required this.device});

  final DeviceState device;

  String _formatSize(int bytes) => FileSize.fromBytes(bytes).toString(
        unit: Unit.auto(size: bytes, baseType: BaseType.metric),
        decimals: 1,
      );

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final l10n = AppLocalizations.of(context);
    final space = device.spaceInfo;
    final total = space?.total.toInt() ?? 0;
    final available = (space?.available.toInt() ?? 0).clamp(0, total);
    final usedFraction = total > 0 ? 1 - available / total : 0.0;
    final isLow = usedFraction >= 0.9;

    return Card(
      margin: EdgeInsets.zero,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(_HomeMetrics.cardRadius),
      ),
      child: Padding(
        padding: const EdgeInsets.all(_HomeMetrics.cardPadding),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            CardHeader(
                icon: Icons.storage_rounded,
                title: l10n.settingsSectionStorage),
            const SizedBox(height: 20),
            Wrap(
              crossAxisAlignment: WrapCrossAlignment.end,
              spacing: 8,
              children: [
                Text(
                  total > 0 ? _formatSize(available) : '—',
                  style: theme.textTheme.headlineLarge?.copyWith(
                    color: isLow ? scheme.error : scheme.onSurface,
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: Text(l10n.homeFreeSpace,
                      style: theme.textTheme.bodyMedium
                          ?.copyWith(color: scheme.onSurfaceVariant)),
                ),
              ],
            ),
            const SizedBox(height: 16),
            LinearProgressIndicator(
              value: usedFraction,
              semanticsLabel: l10n.detailsStorageUsage,
              minHeight: 8,
              borderRadius: BorderRadius.circular(4),
              color: isLow ? scheme.error : scheme.primary,
              backgroundColor: scheme.surfaceContainerHighest,
            ),
            const SizedBox(height: 12),
            Text(
              total > 0
                  ? '${l10n.detailsTotal} ${_formatSize(total)}'
                  : l10n.deviceStorageStatusUnknown,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: scheme.onSurfaceVariant),
            ),
          ],
        ),
      ),
    );
  }
}

class _DeviceMenuButton extends StatelessWidget {
  const _DeviceMenuButton({required this.color});

  final Color color;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return PopupMenuButton<String>(
      tooltip: l10n.deviceActionsTooltip,
      icon: const Icon(Icons.more_vert),
      iconColor: color,
      itemBuilder: (context) => [
        PopupMenuItem(
          value: 'powerOff',
          child: ListTile(
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.power_settings_new),
            title: Text(l10n.powerOffMenu),
          ),
        ),
        PopupMenuItem(
          value: 'reboot',
          child: ListTile(
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.restart_alt),
            title: Text(l10n.rebootMenu),
          ),
        ),
      ],
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
    );
  }

  void _confirmAndSend(BuildContext context,
      {required String title,
      required String message,
      required AdbCommand command}) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        icon: const Icon(Icons.warning_amber_rounded),
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
      builder: (context) {
        final l10n = AppLocalizations.of(context);
        return SimpleDialog(
          title: Text(l10n.rebootOptions),
          contentPadding: const EdgeInsets.only(bottom: 16),
          children: [
            for (final option in [
              (
                value: 'normal',
                icon: Icons.restart_alt,
                label: l10n.rebootNormal
              ),
              (
                value: 'bootloader',
                icon: Icons.developer_board,
                label: l10n.rebootBootloader
              ),
              (
                value: 'recovery',
                icon: Icons.build_circle_outlined,
                label: l10n.rebootRecovery
              ),
              (
                value: 'fastboot',
                icon: Icons.bolt_outlined,
                label: l10n.rebootFastboot
              ),
            ])
              ListTile(
                leading: Icon(option.icon),
                title: Text(option.label),
                onTap: () => Navigator.pop(context, option.value),
              ),
          ],
        );
      },
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
