//! This `hub` crate is the
//! entry point of the Rust logic.

use std::sync::Arc;

use adb::AdbHandler;
use mimalloc::MiMalloc;

mod messages;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

rinf::write_interface!();

pub mod adb;
pub mod models;
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // tokio::spawn(sample_functions::communicate());
    let _adb_handler = AdbHandler::create();

    // Keep the main function running until Dart shutdown.
    rinf::dart_shutdown().await;
}
