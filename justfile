# Run the app
run:
    rinf gen && flutter run

# Run the app in release mode
run-release:
    rinf gen && flutter run --release

# Run the app in profile mode
run-profile:
    rinf gen && flutter run --profile

# Build the app for Linux in debug mode
build:
    rinf gen && flutter build linux --debug

# Build the app for Linux in release mode
build-release:
    rinf gen && flutter build linux --release

# Build the app for Linux in profile mode
build-profile:
    rinf gen && flutter build linux --profile

# Run all tests
test:
    cargo test

# Run all tests, including ignored ones
test-all:
    cargo test -- --include-ignored


