import 'package:flutter/material.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:provider/provider.dart';
import '../providers/device_state.dart';

class StatusBar extends StatelessWidget {
  const StatusBar({super.key});

  Widget _buildConnectionStatus(bool isConnected) {
    return Tooltip(
      message: 'Status: ${isConnected ? 'Connected' : 'Disconnected'}',
      child: Row(
        children: [
          Container(
            width: 18,
            height: 18,
            decoration: BoxDecoration(
              color: isConnected ? Colors.green : Colors.red,
              shape: BoxShape.circle,
            ),
          ),
          const SizedBox(width: 4),
        ],
      ),
    );
  }

  Widget _buildDeviceInfo(DeviceState deviceState) {
    if (!deviceState.isConnected) {
      return const Text('No device connected');
    }
    return Tooltip(
      message:
          'Device: ${deviceState.deviceName}\nSerial: ${deviceState.deviceSerial}',
      child: Row(
        children: [
          Text(deviceState.deviceName),
          const SizedBox(width: 8),
        ],
      ),
    );
  }

  Widget _buildBatteryStatus(DeviceState deviceState) {
    if (!deviceState.isConnected) return const SizedBox.shrink();

    return Tooltip(
      message: 'Headset: ${deviceState.batteryLevel}%\n'
          'Left Controller: ${deviceState.controllerBatteryLevel(deviceState.leftController)}%\n'
          'Right Controller: ${deviceState.controllerBatteryLevel(deviceState.rightController)}%',
      child: Row(
        children: [
          const Icon(Icons.battery_full, size: 16),
          const SizedBox(width: 2),
          Text('${deviceState.batteryLevel}%'),
          const SizedBox(width: 8),
        ],
      ),
    );
  }

  Widget _buildStorageStatus(DeviceState deviceState) {
    if (!deviceState.isConnected || deviceState.spaceInfo == null) {
      return const SizedBox.shrink();
    }
    final spaceInfo = deviceState.spaceInfo;
    final total = FileSize.fromBytes(spaceInfo!.total.toInt()).toString(
      unit: Unit.auto(size: spaceInfo.total.toInt(), baseType: BaseType.metric),
      decimals: 2,
    );
    final available = FileSize.fromBytes(spaceInfo.available.toInt()).toString(
      unit: Unit.auto(
        size: spaceInfo.available.toInt(),
        baseType: BaseType.metric,
      ),
      decimals: 2,
    );

    return Tooltip(
      message: '$available free of $total',
      child: Row(
        children: [
          const Icon(Icons.sd_card, size: 16),
          const SizedBox(width: 2),
          Text(available),
        ],
      ),
    );
  }

  Widget _buildTaskStatus() {
    return const Text('Tasks: 0 running, 0 finished');
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<DeviceState>(
      builder: (context, deviceState, _) {
        return Container(
          height: 24,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.surfaceContainerLowest,
            border: Border(
              top: BorderSide(
                color: Theme.of(context).colorScheme.outlineVariant,
                width: 1,
              ),
            ),
          ),
          child: Row(
            children: [
              // Left side
              _buildConnectionStatus(deviceState.isConnected),
              _buildDeviceInfo(deviceState),
              _buildBatteryStatus(deviceState),
              _buildStorageStatus(deviceState),
              // Right side
              const Spacer(),
              _buildTaskStatus(),
            ],
          ),
        );
      },
    );
  }
}
