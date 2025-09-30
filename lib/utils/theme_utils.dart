import 'dart:io' show File, Platform, Process;
import 'package:flutter/material.dart';
import '../src/l10n/app_localizations.dart';

const Map<String, MaterialColor> kSeedColorPalette = {
  'deep_purple': Colors.deepPurple,
  'indigo': Colors.indigo,
  'blue': Colors.blue,
  'cyan': Colors.cyan,
  'teal': Colors.teal,
  'green': Colors.green,
  'lime': Colors.lime,
  'amber': Colors.amber,
  'orange': Colors.orange,
  'deep_orange': Colors.deepOrange,
  'red': Colors.red,
  'pink': Colors.pink,
  'purple': Colors.purple,
  'brown': Colors.brown,
  'blue_grey': Colors.blueGrey,
};

const String kDefaultSeedKey = 'deep_purple';

MaterialColor seedFromKey(String key) =>
    kSeedColorPalette[key] ?? kSeedColorPalette[kDefaultSeedKey]!;

String normalizeSeedKey(String key) =>
    kSeedColorPalette.containsKey(key) ? key : kDefaultSeedKey;

String seedLabel(AppLocalizations l10n, String key) {
  switch (key) {
    case 'deep_purple':
      return l10n.colorDeepPurple;
    case 'indigo':
      return l10n.colorIndigo;
    case 'blue':
      return l10n.colorBlue;
    case 'cyan':
      return l10n.colorCyan;
    case 'teal':
      return l10n.colorTeal;
    case 'green':
      return l10n.colorGreen;
    case 'lime':
      return l10n.colorLime;
    case 'amber':
      return l10n.colorAmber;
    case 'orange':
      return l10n.colorOrange;
    case 'deep_orange':
      return l10n.colorDeepOrange;
    case 'red':
      return l10n.colorRed;
    case 'pink':
      return l10n.colorPink;
    case 'purple':
      return l10n.colorPurple;
    case 'brown':
      return l10n.colorBrown;
    case 'blue_grey':
      return l10n.colorBlueGrey;
    default:
      return key;
  }
}

/// Best-effort KDE Plasma accent reader (R,G,B in kdeglobals)
Future<Color?> readKdeAccent() async {
  if (!Platform.isLinux) return null;

  final desktop = Platform.environment['XDG_CURRENT_DESKTOP'] ?? '';
  final upper = desktop.toUpperCase();
  final isKde = upper.contains('KDE') || upper.contains('PLASMA');
  if (!isKde) return null;

  Future<String?> tryCmd(String cmd) async {
    try {
      final res = await Process.run(cmd, [
        '--file',
        'kdeglobals',
        '--group',
        'General',
        '--key',
        'AccentColor',
      ]);
      if (res.exitCode == 0) {
        final out = (res.stdout as String).trim();
        return out.isEmpty ? null : out;
      }
    } catch (_) {}
    return null;
  }

  final fromCmd =
      (await tryCmd('kreadconfig6')) ?? await tryCmd('kreadconfig5');
  String? rgb = fromCmd;

  rgb ??= await () async {
    final home = Platform.environment['HOME'] ?? '';
    final path = '$home/.config/kdeglobals';
    final file = File(path);
    if (!await file.exists()) return null;
    final lines = await file.readAsLines();
    bool inGeneral = false;
    for (final line in lines) {
      final l = line.trim();
      if (l.startsWith('[')) inGeneral = l == '[General]';
      if (inGeneral && l.startsWith('AccentColor=')) {
        return l.substring('AccentColor='.length).trim();
      }
    }
    return null;
  }();

  if (rgb == null) return null;

  final parts = rgb.split(',');
  if (parts.length < 3) return null;
  int p(int i) => int.tryParse(parts[i].trim()) ?? 0;
  final r = p(0), g = p(1), b = p(2);
  return Color.fromARGB(0xFF, r, g, b);
}
