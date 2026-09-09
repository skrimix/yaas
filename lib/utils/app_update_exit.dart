import 'dart:ui';

/// Keeps the update candidate attached to the app's cancellable exit flow.
class AppUpdateExit {
  AppUpdateExit({
    required this.requestExit,
    required this.cancelUpdate,
    required this.isExiting,
  });

  final Future<AppExitResponse> Function() requestExit;
  final void Function() cancelUpdate;
  final bool Function() isExiting;
  String? candidateId;

  Future<void> request(String id) async {
    if (candidateId != null) return;
    if (isExiting()) {
      cancelUpdate();
      return;
    }
    candidateId = id;
    try {
      if (await requestExit() == AppExitResponse.cancel) cancelUpdate();
    } catch (_) {
      cancelUpdate();
    } finally {
      candidateId = null;
    }
  }
}
