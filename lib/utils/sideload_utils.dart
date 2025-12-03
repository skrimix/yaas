import 'dart:io';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:toastification/toastification.dart';
import '../providers/device_state.dart';
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';
import 'utils.dart';

/// Threshold below which we warn the user
const int _lowSpaceThresholdBytes = 2 * 1000 * 1000 * 1000;

class SideloadUtils {
  static bool isValidApkFile(String path) {
    final file = File(path);
    return file.existsSync() && file.path.toLowerCase().endsWith('.apk');
  }

  static bool isDirectoryValid(String path) {
    final dir = Directory(path);
    if (!dir.existsSync()) return false;

    try {
      return dir.listSync(recursive: false).any((entity) =>
          entity is File &&
          (entity.path.toLowerCase().endsWith('.apk') ||
              entity.path.toLowerCase() == 'install.txt'));
    } catch (e) {
      debugPrint('Error checking directory validity: $e');
      return false;
    }
  }

  static bool isBackupDirectory(String path) {
    final dir = Directory(path);
    if (!dir.existsSync()) return false;
    final marker = File('${dir.path}${Platform.pathSeparator}.backup');
    return marker.existsSync();
  }

  static void installApp(String path, bool isDirectory) {
    (isDirectory
            ? TaskRequest(
                task: TaskInstallLocalApp(value: path),
              )
            : TaskRequest(
                task: TaskInstallApk(value: path),
              ))
        .sendSignalToRust();
  }

  static void restoreBackup(String backupPath) {
    TaskRequest(
      task: TaskRestoreBackup(value: backupPath),
    ).sendSignalToRust();
  }

  static void showErrorToast(BuildContext context, String message) {
    toastification.show(
      type: ToastificationType.error,
      style: ToastificationStyle.flat,
      title: Text(AppLocalizations.of(context).commonError),
      description: Text(message),
      autoCloseDuration: const Duration(seconds: 3),
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  }

  static void showInfoToast(
      BuildContext context, String title, String description) {
    toastification.show(
      type: ToastificationType.info,
      style: ToastificationStyle.flat,
      title: Text(title),
      description: Text(description),
      autoCloseDuration: const Duration(seconds: 3),
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  }

  /// Calculates the total size of a file or directory in bytes.
  static int calculateSize(String path, bool isDirectory) {
    if (isDirectory) {
      final dir = Directory(path);
      if (!dir.existsSync()) return 0;
      int total = 0;
      try {
        for (final entity in dir.listSync(recursive: true)) {
          if (entity is File) {
            total += entity.lengthSync();
          }
        }
      } catch (_) {
        // Ignore
      }
      return total;
    } else {
      final file = File(path);
      if (!file.existsSync()) return 0;
      try {
        return file.lengthSync();
      } catch (_) {
        return 0;
      }
    }
  }

  /// Checks if installation would leave less than the threshold free and shows a
  /// confirmation dialog if so.
  ///
  /// Returns `true` if the installation should proceed (either there's enough
  /// space or the user confirmed), `false` if the user cancelled.
  ///
  /// [sizeBytes] is the total size of files to be installed.
  static Future<bool> confirmIfLowSpace(
      BuildContext context, int sizeBytes) async {
    final deviceState = context.read<DeviceState>();
    final spaceInfo = deviceState.spaceInfo;
    if (spaceInfo == null) {
      return true;
    }

    final available = spaceInfo.available.toInt();
    final remainingAfterInstall = available - sizeBytes;

    if (remainingAfterInstall >= _lowSpaceThresholdBytes) {
      return true;
    }

    final remainingDisplay = formatSize(
      remainingAfterInstall > 0 ? remainingAfterInstall : 0,
      2,
    );

    final l10n = AppLocalizations.of(context);
    final thresholdDisplay = formatSize(_lowSpaceThresholdBytes, 0);
    final result = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.lowSpaceWarningTitle),
        content: Text(
            l10n.lowSpaceWarningMessage(thresholdDisplay, remainingDisplay)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(l10n.lowSpaceWarningContinue),
          ),
        ],
      ),
    );

    return result == true;
  }
}
