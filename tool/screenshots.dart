import 'dart:convert';
import 'dart:io';

/// Renders the documentation screenshots with an isolated, offline transport.
Future<void> main(List<String> arguments) async {
  if (arguments.length > 1 || arguments.contains('--help')) {
    stdout.writeln('Usage: dart run tool/screenshots.dart [output-directory]');
    return;
  }

  final root = File.fromUri(Platform.script).parent.parent;
  final output = Directory(arguments.firstOrNull ?? '${root.path}/screenshots');
  final flutter = Platform.isWindows ? 'flutter.bat' : 'flutter';
  Future<int> run(List<String> args) async {
    final process = await Process.start(
      flutter,
      args,
      workingDirectory: root.path,
      environment: {'TZ': 'UTC'},
      mode: ProcessStartMode.inheritStdio,
      runInShell: Platform.isWindows,
    );
    return process.exitCode;
  }

  final pubExit = await run(['pub', 'get', '--enforce-lockfile']);
  if (pubExit != 0) {
    exitCode = pubExit;
    return;
  }

  final configFile = File('${root.path}/.dart_tool/package_config.json');
  final config = jsonDecode(await configFile.readAsString()) as Map;
  for (final package in config['packages'] as List) {
    package['rootUri'] = package['name'] == 'rinf'
        ? root.uri.resolve('tool/screenshots/rinf/').toString()
        : configFile.uri.resolve(package['rootUri'] as String).toString();
  }
  final isolatedConfig =
      File('${root.path}/.dart_tool/screenshots-$pid.package_config.json');
  try {
    await isolatedConfig.writeAsString(jsonEncode(config));
    exitCode = await run([
      'test',
      '--no-pub',
      '--packages=${isolatedConfig.path}',
      '--dart-define=SCREENSHOT_DIR=${output.absolute.path}',
      '--reporter=expanded',
      'tool/screenshots/generate_test.dart',
    ]);
    if (exitCode == 0) {
      stdout.writeln('Screenshots saved to ${output.absolute.path}');
    }
  } finally {
    await isolatedConfig.delete();
  }
}
