# Run the app
run:
    rinf gen && flutter run

# Run the app in release mode
run-release:
    rinf gen && flutter run --release

# Run the app in profile mode
run-profile:
    rinf gen && flutter run --profile

# Generate rinf bindings
gen:
    rinf gen

# Build the app for Linux in debug mode
build:
    rinf gen && flutter build linux --debug

# Build the app for Linux in release mode
build-release:
    rinf gen && flutter build linux --release

# Build the app for Linux in profile mode
build-profile:
    rinf gen && flutter build linux --profile

# Build Linux AppImage artifact into dist/yaas.AppImage
build-appimage:
    ./scripts/build_appimage.sh

# Run all tests
test:
    cargo test

# Run all tests, including ignored ones
test-all:
    cargo test -- --include-ignored

# Format Rust code
format-rust:
    cargo +nightly fmt

# Format Dart code
format-dart:
    dart format .

# Format all code
format:
    just format-rust
    just format-dart

# Create app icons using app_icon.svg
create-icons:
    magick -background none assets/svg/app_icon.svg assets/png/app_icon.png && dart run icons_launcher:create

# Run cargo check with windows target
check-windows:
    command -v x86_64-w64-mingw32-gcc > /dev/null || (echo "x86_64-w64-mingw32-gcc tool not installed" && exit 1)
    cargo check --target x86_64-pc-windows-gnu
