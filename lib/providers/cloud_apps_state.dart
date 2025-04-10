import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

class CloudAppsState extends ChangeNotifier {
  List<CloudApp> _apps = [];
  String? _error;
  bool _isLoading = false;

  List<CloudApp> get apps => _apps;
  String? get error => _error;
  bool get isLoading => _isLoading;

  CloudAppsState() {
    CloudAppsChangedEvent.rustSignalStream.listen((event) {
      _apps = event.message.apps;
      _error = event.message.error;
      _isLoading = false;
      notifyListeners();
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
