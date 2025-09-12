import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../../providers/adb_state.dart';
import '../../providers/device_state.dart';
import '../../providers/settings_state.dart';
import '../../src/bindings/signals/signals.dart' as signals;
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

enum _DiagLevel { ok, warn, error, info }

class ConnectionDiagnosticsDialog extends StatelessWidget {
  const ConnectionDiagnosticsDialog({super.key});

  Color _levelColor(_DiagLevel level) {
    switch (level) {
      case _DiagLevel.ok:
        return Colors.green;
      case _DiagLevel.warn:
        return Colors.amber;
      case _DiagLevel.error:
        return Colors.red;
      case _DiagLevel.info:
        return Colors.grey;
    }
  }

  IconData _levelIcon(_DiagLevel level) {
    switch (level) {
      case _DiagLevel.ok:
        return Icons.check_circle;
      case _DiagLevel.warn:
        return Icons.error_outline;
      case _DiagLevel.error:
        return Icons.cancel;
      case _DiagLevel.info:
        return Icons.info_outline;
    }
  }

  _DiagLevel _serverLevel(AdbState state) {
    if (state is AdbStateServerNotRunning) return _DiagLevel.error;
    if (state is AdbStateServerStarting) return _DiagLevel.warn;
    return _DiagLevel.ok;
  }

  _DiagLevel _devicesLevel(AdbState state) {
    if (state is AdbStateNoDevices) return _DiagLevel.error;
    if (state is AdbStateDevicesAvailable) return _DiagLevel.warn;
    if (state is AdbStateDeviceUnauthorized) return _DiagLevel.error;
    if (state is AdbStateDeviceConnected) return _DiagLevel.ok;
    return _DiagLevel.warn;
  }

  _DiagLevel _authLevel(AdbState state) {
    if (state is AdbStateDeviceUnauthorized) return _DiagLevel.error;
    if (state is AdbStateDeviceConnected) return _DiagLevel.ok;
    return _DiagLevel.info;
  }

  Widget _legend(AppLocalizations l10n) {
    Widget dot(Color c) => Container(
          width: 10,
          height: 10,
          decoration: BoxDecoration(color: c, shape: BoxShape.circle),
        );
    return Row(
      children: [
        Text(
          l10n.diagnosticsLegendTitle,
          style: const TextStyle(fontWeight: FontWeight.bold),
        ),
        const SizedBox(width: 12),
        dot(_levelColor(_DiagLevel.ok)),
        const SizedBox(width: 6),
        Text(l10n.diagnosticsLegendOk),
        const SizedBox(width: 12),
        dot(_levelColor(_DiagLevel.warn)),
        const SizedBox(width: 6),
        Text(l10n.diagnosticsLegendWarning),
        const SizedBox(width: 12),
        dot(_levelColor(_DiagLevel.error)),
        const SizedBox(width: 6),
        Text(l10n.diagnosticsLegendError),
      ],
    );
  }

  Widget _item(
    BuildContext context, {
    required _DiagLevel level,
    required String title,
    required String description,
    Widget? trailing,
  }) {
    final color = _levelColor(level);
    return ListTile(
      leading: Icon(_levelIcon(level), color: color),
      title: Text(title),
      subtitle: Text(description),
      trailing: trailing,
      contentPadding: const EdgeInsets.symmetric(horizontal: 8),
      dense: false,
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final adb = context.watch<AdbStateProvider>();
    final device = context.watch<DeviceState>();
    final settings = context.watch<SettingsState>();

    final adbState = adb.state;

    final serverLevel = _serverLevel(adbState);
    final devicesLevel = _devicesLevel(adbState);
    final authLevel = _authLevel(adbState);

    String serverDesc() {
      if (adbState is AdbStateServerNotRunning) {
        return l10n.diagnosticsServerNotRunningDesc;
      }
      if (adbState is AdbStateServerStarting) {
        return l10n.diagnosticsServerStartingDesc;
      }
      return l10n.diagnosticsConnectedDesc; // server up
    }

    String devicesDesc() {
      if (adbState is AdbStateNoDevices) return l10n.diagnosticsNoDevicesDesc;
      if (adbState is AdbStateDevicesAvailable) {
        final count = adbState.value.length;
        return l10n.diagnosticsDevicesAvailableDesc(count);
      }
      if (adbState is AdbStateDeviceUnauthorized) {
        return l10n.diagnosticsUnauthorizedDesc;
      }
      if (adbState is AdbStateDeviceConnected) {
        return l10n.diagnosticsConnectedDesc;
      }
      return l10n.diagnosticsUnknownDesc;
    }

    String authDesc() {
      if (adbState is AdbStateDeviceUnauthorized) {
        return l10n.diagnosticsUnauthorizedDesc;
      }
      if (adbState is AdbStateDeviceConnected) {
        return l10n.diagnosticsConnectedDesc;
      }
      return l10n.diagnosticsUnknownDesc;
    }

    final deviceLevel = device.isConnected ? _DiagLevel.ok : _DiagLevel.info;
    final deviceDesc = device.isConnected
        ? '"${device.deviceName}" • ${device.deviceSerial}'
        : l10n.noDeviceConnected;

    final adbPath = settings.settings.adbPath;
    final adbPathDesc = (adbPath.isEmpty)
        ? l10n.diagnosticsUsingSystemPath
        : l10n.diagnosticsConfiguredPath(adbPath);

    return AlertDialog(
      title: Row(
        children: [
          Expanded(child: Text(l10n.diagnosticsTitle)),
          IconButton(
            tooltip: l10n.refresh,
            onPressed: () {
              signals.AdbRequest(
                command: const signals.AdbCommandRefreshDevice(),
                commandKey: '',
              ).sendSignalToRust();
            },
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _legend(l10n),
            const SizedBox(height: 12),
            Divider(
                height: 1, color: Theme.of(context).colorScheme.outlineVariant),
            const SizedBox(height: 8),
            _item(
              context,
              level: serverLevel,
              title: l10n.diagnosticsAdbServer,
              description: serverDesc(),
            ),
            _item(
              context,
              level: devicesLevel,
              title: l10n.diagnosticsDevices,
              description: devicesDesc(),
            ),
            _item(
              context,
              level: authLevel,
              title: l10n.diagnosticsAuthorization,
              description: authDesc(),
            ),
            _item(
              context,
              level: deviceLevel,
              title: l10n.diagnosticsActiveDevice,
              description: deviceDesc,
            ),
            const SizedBox(height: 8),
            ListTile(
              leading: const Icon(Icons.settings),
              title: Text(l10n.diagnosticsAdbPath),
              subtitle: Text(adbPathDesc),
              contentPadding: const EdgeInsets.symmetric(horizontal: 8),
              dense: false,
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).maybePop(),
          child: Text(l10n.commonClose),
        ),
      ],
    );
  }
}
