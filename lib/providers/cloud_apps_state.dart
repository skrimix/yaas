import 'package:flutter/material.dart';
import '../messages/all.dart';

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
      _error = event.message.hasError() ? event.message.error : null;
      _isLoading = false;
      notifyListeners();
    });
  }

  void refresh() {
    _isLoading = true;
    notifyListeners();
    GetCloudAppsRequest(refresh: true).sendSignalToRust();
  }

  void load() {
    if (_apps.isEmpty && !_isLoading) {
      _isLoading = true;
      notifyListeners();
      GetCloudAppsRequest(refresh: false).sendSignalToRust();
    }
  }
}
