import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart' as messages;

// Global UI state retained across page navigation.
class AppState extends ChangeNotifier {
  // Rust panic state
  String? _panicMessage;
  String? get panicMessage => _panicMessage;
  void setPanicMessage(String message) {
    _panicMessage = message;
    notifyListeners();
  }

  // Download Apps page state
  String _downloadSearchQuery = '';
  String _downloadSortKey = 'name'; // 'name' | 'date' | 'size'
  bool _downloadSortAscending = true;
  double _downloadScrollOffset = 0.0;
  bool _downloadShowCheckboxes = false;
  bool _downloadShowOnlySelected = false;
  final List<String> _downloadSelectedFullNames = [];

  String get downloadSearchQuery => _downloadSearchQuery;
  String get downloadSortKey => _downloadSortKey;
  bool get downloadSortAscending => _downloadSortAscending;
  double get downloadScrollOffset => _downloadScrollOffset;
  bool get downloadShowCheckboxes => _downloadShowCheckboxes;
  bool get downloadShowOnlySelected => _downloadShowOnlySelected;
  List<String> get downloadSelectedFullNames =>
      List.unmodifiable(_downloadSelectedFullNames);

  void setDownloadSearchQuery(String value) {
    if (_downloadSearchQuery == value) return;
    _downloadSearchQuery = value;
    notifyListeners();
  }

  void setDownloadSort(String key, bool ascending) {
    if (_downloadSortKey == key && _downloadSortAscending == ascending) return;
    _downloadSortKey = key;
    _downloadSortAscending = ascending;
    notifyListeners();
  }

  void setDownloadScrollOffset(double offset) {
    if (offset == _downloadScrollOffset) return;
    _downloadScrollOffset = offset;
    // No notify needed for passive state updates
  }

  void setDownloadShowCheckboxes(bool value) {
    if (value == _downloadShowCheckboxes) return;
    _downloadShowCheckboxes = value;
    notifyListeners();
  }

  void setDownloadShowOnlySelected(bool value) {
    if (value == _downloadShowOnlySelected) return;
    _downloadShowOnlySelected = value;
    notifyListeners();
  }

  void setDownloadSelectedFullNames(Iterable<String> values) {
    _downloadSelectedFullNames
      ..clear()
      ..addAll(values);
    notifyListeners();
  }

  // Manage Apps page state
  int _manageAppsCategoryIndex = 0; // 0: vr, 1: other, 2: system
  int get manageAppsCategoryIndex => _manageAppsCategoryIndex;
  void setManageAppsCategoryIndex(int index) {
    if (_manageAppsCategoryIndex == index) return;
    _manageAppsCategoryIndex = index;
    notifyListeners();
  }

  // Manage Apps scroll offsets per category (vr, other, system)
  final List<double> _manageScrollOffsets = [0.0, 0.0, 0.0];
  double getManageScrollOffset(int categoryIndex) {
    if (categoryIndex < 0 || categoryIndex >= _manageScrollOffsets.length) {
      debugPrint(
          '[AppState] getManageScrollOffset: invalid category index $categoryIndex');
      return 0.0;
    }
    return _manageScrollOffsets[categoryIndex];
  }

  void setManageScrollOffset(int categoryIndex, double offset) {
    if (categoryIndex < 0 || categoryIndex >= _manageScrollOffsets.length) {
      debugPrint(
          '[AppState] setManageScrollOffset: invalid category index $categoryIndex');
      return;
    }
    _manageScrollOffsets[categoryIndex] = offset;
    // Passive update; no notify
  }

  // Local Sideload page state
  bool _sideloadIsDirectory = false;
  String _sideloadLastPath = '';

  bool get sideloadIsDirectory => _sideloadIsDirectory;
  String get sideloadLastPath => _sideloadLastPath;

  void setSideloadIsDirectory(bool isDirectory) {
    if (_sideloadIsDirectory == isDirectory) return;
    _sideloadIsDirectory = isDirectory;
    notifyListeners();
  }

  void setSideloadLastPath(String path) {
    if (_sideloadLastPath == path) return;
    _sideloadLastPath = path;
    notifyListeners();
  }

  // Backend (Rust) version/build info
  BackendVersionInfo? _backendVersionInfo;
  BackendVersionInfo? get backendVersionInfo => _backendVersionInfo;
  void setBackendVersionInfo(messages.AppVersionInfo info) {
    _backendVersionInfo = BackendVersionInfo(
      backendVersion: info.backendVersion,
      profile: info.profile,
      rustcVersion: info.rustcVersion,
      builtTimeUtc: info.builtTimeUtc,
      gitCommitHash: info.gitCommitHash,
      gitCommitHashShort: info.gitCommitHashShort,
      gitDirty: info.gitDirty ?? false,
    );
    notifyListeners();
  }
}

class BackendVersionInfo {
  final String backendVersion;
  final String profile;
  final String rustcVersion;
  final String builtTimeUtc;
  final String? gitCommitHash;
  final String? gitCommitHashShort;
  final bool gitDirty;

  const BackendVersionInfo({
    required this.backendVersion,
    required this.profile,
    required this.rustcVersion,
    required this.builtTimeUtc,
    this.gitCommitHash,
    this.gitCommitHashShort,
    required this.gitDirty,
  });
}
