# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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

## Development Commands

**Building and Running:**
- `just run`: Generate bindings and run in debug mode
- `just run-release`: Generate bindings and run in release mode
- `just build`: Generate bindings and build Linux debug
- `just build-release`: Generate bindings and build Linux release
- `just gen`: Generate bindings

**Testing:**
- `just test`: Run Rust tests
- `just test-all`: Run all Rust tests including ignored ones
- `flutter test`: Run Flutter/Dart tests

**Formatting:**
- `just format`: Format all code
- `just format-rust`: Format Rust code
- `just format-dart`: Format Dart code

**Analysis:**
- `flutter analyze`: Lint Dart code
- `cargo clippy`: Lint Rust code

**Key Notes:**
- Always run `rinf gen` before Flutter commands to generate fresh bindings
- Always format code after completing a task (`just format` or `just format-rust` or `just format-dart`)
- The `hub` crate name cannot be changed (required by Rinf)
- Uses `just` as the primary build tool instead of direct Flutter/Cargo commands
- Call `cargo check` directly when encountering build issues to get more detailed error messages

## Code Generation

The project uses Rinf for automatic code generation:
- Rust signal definitions in `native/hub/src/models/signals/` generate corresponding Dart classes
- Always run `just gen` after modifying Rust signals before Flutter operations
- Generated files are in `lib/src/bindings/`

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