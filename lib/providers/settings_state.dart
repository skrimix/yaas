import 'package:flutter/material.dart';

enum ConnectionType {
  usb,
  wireless,
}

enum DownloadCleanupPolicy {
  deleteAfterInstall,
  keepOneVersion,
  keepTwoVersions,
  keepAllVersions,
}

class SettingsState extends ChangeNotifier {
  String _rclonePath = '/usr/bin/rclone';
  String _rcloneRemoteName = 'remote';
  String _adbPath = '/usr/bin/adb';
  ConnectionType _preferredConnectionType = ConnectionType.usb;
  String _downloadsLocation = '~/Downloads/rql';
  String _backupsLocation = '~/Documents/rql/backups';
  String _bandwidthLimit = '';
  DownloadCleanupPolicy _cleanupPolicy =
      DownloadCleanupPolicy.deleteAfterInstall;

  // Getters
  String get rclonePath => _rclonePath;
  String get rcloneRemoteName => _rcloneRemoteName;
  String get adbPath => _adbPath;
  ConnectionType get preferredConnectionType => _preferredConnectionType;
  String get downloadsLocation => _downloadsLocation;
  String get backupsLocation => _backupsLocation;
  String get bandwidthLimit => _bandwidthLimit;
  DownloadCleanupPolicy get cleanupPolicy => _cleanupPolicy;

  // Setters
  void setRclonePath(String path) {
    _rclonePath = path;
    notifyListeners();
  }

  void setRcloneRemoteName(String name) {
    _rcloneRemoteName = name;
    notifyListeners();
  }

  void setAdbPath(String path) {
    _adbPath = path;
    notifyListeners();
  }

  void setPreferredConnectionType(ConnectionType type) {
    _preferredConnectionType = type;
    notifyListeners();
  }

  void setDownloadsLocation(String path) {
    _downloadsLocation = path;
    notifyListeners();
  }

  void setBackupsLocation(String path) {
    _backupsLocation = path;
    notifyListeners();
  }

  void setBandwidthLimit(String limit) {
    _bandwidthLimit = limit;
    notifyListeners();
  }

  void setCleanupPolicy(DownloadCleanupPolicy policy) {
    _cleanupPolicy = policy;
    notifyListeners();
  }
}
