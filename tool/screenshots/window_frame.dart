import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/painting.dart';
import 'package:flutter_svg/flutter_svg.dart';

/// Adds the window decoration after capture so dialogs only dim the app.
class WindowFrame {
  WindowFrame._(this._decoration, this._icon);

  final PictureInfo _decoration;
  final PictureInfo _icon;

  static Future<WindowFrame> load() async {
    final decoration = await vg.loadPicture(
      SvgStringLoader(
          await File('tool/screenshots/breeze_frame.svg').readAsString()),
      null,
    );
    final icon = await vg.loadPicture(
      SvgStringLoader(await File('assets/svg/app_icon.svg').readAsString()),
      null,
    );
    return WindowFrame._(decoration, icon);
  }

  Future<ui.Image> render(ui.Image content) async {
    final size = Size(content.width + 2, content.height + 29);
    if (_decoration.size != size) {
      throw StateError('Update breeze_frame.svg to match the capture size.');
    }
    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder);
    canvas.clipRRect(RRect.fromRectAndRadius(
      Offset.zero & size,
      const Radius.circular(4),
    ));
    canvas.drawImage(content, const Offset(1, 28), Paint());
    canvas.drawPicture(_decoration.picture);
    canvas.save();
    canvas.translate(6, 6);
    canvas.scale(16 / _icon.size.width, 16 / _icon.size.height);
    canvas.drawPicture(_icon.picture);
    canvas.restore();

    final title = TextPainter(
      text: const TextSpan(
        text: 'YAAS',
        style: TextStyle(
          fontFamily: 'Roboto',
          fontSize: 12,
          color: Color(0xffcdd6f4),
        ),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    title.paint(canvas,
        Offset((size.width - title.width) / 2, (28 - title.height) / 2));
    title.dispose();

    final picture = recorder.endRecording();
    try {
      return await picture.toImage(size.width.toInt(), size.height.toInt());
    } finally {
      picture.dispose();
    }
  }

  void dispose() {
    _decoration.picture.dispose();
    _icon.picture.dispose();
  }
}
