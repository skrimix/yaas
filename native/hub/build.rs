fn main() {
    println!("cargo:rerun-if-changed=./assets");
    built::write_built_file().expect("Failed to acquire build-time information")
}
