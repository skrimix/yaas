import 'package:flutter/material.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:provider/provider.dart';
import 'package:toastification/toastification.dart';
import '../providers/device_state.dart';
import '../providers/adb_state.dart';
import '../providers/task_state.dart';
import 'task_list_dialog.dart';

class StatusBar extends StatelessWidget {
  const StatusBar({super.key});

  Widget _buildConnectionStatus(AdbStateProvider adbState) {
    return Tooltip(
      message: 'ADB Status: ${adbState.statusDescription}',
      child: Row(
        children: [
          Container(
            width: 18,
            height: 18,
            decoration: BoxDecoration(
              color: adbState.connectionColor,
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
      return const Text('No device connection');
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
          const SizedBox(width: 8),
        ],
      ),
    );
  }

  Widget _buildTaskStatus(BuildContext context, TaskState taskState) {
    final activeTasks = taskState.activeTasks;
    final recentTasks = taskState.recentTasks;
    final hasActiveTasks = activeTasks.isNotEmpty;
    final hasRecentTasks = recentTasks.isNotEmpty;
    final progress = hasActiveTasks ? activeTasks.first.totalProgress : null;

    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: () {
          showDialog(
            context: context,
            builder: (context) => TaskListDialog(
              initialTabIndex: hasActiveTasks ? 0 : 1,
            ),
          );
        },
        child: Row(
          children: [
            if (hasActiveTasks) ...[
              SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(
                  value: progress,
                  strokeWidth: 2,
                ),
              ),
              const SizedBox(width: 8),
              Text(
                '${activeTasks.length} active task${activeTasks.length > 1 ? 's' : ''}',
              ),
            ] else ...[
              Icon(
                hasRecentTasks ? Icons.task_alt : Icons.check_circle_outline,
                size: 16,
                color: Theme.of(context).colorScheme.onSurface.withValues(
                      alpha: hasRecentTasks ? 1.0 : 0.5,
                    ),
              ),
              if (hasRecentTasks || true) ...[
                // TODO: decide how we want to show this
                const SizedBox(width: 8),
                Text(
                  'View tasks',
                  style: TextStyle(
                    color: Theme.of(context).colorScheme.onSurface,
                  ),
                ),
              ],
            ],
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer2<DeviceState, AdbStateProvider>(
      builder: (context, deviceState, adbState, _) {
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
              left: BorderSide(
                color: Theme.of(context).colorScheme.outlineVariant,
                width: 1,
              ),
            ),
          ),
          child: Row(
            children: [
              // Left side
              _buildConnectionStatus(adbState),
              _buildDeviceInfo(deviceState),
              _buildBatteryStatus(deviceState),
              _buildStorageStatus(deviceState),
              // TODO: Implement refresh button
              if (deviceState.isConnected) ...[
                Tooltip(
                  message: 'Refresh all data',
                  child: SizedBox(
                    width: 23,
                    height: 23,
                    child: IconButton(
                      onPressed: () {
                        toastification.show(
                          type: ToastificationType.info,
                          style: ToastificationStyle.flat,
                          title: const Text('Not implemented'),
                          description:
                              const Text('This feature is not implemented yet'),
                          autoCloseDuration: const Duration(seconds: 3),
                          backgroundColor:
                              Theme.of(context).colorScheme.surfaceContainer,
                          borderSide: BorderSide.none,
                          alignment: Alignment.bottomRight,
                        );
                      },
                      icon: const Icon(Icons.refresh),
                      padding: EdgeInsets.zero,
                      iconSize: 16,
                    ),
                  ),
                ),
              ],
              // Right side
              const Spacer(),
              Consumer<TaskState>(
                builder: (context, taskState, _) =>
                    _buildTaskStatus(context, taskState),
              ),
            ],
          ),
        );
      },
    );
  }
}
