# Repository Guidelines

## Project Structure & Module Organization
- `lib/`: Flutter UI, providers, widgets, and app entry (`main.dart`).
- `native/hub`: Rust crate integrated via Rinf (FFI bridge, signals, logic).
- `assets/`: Static assets (SVG/PNG). Update `pubspec.yaml` when adding.
- Platform folders: `linux/`, `macos/`, `windows/` for desktop builds.
- Root tooling: `justfile` (common tasks), `analysis_options.yaml` (lints).

## Build, Test, and Development Commands
- `just run`: Generate Rinf bindings and run the app in debug.
- `just run-release` | `just run-profile`: Release/profile runs.
- `just build` | `just build-release`: Build Linux desktop binaries.
- `just gen`: Regenerate Rinf bindings (Dart/Rust FFI stubs).
- `just test`: Run Rust unit tests (`cargo test`).
- `flutter analyze`: Static analysis; keep output clean.
- `just format` | `just format-dart` | `just format-rust`: Auto-format code.

## Coding Style & Naming Conventions
- Dart: Follow `flutter_lints` (see `analysis_options.yaml`). Use `dart format .`.
  - Files `lower_snake_case.dart`; classes `PascalCase`; members `lowerCamelCase`.
- Rust: Use `rustfmt` (nightly, via `just format-rust`).
  - Modules `snake_case`; types `PascalCase`; constants `SCREAMING_SNAKE_CASE`.
- Prefer small providers/widgets; keep FFI boundaries in `native/hub`.

## Testing Guidelines
- Rust: Add fast unit tests in-module with `#[cfg(test)]` or integration tests.
  - Run with `just test` (use `just test-all` to include ignored tests).
- Flutter: Place tests under `test/`; run with `flutter test` when added.
- Aim for meaningful coverage around parsing, signals, and provider logic.

## Commit & Pull Request Guidelines
- Commits: Imperative, concise, scoped when helpful. Examples:
  - "Fix ADB server launch on Windows"
  - "Sort apps by relevance when searching"
- PRs: Include description, linked issues, before/after screenshots for UI, and platforms tested.
  - Ensure `just format` and `flutter analyze` pass.
  - Run `just gen` locally; do not commit build artifacts. Commit generated bindings only if reviewed as source of truth.

## Security & Configuration Tips
- Do not commit secrets or user data. Configure ADB path inside app settings.
- Cross‑platform: confirm behavior on Linux/Windows/macOS when touching platform code.
