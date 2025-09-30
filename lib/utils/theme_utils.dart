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
const String kCustomColorKey = '__custom__';

/// Parse hex color string (e.g., "#FF5733" or "FF5733")
Color? parseHexColor(String hex) {
  final normalized = hex.replaceAll('#', '').trim();
  if (normalized.length != 6) return null;
  final value = int.tryParse(normalized, radix: 16);
  if (value == null) return null;
  return Color(0xFF000000 | value);
}

/// Check if a key is a valid hex color
bool isHexColor(String key) {
  if (key.startsWith(kCustomColorKey)) {
    final hex = key.substring(kCustomColorKey.length);
    return parseHexColor(hex) != null;
  }
  return false;
}

MaterialColor seedFromKey(String key) {
  // Check if it's a hex color (format: __custom__RRGGBB)
  if (key.startsWith(kCustomColorKey)) {
    final hex = key.substring(kCustomColorKey.length);
    final color = parseHexColor(hex);
    if (color != null) {
      final a = (color.a * 255).round();
      final r = (color.r * 255).round();
      final g = (color.g * 255).round();
      final b = (color.b * 255).round();
      final argb = a << 24 | r << 16 | g << 8 | b;
      return MaterialColor(argb, {
        50: color.withValues(alpha: 0.1),
        100: color.withValues(alpha: 0.2),
        200: color.withValues(alpha: 0.3),
        300: color.withValues(alpha: 0.4),
        400: color.withValues(alpha: 0.5),
        500: color.withValues(alpha: 0.6),
        600: color.withValues(alpha: 0.7),
        700: color.withValues(alpha: 0.8),
        800: color.withValues(alpha: 0.9),
        900: color,
      });
    }
  }
  return kSeedColorPalette[key] ?? kSeedColorPalette[kDefaultSeedKey]!;
}

String normalizeSeedKey(String key) {
  if (isHexColor(key)) return key;
  return kSeedColorPalette.containsKey(key) ? key : kDefaultSeedKey;
}

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
