import 'package:flutter/material.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../../src/bindings/signals/signals.dart' as signals;
import 'package:toastification/toastification.dart';
import '../../providers/device_state.dart';
import '../../providers/adb_state.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/task_state.dart';
import '../../utils/utils.dart';
import '../dialogs/task_list_dialog.dart';
import '../dialogs/connection_diagnostics_dialog.dart';
import 'animated_refresh_button.dart';

class StatusBar extends StatelessWidget {
  const StatusBar({super.key});

  Widget _buildConnectionStatus(
    BuildContext context,
    AdbStateProvider adbState,
    AppLocalizations l10n,
  ) {
    return Tooltip(
      message: l10n.statusAdb(adbState.statusDescription(context)),
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: () {
            showDialog(
              context: context,
              builder: (context) => const ConnectionDiagnosticsDialog(),
            );
          },
          borderRadius: BorderRadius.circular(20),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 3),
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
          ),
        ),
      ),
    );
  }

  Widget _buildDeviceInfo(
    BuildContext context,
    DeviceState deviceState,
    AppLocalizations l10n,
  ) {
    final isWireless = deviceState.isWireless;
    final name = deviceState.deviceName;
    final addr = deviceState.deviceSerial;
    final trueSerial = deviceState.deviceTrueSerial;

    final tooltip = isWireless
        ? l10n.statusDeviceInfoWireless(name, addr, trueSerial)
        : l10n.statusDeviceInfo(name, trueSerial);

    final label = isWireless ? '$name · $addr ($trueSerial)' : '$name · $addr';

    return _DeviceSwitcherLabel(
      tooltip: tooltip,
      label: label,
    );
  }

  Widget _buildBatteryStatus(
    BuildContext context,
    DeviceState deviceState,
    AppLocalizations l10n,
  ) {
    return Tooltip(
      message: '${l10n.headset}: ${deviceState.batteryLevel}%\n'
          '${l10n.leftController}: ${deviceState.controllerBatteryLevel(deviceState.leftController)}%\n'
          '${l10n.rightController}: ${deviceState.controllerBatteryLevel(deviceState.rightController)}%',
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: () async {
            final key = DateTime.now().millisecondsSinceEpoch.toString();
            signals.AdbRequest(
              command: const signals.AdbCommandGetBatteryDump(),
              commandKey: key,
            ).sendSignalToRust();

            try {
              final event = await signals.BatteryDumpResponse.rustSignalStream
                  .firstWhere((e) => e.message.commandKey == key)
                  .timeout(const Duration(seconds: 10));
              // Copy to clipboard
              if (context.mounted) {
                copyToClipboard(context, event.message.dump,
                    title: l10n.commonSuccess,
                    description: l10n.batteryDumpCopied);
              } else {
                Clipboard.setData(ClipboardData(text: event.message.dump));
              }
            } catch (_) {
              if (!context.mounted) return;
              toastification.show(
                type: ToastificationType.error,
                style: ToastificationStyle.flat,
                title: Text(l10n.commonError),
                description: Text(l10n.batteryDumpFailed),
                autoCloseDuration: const Duration(seconds: 3),
                backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
                borderSide: BorderSide.none,
                alignment: Alignment.bottomRight,
              );
            }
          },
          child: Row(
            children: [
              const Icon(Icons.battery_full, size: 16),
              const SizedBox(width: 2),
              Text('${deviceState.batteryLevel}%'),
              const SizedBox(width: 8),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildStorageStatus(
    BuildContext context,
    DeviceState deviceState,
    AppLocalizations l10n,
  ) {
    if (deviceState.spaceInfo == null) {
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
      message: l10n.storageTooltip(available, total),
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

  Widget _buildRefreshButton(DeviceState deviceState, AppLocalizations l10n) {
    return AnimatedRefreshButton(
      deviceState: deviceState,
      tooltip: l10n.refreshAllData,
    );
  }

  Widget _buildTaskStatus(
    BuildContext context,
    TaskState taskState,
    AppLocalizations l10n,
  ) {
    final activeTasks = taskState.activeTasks;
    final recentTasks = taskState.recentTasks;
    final hasActiveTasks = activeTasks.isNotEmpty;
    final hasRecentTasks = recentTasks.isNotEmpty;
    final progress = hasActiveTasks ? activeTasks.first.stepProgress : null;
    final failedCount = taskState.failedTaskCount;

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
              Text(l10n.activeTasks(activeTasks.length)),
            ] else ...[
              Icon(
                failedCount > 0
                    ? Icons.error_outline
                    : hasRecentTasks
                        ? Icons.task_alt
                        : Icons.check_circle_outline,
                size: 16,
                color: failedCount > 0
                    ? Theme.of(context).colorScheme.error
                    : Theme.of(context).colorScheme.onSurface.withValues(
                          alpha: hasRecentTasks ? 1.0 : 0.5,
                        ),
              ),
              const SizedBox(width: 8),
              if (failedCount > 0) ...[
                Text(
                  l10n.failedTasks(failedCount),
                  style: TextStyle(
                    color: Theme.of(context).colorScheme.error,
                  ),
                ),
              ] else ...[
                Text(
                  l10n.viewTasks,
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
        final l10n = AppLocalizations.of(context);
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
              _buildConnectionStatus(context, adbState, l10n),
              if (deviceState.isConnected) ...[
                _buildDeviceInfo(context, deviceState, l10n),
                _buildBatteryStatus(context, deviceState, l10n),
                _buildStorageStatus(context, deviceState, l10n),
                _buildRefreshButton(deviceState, l10n),
              ] else ...[
                const SizedBox(width: 8),
                Text(l10n.noDeviceConnected),
              ],
              // Right side
              const Spacer(),
              Consumer<TaskState>(
                builder: (context, taskState, _) =>
                    _buildTaskStatus(context, taskState, l10n),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _DeviceSwitcherLabel extends StatefulWidget {
  final String tooltip;
  final String label;

  const _DeviceSwitcherLabel({required this.tooltip, required this.label});

  @override
  State<_DeviceSwitcherLabel> createState() => _DeviceSwitcherLabelState();
}

class _DeviceSwitcherLabelState extends State<_DeviceSwitcherLabel>
    with SingleTickerProviderStateMixin {
  final MenuController _menuController = MenuController();
  late AnimationController _animationController;
  late Animation<double> _fadeAnimation;

  @override
  void initState() {
    super.initState();
    _animationController = AnimationController(
      duration: const Duration(milliseconds: 200),
      vsync: this,
    );
    _fadeAnimation = CurvedAnimation(
      parent: _animationController,
      curve: Curves.easeOut,
    );
  }

  @override
  void dispose() {
    _animationController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final adb = context.watch<AdbStateProvider>();
    final device = context.watch<DeviceState>();
    final devices = adb.availableDevices;
    final anyWireless = devices.any((e) => e.isWireless);
    final current = device.isConnected &&
            devices.any((e) => e.serial == device.deviceSerial)
        ? device.deviceSerial
        : null;

    // Menu width is hardcoded because Flutter's MenuAnchor doesn't support
    // intrinsic width calculation well, and using LayoutBuilder causes issues
    // with menu positioning. The values accommodate typical device serials.
    final menuWidth = anyWireless ? 400.0 : 360.0;
    List<Widget> buildMenuItems() {
      final l10n = AppLocalizations.of(context);
      final header = <Widget>[
        SizedBox(
          width: menuWidth,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(12, 10, 12, 6),
            child: Text(
              l10n.diagnosticsDevices,
              style: Theme.of(context).textTheme.titleSmall,
            ),
          ),
        ),
        SizedBox(width: menuWidth, child: const Divider(height: 8)),
      ];

      if (devices.isEmpty) {
        return [
          ...header,
          _AnimatedMenuItem(
            animation: _fadeAnimation,
            delay: 0,
            child: SizedBox(
              width: menuWidth,
              child: MenuItemButton(
                onPressed: null,
                leadingIcon: const Icon(Icons.devices_outlined, size: 18),
                child: Text(l10n.noDeviceConnected),
              ),
            ),
          ),
        ];
      }
      return [
        ...header,
        ...devices.asMap().entries.map((mapEntry) {
          final index = mapEntry.key;
          final entry = mapEntry.value;
          final serial = entry.serial;
          final isCurrent = serial == current;
          final isWireless = entry.isWireless;
          final isReady = entry.state == signals.AdbBriefState.device;
          final titleText = (entry.name != null && entry.name!.isNotEmpty)
              ? entry.name!
              : serial;
          final subtitle = StringBuffer()
            ..write(serial)
            ..write(isWireless && entry.trueSerial != null
                ? ' • ${entry.trueSerial}'
                : '')
            ..write(' • ')
            ..write(isWireless
                ? l10n.settingsConnectionWireless
                : l10n.settingsConnectionUsb);
          if (!isReady) {
            subtitle.write(' • ');
            final stateLabel = () {
              switch (entry.state) {
                case signals.AdbBriefState.unauthorized:
                  return l10n.statusAdbDeviceUnauthorized;
                case signals.AdbBriefState.offline:
                  return l10n.statusAdbStateOffline;
                case signals.AdbBriefState.bootloader:
                  return l10n.statusAdbStateBootloader;
                case signals.AdbBriefState.recovery:
                  return l10n.statusAdbStateRecovery;
                case signals.AdbBriefState.noPermissions:
                  return l10n.statusAdbStateNoPermissions;
                case signals.AdbBriefState.sideload:
                  return l10n.statusAdbStateSideload;
                case signals.AdbBriefState.authorizing:
                  return l10n.statusAdbStateAuthorizing;
                case signals.AdbBriefState.unknown:
                  return l10n.statusAdbStateUnknown;
                default:
                  return '';
              }
            }();
            if (!isReady && stateLabel.isNotEmpty) subtitle.write(stateLabel);
          }

          return _AnimatedMenuItem(
            animation: _fadeAnimation,
            delay: index,
            child: SizedBox(
                width: menuWidth,
                child: MenuItemButton(
                  onPressed: !isReady
                      ? null
                      : () {
                          if (serial != current) {
                            signals.AdbRequest(
                              command:
                                  signals.AdbCommandConnectTo(value: serial),
                              commandKey: 'select-device',
                            ).sendSignalToRust();
                          }
                          if (_menuController.isOpen) _menuController.close();
                        },
                  leadingIcon: Icon(
                    isWireless ? Icons.wifi_tethering : Icons.usb,
                    size: 18,
                  ),
                  trailingIcon:
                      isCurrent ? const Icon(Icons.check, size: 18) : null,
                  style: const ButtonStyle(
                    padding: WidgetStatePropertyAll(
                      EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                    ),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(titleText),
                      const SizedBox(height: 2),
                      Text(
                        subtitle.toString(),
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                            color:
                                Theme.of(context).colorScheme.onSurfaceVariant),
                      ),
                    ],
                  ),
                )),
          );
        }),
      ];
    }

    return Padding(
      padding: const EdgeInsets.only(right: 8),
      child: Tooltip(
        message: widget.tooltip,
        child: MenuAnchor(
          controller: _menuController,
          style: MenuStyle(
            backgroundColor:
                WidgetStatePropertyAll(Theme.of(context).colorScheme.surface),
            elevation: const WidgetStatePropertyAll(8),
            shape: WidgetStatePropertyAll(
                RoundedRectangleBorder(borderRadius: BorderRadius.circular(8))),
            alignment: Alignment.topCenter,
          ),
          alignmentOffset: Offset(-menuWidth / 2, 12),
          menuChildren: buildMenuItems(),
          builder: (context, controller, child) {
            // Trigger animation when menu opens
            if (controller.isOpen &&
                _animationController.status == AnimationStatus.dismissed) {
              WidgetsBinding.instance.addPostFrameCallback((_) {
                _animationController.forward();
              });
            } else if (!controller.isOpen) {
              _animationController.reverse();
            }

            return Material(
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(6),
              ),
              clipBehavior: Clip.antiAlias,
              color: Theme.of(context).colorScheme.surfaceContainerHigh,
              elevation: 8,
              child: InkWell(
                splashFactory: InkSplash.splashFactory,
                onTap: () {
                  controller.isOpen ? controller.close() : controller.open();
                },
                child: Padding(
                  padding: const EdgeInsets.only(left: 4),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      // Icon(Icons.devices_outlined,
                      //     size: 16,
                      //     color:
                      //         Theme.of(context).colorScheme.onSurfaceVariant),
                      // const SizedBox(width: 6),
                      Text(
                        widget.label,
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.onSurface,
                        ),
                      ),
                      const SizedBox(width: 4),
                      Icon(Icons.arrow_drop_down,
                          size: 18,
                          color:
                              Theme.of(context).colorScheme.onSurfaceVariant),
                    ],
                  ),
                ),
              ),
            );
          },
        ),
      ),
    );
  }
}

class _AnimatedMenuItem extends StatelessWidget {
  final Animation<double> animation;
  final int delay;
  final Widget child;

  const _AnimatedMenuItem({
    required this.animation,
    required this.delay,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    final delayedAnimation = CurvedAnimation(
      parent: animation,
      curve: Interval(
        delay * 0.05,
        1.0,
        curve: Curves.easeOut,
      ),
    );

    return FadeTransition(
      opacity: delayedAnimation,
      child: SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(0, -0.2),
          end: Offset.zero,
        ).animate(delayedAnimation),
        child: child,
      ),
    );
  }
}
