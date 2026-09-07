# Documentation screenshots

From the project root, run:

```sh
just gen                              # Needed on a fresh checkout or after signal changes
just screenshots                      # Refresh the six PNGs in screenshots/
just screenshots build/screenshots    # Render previews in a separate directory
```

Without `just`, use `rinf gen` and `dart run tool/screenshots.dart [output-directory]`.
The runner resolves the locked Flutter dependencies. It needs Flutter and generated
Rinf bindings, but no Rust core build, headset, downloader, or desktop session.

The captures use the real pages, navigation, status bar, and dialogs with fictional
data from `fixtures.dart`. The app renders at 1264 × 711 with the dark purple theme,
English text, Roboto from the Flutter SDK, and UTC dates. A Breeze-style frame
brings the saved images to 1266 × 740. Edit `breeze_frame.svg` to change the window
decoration; `window_frame.dart` adds the app icon and centered title. The frame is
composited after capture so modal dialogs only dim the app content.

The four window controls use SVGs from KDE Breeze icons:
`pin.svg`, `window-minimize.svg`, `window-maximize.svg`, and `window-close.svg`.
They are drawn at native size, recolored to match the frame, and placed on whole
pixels in centered 16px canvases to preserve their stroke weight.
[Breeze icons](https://develop.kde.org/frameworks/breeze-icons/) copyright KDE and
licenced under the GNU LGPL version 3 or later. The license texts are included in
[`licenses/`](licenses/).

The app details cover is drawn locally; video playback is outside the capture.
Task messages, step counts, and indeterminate progress match the Rust task code
in `native/hub/src/task/`. Keep the fixtures in sync when those formats change.
Use the same Flutter version between runs for consistent rendering; font and
engine updates can change pixels.

`generate_test.dart` feeds fixture messages through the generated bindings and
opens each view. To add a screenshot, navigate to the page, supply any requested
data, check that its content is visible, and call `capture` with the filename.
Use fixed frame durations for screens with continuous progress animations.

The runner writes a temporary package configuration that substitutes
`rinf/lib/rinf.dart` for the native transport. The substitute accepts only the read
requests used by these scenes and fails on unexpected requests. The application's
normal package configuration and production code stay unchanged. Run this test
through the runner so the substitute and UTC timezone are installed.

The generator does not compare pixels against a golden baseline. Normal
`flutter test` runs do not rewrite documentation screenshots.
