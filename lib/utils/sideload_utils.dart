import 'dart:io';
import 'package:flutter/material.dart';
import 'package:toastification/toastification.dart';
import '../src/bindings/bindings.dart';
import '../src/l10n/app_localizations.dart';

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
      // TODO: log error
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
}
