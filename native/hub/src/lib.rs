//! This `hub` crate is the
//! entry point of the Rust logic.

use std::sync::Arc;

use adb::AdbHandler;
use mimalloc::MiMalloc;

mod messages;
mod sample_functions;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

rinf::write_interface!();

pub mod adb;
pub mod models;
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // tokio::spawn(sample_functions::communicate());
    let adb_handler = AdbHandler::new();
    AdbHandler::start_device_monitor(Arc::new(adb_handler)).await;

    // Keep the main function running until Dart shutdown.
    rinf::dart_shutdown().await;
}
