//! This `hub` crate is the
//! entry point of the Rust logic.

use adb::AdbHandler;
use messages::RustPanic;
use mimalloc::MiMalloc;

mod messages;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

rinf::write_interface!();

pub mod adb;
pub mod models;
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let message = format!("{panic_info}\n{backtrace}");
        rinf::debug_print!("RUST PANIC: {message}");
        RustPanic { message }.send_signal_to_dart();
        original_hook(panic_info);
    }));

    let _adb_handler = AdbHandler::create();

    // Keep the main function running until Dart shutdown.
    rinf::dart_shutdown().await;
}
