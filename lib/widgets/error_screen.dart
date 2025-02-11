import 'dart:io' show exit;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
// import 'package:rinf/rinf.dart';
import 'package:toastification/toastification.dart';

class ErrorScreen extends StatelessWidget {
  final String message;

  const ErrorScreen({super.key, required this.message});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 32),
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 600),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(
                    Icons.error_outline,
                    size: 64,
                    color: Theme.of(context).colorScheme.error,
                  ),
                  const SizedBox(height: 24),
                  Text(
                    'Fatal Error',
                    style: Theme.of(context).textTheme.headlineMedium,
                  ),
                  const SizedBox(height: 16),
                  Flexible(
                    child: Container(
                      margin: const EdgeInsets.symmetric(horizontal: 32),
                      decoration: BoxDecoration(
                        color: Theme.of(context)
                            .colorScheme
                            .surfaceContainerHighest,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: SingleChildScrollView(
                        padding: const EdgeInsets.all(16),
                        child: SelectableText(
                          message,
                          textAlign: TextAlign.left,
                          style: Theme.of(context).textTheme.bodyLarge,
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(height: 24),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      FilledButton.icon(
                        onPressed: () {
                          // finalizeRust();
                          exit(1);
                        },
                        icon: const Icon(Icons.close),
                        label: const Text('Exit Application'),
                      ),
                      const SizedBox(width: 16),
                      OutlinedButton.icon(
                        onPressed: () {
                          Clipboard.setData(ClipboardData(text: message));
                          // ScaffoldMessenger.of(context).showSnackBar(
                          //   const SnackBar(
                          //     content:
                          //         Text('Error message copied to clipboard'),
                          //     behavior: SnackBarBehavior.floating,
                          //   ),
                          // );
                          toastification.show(
                            type: ToastificationType.success,
                            title:
                                const Text('Error message copied to clipboard'),
                            borderSide: BorderSide.none,
                            autoCloseDuration: const Duration(seconds: 3),
                          );
                        },
                        icon: const Icon(Icons.copy),
                        label: const Text('Copy Error'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
