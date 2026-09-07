import 'dart:typed_data';

/// The screenshot runner substitutes this transport for the native Rinf API.
class RustSignalPack<T> {
  RustSignalPack(this.message, this.binary);

  final T message;
  final Uint8List binary;
}

Future<void> initializeRust(
  Map<String, void Function(Uint8List, Uint8List)> assignRustSignal, {
  String? compiledLibPath,
}) async {
  throw StateError('Screenshots must seed their own data.');
}

void finalizeRust() {}

/// Read requests are answered by the screenshot fixtures through the bindings.
void sendDartSignal(
  String endpointSymbol,
  Uint8List messageBytes,
  Uint8List binary,
) {
  const reads = {
    'rinf_send_dart_signal_get_downloads_request',
    'rinf_send_dart_signal_get_app_details_request',
    'rinf_send_dart_signal_get_app_reviews_request',
  };
  if (!reads.contains(endpointSymbol)) {
    throw StateError('Unexpected screenshot request: $endpointSymbol');
  }
}
