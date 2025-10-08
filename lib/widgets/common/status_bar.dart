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
    return _DeviceSwitcherLabel(
      tooltip: l10n.statusDeviceInfo(
          deviceState.deviceName, deviceState.deviceSerial),
      label: '${deviceState.deviceName} · ${deviceState.deviceSerial}',
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
    return _AnimatedRefreshButton(deviceState: deviceState, l10n: l10n);
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
                hasRecentTasks ? Icons.task_alt : Icons.check_circle_outline,
                size: 16,
                color: Theme.of(context).colorScheme.onSurface.withValues(
                      alpha: hasRecentTasks ? 1.0 : 0.5,
                    ),
              ),
              if (hasRecentTasks || true) ...[
                // TODO: show task error count badge
                // TODO: show task count?
                // TODO: add recent task clearing
                const SizedBox(width: 8),
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

class _AnimatedRefreshButton extends StatefulWidget {
  final DeviceState deviceState;
  final AppLocalizations l10n;

  const _AnimatedRefreshButton({required this.deviceState, required this.l10n});

  @override
  State<_AnimatedRefreshButton> createState() => _AnimatedRefreshButtonState();
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
    final current = device.isConnected &&
            devices.any((e) => e.serial == device.deviceSerial)
        ? device.deviceSerial
        : null;

    const double menuWidth = 360;
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
          final isUnauthorized =
              entry.state == signals.AdbBriefState.unauthorized;
          final titleText = (entry.name != null && entry.name!.isNotEmpty)
              ? entry.name!
              : serial;
          final subtitle = StringBuffer()
            ..write(serial)
            ..write(' • ')
            ..write(isWireless
                ? l10n.settingsConnectionWireless
                : l10n.settingsConnectionUsb);
          if (isUnauthorized) {
            subtitle.write(' • ');
            subtitle.write(l10n.statusAdbDeviceUnauthorized);
          }

          return _AnimatedMenuItem(
            animation: _fadeAnimation,
            delay: index,
            child: SizedBox(
                width: menuWidth,
                child: MenuItemButton(
                  onPressed: isUnauthorized
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
          alignmentOffset: const Offset(-menuWidth / 2, 12),
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
              elevation: 8,
              surfaceTintColor: Colors.white,
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

class _AnimatedRefreshButtonState extends State<_AnimatedRefreshButton>
    with TickerProviderStateMixin {
  late AnimationController _rotationController;
  late AnimationController _successController;
  late Animation<double> _rotation;
  late Animation<double> _checkmarkScale;

  bool _isRefreshing = false;
  bool _showCheckmark = false;
  DateTime? _lastDeviceUpdate;

  @override
  void initState() {
    super.initState();

    // Animation for spinning refresh icon
    _rotationController = AnimationController(
      duration: const Duration(milliseconds: 1000),
      vsync: this,
    );
    _rotation = Tween<double>(begin: 0, end: 1).animate(_rotationController);

    // Animation for checkmark appearance
    _successController = AnimationController(
      duration: const Duration(milliseconds: 300),
      vsync: this,
    );
    _checkmarkScale = Tween<double>(begin: 0, end: 1).animate(
      CurvedAnimation(parent: _successController, curve: Curves.elasticOut),
    );

    // Listen for device updates to trigger success animation
    widget.deviceState.addListener(_onDeviceStateChanged);
    _lastDeviceUpdate = DateTime.now();
  }

  @override
  void dispose() {
    widget.deviceState.removeListener(_onDeviceStateChanged);
    _rotationController.dispose();
    _successController.dispose();
    super.dispose();
  }

  void _onDeviceStateChanged() {
    if (_isRefreshing && widget.deviceState.device != null) {
      final now = DateTime.now();
      // Only trigger success animation if device was updated recently after refresh
      if (_lastDeviceUpdate != null &&
          now.difference(_lastDeviceUpdate!).inSeconds < 2) {
        _showSuccess();
      }
    }
  }

  void _onRefreshPressed() {
    if (_isRefreshing || _showCheckmark) return;

    setState(() {
      _isRefreshing = true;
      _showCheckmark = false;
    });

    _rotationController.repeat();
    _lastDeviceUpdate = DateTime.now();

    signals.AdbRequest(
            command: signals.AdbCommandRefreshDevice(), commandKey: '')
        .sendSignalToRust();

    // Fallback: stop spinning after 5 seconds
    Future.delayed(const Duration(seconds: 5), () {
      if (_isRefreshing && mounted) {
        _stopRefreshing();
      }
    });
  }

  void _showSuccess() {
    if (!mounted) return;

    _rotationController.stop();
    setState(() {
      _isRefreshing = false;
      _showCheckmark = true;
    });

    _successController.forward().then((_) {
      Future.delayed(const Duration(milliseconds: 800), () {
        if (mounted) {
          _successController.reverse().then((_) {
            if (mounted) {
              setState(() {
                _showCheckmark = false;
              });
            }
          });
        }
      });
    });
  }

  void _stopRefreshing() {
    if (!mounted) return;

    _rotationController.stop();
    setState(() {
      _isRefreshing = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.l10n.refreshAllData,
      child: SizedBox(
        width: 23,
        height: 23,
        child: IconButton(
          onPressed: _onRefreshPressed,
          icon: AnimatedSwitcher(
            duration: const Duration(milliseconds: 200),
            child: _showCheckmark
                ? ScaleTransition(
                    key: const Key('checkmark'),
                    scale: _checkmarkScale,
                    child: Icon(
                      Icons.check,
                      color: Colors.green,
                      size: 16,
                    ),
                  )
                : _isRefreshing
                    ? RotationTransition(
                        key: const Key('spinning'),
                        turns: _rotation,
                        child: const Icon(
                          Icons.refresh,
                          size: 16,
                        ),
                      )
                    : const Icon(
                        Icons.refresh,
                        key: Key('idle'),
                        size: 16,
                      ),
          ),
          padding: EdgeInsets.zero,
          iconSize: 16,
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
