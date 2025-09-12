# Repository Guidelines

## Project Overview

YAAS is a Flutter application with Rust backend integration using the Rinf framework. It's a cross-platform desktop application for managing Android devices via ADB, with features for app management, sideloading, and cloud app downloads.

## Architecture

This is a hybrid Flutter-Rust application:

- **Frontend**: Flutter (Dart) with Provider for state management
- **Backend**: Rust (`native/hub` crate) integrated via Rinf framework
- **Communication**: Rinf handles Flutter-Rust message passing through generated bindings
- **Build System**: Uses `just` for task automation, Flutter for frontend builds, Cargo for Rust compilation

Key architectural components:
- `lib/main.dart`: Entry point with Provider setup for state management
- `native/hub/src/lib.rs`: Rust backend entry point with async runtime
- `lib/src/bindings/`: Auto-generated Dart-Rust communication layer
- `lib/providers/`: State management (DeviceState, AdbState, CloudAppsState, TaskState, SettingsState)
- `lib/widgets/`: UI components
- `native/hub/src/models/`: Rust data models and signal definitions

## Build, Test, and Development Commands
- `just run`: Generate Rinf bindings and run the app in debug.
- `just run-release` | `just run-profile`: Release/profile runs.
- `just build` | `just build-release`: Build Linux desktop binaries.
- `just gen`: Regenerate Rinf bindings (Dart/Rust FFI stubs).
- `just test` | `just test-all`: Run Rust unit tests (`cargo test`). `test-all` includes ignored tests.
- `flutter analyze`: Static analysis; keep output clean.
- `just format` | `just format-dart` | `just format-rust`: Auto-format code.
- `cargo clippy`: Lint Rust code.

## Code Generation

The project uses Rinf for automatic code generation:
- Rust signal definitions in `native/hub/src/models/signals/` generate corresponding Dart classes
- Always run `just gen` after modifying Rust signals before Flutter operations
- Generated files are in `lib/src/bindings/`

### Localization (ARB → Dart)
- Edit only the ARB files in `lib/l10n` when adding or changing localized strings.
- Do not modify any generated localization Dart files in `lib/src/l10n` directly.
- Generate implementations with `flutter gen-l10n` (configured via `l10n.yaml`).
- Ensure `flutter analyze` passes after regeneration.

## Dependencies

**Flutter key packages:**
- `rinf`: Flutter-Rust integration framework
- `provider`: State management
- `flutter_svg`: SVG rendering
- `desktop_drop`: Drag-and-drop support
- `file_picker`: File selection dialogs

**Rust key crates:**
- `rinf`: Rust side of Flutter integration
- `tokio`: Async runtime
- `forensic-adb`: Custom ADB implementation
- `anyhow`: Error handling
- `tracing`: Logging

## State Management

Uses Provider pattern with these main states:
- `DeviceState`: Connected Android device information
- `AdbStateProvider`: ADB connection and command status
- `CloudAppsState`: Available cloud applications
- `TaskState`: Background task management
- `SettingsState`: Application settings

State flows from Rust backend to Flutter frontend via Rinf signals.

## Coding Style & Naming Conventions
- Dart: Follow `flutter_lints` (see `analysis_options.yaml`). Use `just format-dart` (`dart format .`).
  - Files `lower_snake_case.dart`; classes `PascalCase`; members `lowerCamelCase`.
- Rust: Use `rustfmt` (nightly, via `just format-rust`/`cargo +nightly fmt`).
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
- Do not commit secrets or user data.
- All inputs, dependencies, and remote file repositories are trusted.
- Cross‑platform: confirm behavior on Linux/Windows/macOS when touching platform code.
