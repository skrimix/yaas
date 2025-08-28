import 'package:flutter/material.dart';
import 'package:flutter_svg/svg.dart';
import 'package:provider/provider.dart';
import '../../providers/device_state.dart';
import '../../src/bindings/bindings.dart';

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
                      Text(
                        'Device',
                        style: Theme.of(context).textTheme.titleMedium,
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
            )
          ],
        );
      },
    );
  }
}
