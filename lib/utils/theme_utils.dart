import 'dart:io' show File, Platform, Process;
import 'package:dynamic_color/dynamic_color.dart';
import 'package:flutter/material.dart';
import '../src/l10n/app_localizations.dart';

/// Builds the app theme with shared component styles for [scheme].
ThemeData buildAppTheme(ColorScheme scheme) {
  final inputBorder = OutlineInputBorder(
    borderRadius: BorderRadius.circular(12),
    borderSide: BorderSide.none,
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    cardTheme: CardThemeData(
      elevation: 0,
      color: scheme.surfaceContainerLow,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
    ),
    inputDecorationTheme: InputDecorationThemeData(
      filled: true,
      fillColor: WidgetStateColor.resolveWith(
        (states) => scheme.surfaceContainerHighest.withValues(
          alpha: states.contains(WidgetState.disabled) ? 0.35 : 0.7,
        ),
      ),
      border: inputBorder,
      enabledBorder: inputBorder,
      disabledBorder: inputBorder,
      focusedBorder: inputBorder.copyWith(
        borderSide: BorderSide(color: scheme.primary, width: 2),
      ),
      errorBorder: inputBorder.copyWith(
        borderSide: BorderSide(color: scheme.error),
      ),
      focusedErrorBorder: inputBorder.copyWith(
        borderSide: BorderSide(color: scheme.error, width: 2),
      ),
    ),
    extensions: [StatusColors.harmonized(scheme)],
  );
}

/// Success and warning colors harmonized with the active color scheme.
@immutable
class StatusColors extends ThemeExtension<StatusColors> {
  const StatusColors({
    required this.success,
    required this.onSuccess,
    required this.successContainer,
    required this.onSuccessContainer,
    required this.warning,
    required this.onWarning,
    required this.warningContainer,
    required this.onWarningContainer,
  });

  factory StatusColors.harmonized(ColorScheme scheme) {
    ColorScheme tonal(Color source) => ColorScheme.fromSeed(
          seedColor: source.harmonizeWith(scheme.primary),
          brightness: scheme.brightness,
          dynamicSchemeVariant: DynamicSchemeVariant.fidelity,
        );
    final success = tonal(const Color(0xFF2E7D32));
    final warning = tonal(const Color(0xFFF9A825));
    return StatusColors(
      success: success.primary,
      onSuccess: success.onPrimary,
      successContainer: success.primaryContainer,
      onSuccessContainer: success.onPrimaryContainer,
      warning: warning.primary,
      onWarning: warning.onPrimary,
      warningContainer: warning.primaryContainer,
      onWarningContainer: warning.onPrimaryContainer,
    );
  }

  static StatusColors of(BuildContext context) {
    final theme = Theme.of(context);
    return theme.extension<StatusColors>() ??
        StatusColors.harmonized(theme.colorScheme);
  }

  final Color success;
  final Color onSuccess;
  final Color successContainer;
  final Color onSuccessContainer;
  final Color warning;
  final Color onWarning;
  final Color warningContainer;
  final Color onWarningContainer;

  @override
  StatusColors copyWith({
    Color? success,
    Color? onSuccess,
    Color? successContainer,
    Color? onSuccessContainer,
    Color? warning,
    Color? onWarning,
    Color? warningContainer,
    Color? onWarningContainer,
  }) {
    return StatusColors(
      success: success ?? this.success,
      onSuccess: onSuccess ?? this.onSuccess,
      successContainer: successContainer ?? this.successContainer,
      onSuccessContainer: onSuccessContainer ?? this.onSuccessContainer,
      warning: warning ?? this.warning,
      onWarning: onWarning ?? this.onWarning,
      warningContainer: warningContainer ?? this.warningContainer,
      onWarningContainer: onWarningContainer ?? this.onWarningContainer,
    );
  }

  @override
  StatusColors lerp(StatusColors? other, double t) {
    if (other == null) return this;
    return StatusColors(
      success: Color.lerp(success, other.success, t)!,
      onSuccess: Color.lerp(onSuccess, other.onSuccess, t)!,
      successContainer:
          Color.lerp(successContainer, other.successContainer, t)!,
      onSuccessContainer:
          Color.lerp(onSuccessContainer, other.onSuccessContainer, t)!,
      warning: Color.lerp(warning, other.warning, t)!,
      onWarning: Color.lerp(onWarning, other.onWarning, t)!,
      warningContainer:
          Color.lerp(warningContainer, other.warningContainer, t)!,
      onWarningContainer:
          Color.lerp(onWarningContainer, other.onWarningContainer, t)!,
    );
  }
}

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
