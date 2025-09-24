import 'package:flutter/material.dart';
import 'package:flutter_svg/svg.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';
import '../device/device_actions.dart';
import '../common/no_device_connected_indicator.dart';

class Home extends StatelessWidget {
  const Home({super.key});

  Widget _buildDeviceStatus(
    BuildContext context, {
    required String title,
    String? status,
    required int batteryLevel,
    required Widget icon,
    bool isDimmed = false,
    AppLocalizations? l10n,
  }) {
    return Tooltip(
      message: '$title\n'
          '${status != null ? '${(l10n ?? AppLocalizations.of(context)).statusLabel}: $status\n' : ''}'
          '${(l10n ?? AppLocalizations.of(context)).batteryLabel}: $batteryLevel%',
      child: Opacity(
        opacity: isDimmed ? 0.5 : 1.0,
        child: Column(
          children: [
            icon,
            const SizedBox(height: 8),
            Container(
              width: 30,
              height: 4,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(2),
                color: Colors.grey[300],
              ),
              child: FractionallySizedBox(
                alignment: Alignment.centerLeft,
                widthFactor: batteryLevel / 100,
                child: Container(
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(2),
                    color: Colors.green,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        final l10n = AppLocalizations.of(context);
        if (!deviceState.isConnected) {
          return const NoDeviceConnectedIndicator();
        }

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: 350,
              child: Card(
                child: Padding(
                  padding: const EdgeInsets.all(16.0),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          Text(
                            l10n.deviceTitle,
                            style: Theme.of(context).textTheme.titleMedium,
                          ),
                          _DeviceMenuButton(),
                        ],
                      ),
                      Center(
                        child: Column(
                          children: [
                            Image.asset(
                              'assets/png/headset/${deviceState.productName.isEmpty ? "unknown" : deviceState.productName}.png',
                              width: 192,
                              height: 164,
                              cacheWidth: 192,
                              cacheHeight: 192,
                              fit: BoxFit.fitWidth,
                              errorBuilder: (context, error, stackTrace) {
                                return Image.asset(
                                  'assets/png/headset/unknown.png',
                                  width: 192,
                                  height: 164,
                                  cacheWidth: 192,
                                  cacheHeight: 192,
                                  fit: BoxFit.fitWidth,
                                );
                              },
                            ),
                            Text(
                              deviceState.deviceName,
                              style: const TextStyle(
                                fontSize: 16,
                                fontWeight: FontWeight.bold,
                              ),
                            ),
                            const SizedBox(height: 8),
                            Row(
                              mainAxisAlignment: MainAxisAlignment.center,
                              children: [
                                _buildDeviceStatus(
                                  context,
                                  title: l10n.leftController,
                                  status: deviceState.controllerStatusString(
                                      context, deviceState.leftController),
                                  batteryLevel:
                                      deviceState.controllerBatteryLevel(
                                          deviceState.leftController),
                                  icon: SvgPicture.asset(
                                      'assets/svg/controller_l.svg'),
                                  isDimmed:
                                      deviceState.leftController?.status !=
                                          ControllerStatus.active,
                                  l10n: l10n,
                                ),
                                const SizedBox(width: 12),
                                _buildDeviceStatus(
                                  context,
                                  title: l10n.headset,
                                  batteryLevel: deviceState.batteryLevel,
                                  icon: SvgPicture.asset(
                                      'assets/svg/headset.svg'),
                                  l10n: l10n,
                                ),
                                const SizedBox(width: 12),
                                _buildDeviceStatus(
                                  context,
                                  title: l10n.rightController,
                                  status: deviceState.controllerStatusString(
                                      context, deviceState.rightController),
                                  batteryLevel:
                                      deviceState.controllerBatteryLevel(
                                          deviceState.rightController),
                                  icon: SvgPicture.asset(
                                      'assets/svg/controller_r.svg'),
                                  isDimmed:
                                      deviceState.rightController?.status !=
                                          ControllerStatus.active,
                                  l10n: l10n,
                                ),
                              ],
                            )
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            const SizedBox(height: 12),
            const DeviceActionsCard(),
          ],
        );
      },
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
