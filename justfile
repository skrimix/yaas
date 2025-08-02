# Run the app
run:
    flutter run

# Run the app in release mode
run-release:
    flutter run --release

# Run the app in profile mode
run-profile:
    flutter run --profile

# Build the app for Linux in debug mode
build:
    flutter build linux --debug

# Build the app for Linux in release mode
build-release:
    flutter build linux --release

# Build the app for Linux in profile mode
build-profile:
    flutter build linux --profile

# Run all tests
test:
    cargo test

# Run all tests, including ignored ones
test-all:
    cargo test -- --include-ignored


