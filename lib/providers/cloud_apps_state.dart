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

  String thumbnailUrlFor(String packageName) {
    return '${_mediaBaseUrl}thumbnails/$packageName.jpg';
  }

  String trailerUrlFor(String packageName) {
    return '${_mediaBaseUrl}videos/$packageName.mp4';
  }

  CloudAppsState() {
    CloudAppsChangedEvent.rustSignalStream.listen((event) {
      _isLoading = event.message.isLoading;
      _error = event.message.error;
      final apps = event.message.apps;
      if (apps != null) {
        _apps = apps;
      }
      if (_error != null) {
        _apps = [];
      }
      notifyListeners();
    });

    // Reset state when downloader becomes unavailable, and auto-load when it becomes available
    DownloaderAvailabilityChanged.rustSignalStream.listen((event) {
      final msg = event.message;
      if (!msg.available) {
        _apps = [];
        _error = null;
        _isLoading = false;
        notifyListeners();
      } else {
        // Kick off a load when downloader becomes available and we have nothing yet
        if (_apps.isEmpty && !_isLoading) {
          load();
        }
      }
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
    LoadCloudAppsRequest(refresh: true).sendSignalToRust();
  }

  void load() {
    if (_apps.isEmpty && !_isLoading) {
      LoadCloudAppsRequest(refresh: false).sendSignalToRust();
    }
  }
}
