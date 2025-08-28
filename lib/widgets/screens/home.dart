import 'package:flutter/material.dart';
import 'package:flutter_svg/svg.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart';
import '../device/device_actions.dart';

class Home extends StatelessWidget {
  const Home({super.key});

  Widget _buildDeviceStatus({
    required String title,
    String? status,
    required int batteryLevel,
    required Widget icon,
    bool isDimmed = false,
  }) {
    return Tooltip(
      message:
          '$title\n${status != null ? 'Status: $status\n' : ''}Battery: $batteryLevel%',
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
        if (!deviceState.isConnected) {
          return const Center(
            child: Text(
              'No device connected',
              style: TextStyle(fontSize: 18),
            ),
          );
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
                            'Device',
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
                                  title: 'Left Controller',
                                  status: deviceState.controllerStatusString(
                                      deviceState.leftController),
                                  batteryLevel:
                                      deviceState.controllerBatteryLevel(
                                          deviceState.leftController),
                                  icon: SvgPicture.asset(
                                      'assets/svg/controller_l.svg'),
                                  isDimmed:
                                      deviceState.leftController?.status !=
                                          ControllerStatus.active,
                                ),
                                const SizedBox(width: 12),
                                _buildDeviceStatus(
                                  title: 'Headset',
                                  batteryLevel: deviceState.batteryLevel,
                                  icon: SvgPicture.asset(
                                      'assets/svg/headset.svg'),
                                ),
                                const SizedBox(width: 12),
                                _buildDeviceStatus(
                                  title: 'Right Controller',
                                  status: deviceState.controllerStatusString(
                                      deviceState.rightController),
                                  batteryLevel:
                                      deviceState.controllerBatteryLevel(
                                          deviceState.rightController),
                                  icon: SvgPicture.asset(
                                      'assets/svg/controller_r.svg'),
                                  isDimmed:
                                      deviceState.rightController?.status !=
                                          ControllerStatus.active,
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
    return PopupMenuButton<String>(
      tooltip: 'Device actions',
      onSelected: (value) async {
        switch (value) {
          case 'powerOff':
            _confirmAndSend(
              context,
              title: 'Power off device',
              message: 'Are you sure you want to power off the device?',
              command: const AdbCommandReboot(value: RebootMode.powerOff),
            );
            break;
          case 'reboot':
            _showRebootOptions(context);
            break;
        }
      },
      itemBuilder: (context) => [
        const PopupMenuItem(value: 'powerOff', child: Text('Power off...')),
        const PopupMenuItem(value: 'reboot', child: Text('Reboot...')),
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
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Confirm')),
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
        title: const Text('Reboot options'),
        children: [
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'normal'),
            child: const Text('Normal'),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'bootloader'),
            child: const Text('Bootloader'),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'recovery'),
            child: const Text('Recovery'),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, 'fastboot'),
            child: const Text('Fastboot'),
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
          title: 'Reboot device',
          message: 'Reboot the device now?',
          command: const AdbCommandReboot(value: RebootMode.normal),
        );
        break;
      case 'bootloader':
        _confirmAndSend(
          context,
          title: 'Reboot to bootloader',
          message: 'Reboot the device to bootloader?',
          command: const AdbCommandReboot(value: RebootMode.bootloader),
        );
        break;
      case 'recovery':
        _confirmAndSend(
          context,
          title: 'Reboot to recovery',
          message: 'Reboot the device to recovery?',
          command: const AdbCommandReboot(value: RebootMode.recovery),
        );
        break;
      case 'fastboot':
        _confirmAndSend(
          context,
          title: 'Reboot to fastboot',
          message: 'Reboot the device to fastboot?',
          command: const AdbCommandReboot(value: RebootMode.fastboot),
        );
        break;
    }
  }
}
