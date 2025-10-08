import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../../providers/adb_state.dart';
import '../../providers/device_state.dart';
import '../../providers/settings_state.dart';
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
    if (state is AdbStateServerStartFailed) return _DiagLevel.error;
    return _DiagLevel.ok;
  }

  _DiagLevel _devicesLevel(AdbState state) {
    if (state is AdbStateNoDevices) return _DiagLevel.error;
    if (state is AdbStateDevicesAvailable) return _DiagLevel.warn;
    if (state is AdbStateDeviceUnauthorized ||
        state is AdbStateDeviceConnected) {
      return _DiagLevel.ok;
    }
    return _DiagLevel.warn;
  }

  _DiagLevel _authLevel(AdbState state) {
    if (state is AdbStateDeviceUnauthorized) return _DiagLevel.error;
    if (state is AdbStateDeviceConnected) return _DiagLevel.ok;
    return _DiagLevel.info;
  }

  Widget _item(
    BuildContext context, {
    required _DiagLevel level,
    required String title,
    required String description,
    Widget? trailing,
    Widget? additionalContent,
  }) {
    final color = _levelColor(level);
    return ListTile(
      leading: Icon(_levelIcon(level), color: color),
      title: Text(title),
      subtitle: additionalContent != null
          ? Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(description),
                const SizedBox(height: 4),
                additionalContent,
              ],
            )
          : Text(description),
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
      if (adbState is AdbStateServerStartFailed) {
        return l10n.diagnosticsServerStartFailedDesc;
      }
      return l10n.diagnosticsServerRunningDesc;
    }

    String devicesDesc() {
      if (adbState is AdbStateNoDevices) return l10n.diagnosticsNoDevicesDesc;
      if (adbState is AdbStateDevicesAvailable) {
        final count = adbState.value.length;
        return l10n.diagnosticsDevicesAvailableDesc(count);
      }
      if (adbState is AdbStateDeviceUnauthorized ||
          adbState is AdbStateDeviceConnected) {
        final count = adb.devicesCount;
        return l10n.diagnosticsDevicesAvailableDesc(count <= 0 ? 1 : count);
      }
      return l10n.diagnosticsUnknownDesc;
    }

    Widget? devicesListWidget() {
      if (adb.availableDevices.isEmpty) return null;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: adb.availableDevices.map((device) {
          final name = device.serial;
          final state = device.state.name;
          return Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Text(
              '• $name ($state)',
              style: Theme.of(context)
                  .textTheme
                  .bodySmall
                  ?.copyWith(fontFamily: 'monospace'),
            ),
          );
        }).toList(),
      );
    }

    String authDesc() {
      if (adbState is AdbStateDeviceUnauthorized) {
        return l10n.diagnosticsUnauthorizedDesc;
      }
      if (adbState is AdbStateDeviceConnected) {
        return l10n.diagnosticsAuthorizedDesc;
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
      title: Text(l10n.diagnosticsTitle),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ListTile(
              leading: const Icon(Icons.settings),
              title: Text(l10n.diagnosticsAdbPath),
              subtitle: Text(adbPathDesc),
              contentPadding: const EdgeInsets.symmetric(horizontal: 8),
              dense: false,
            ),
            const SizedBox(height: 4),
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
              additionalContent: devicesListWidget(),
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
