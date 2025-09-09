import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

class CloudAppsState extends ChangeNotifier {
  List<CloudApp> _apps = [];
  String? _error;
  bool _isLoading = false;
  String _mediaBaseUrl = '';
  String? _mediaCacheDir;

  List<CloudApp> get apps => _apps;
  String? get error => _error;
  bool get isLoading => _isLoading;
  String get mediaBaseUrl => _mediaBaseUrl;
  String? get mediaCacheDir => _mediaCacheDir;

  String _fixPackageName(String packageName) {
    if (packageName.startsWith('mr.')) {
      return packageName.substring(3);
    }
    return packageName;
  }

  String thumbnailUrlFor(String packageName) {
    final slug = _fixPackageName(packageName);
    return '${_mediaBaseUrl}thumbnails/$slug.jpg';
  }

  String trailerUrlFor(String packageName) {
    final slug = _fixPackageName(packageName);
    return '${_mediaBaseUrl}videos/$slug.mp4';
  }

  CloudAppsState() {
    CloudAppsChangedEvent.rustSignalStream.listen((event) {
      _apps = event.message.apps;
      _error = event.message.error;
      _isLoading = false;
      notifyListeners();
    });

    // Receive media config from Rust
    MediaConfigChanged.rustSignalStream.listen((event) {
      final cfg = event.message;
      final newUrl = cfg.mediaBaseUrl.endsWith('/')
          ? cfg.mediaBaseUrl
          : '${cfg.mediaBaseUrl}/';
      bool changed = false;
      if (_mediaBaseUrl != newUrl) {
        _mediaBaseUrl = newUrl;
        changed = true;
      }
      if (_mediaCacheDir != cfg.cacheDir) {
        _mediaCacheDir = cfg.cacheDir;
        changed = true;
      }
      if (changed) notifyListeners();
    });
  }

  void refresh() {
    _isLoading = true;
    notifyListeners();
    LoadCloudAppsRequest(refresh: true).sendSignalToRust();
  }

  void load() {
    if (_apps.isEmpty && !_isLoading) {
      _isLoading = true;
      notifyListeners();
      LoadCloudAppsRequest(refresh: false).sendSignalToRust();
    }
  }
}
