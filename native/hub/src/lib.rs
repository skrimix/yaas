//! This `hub` crate is the
//! entry point of the Rust logic.

use adb::AdbHandler;
use anyhow::{Context, Result};
use downloader::Downloader;
use messages::RustPanic;
use mimalloc::MiMalloc;
use task::TaskManager;
use tracing::Level;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::fmt::format::{self, FmtSpan};

mod messages;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

rinf::write_interface!();

pub mod adb;
pub mod downloader;
pub mod models;
pub mod task;
pub mod utils;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let message = format!("{panic_info}\n{backtrace}");
        RustPanic { message }.send_signal_to_dart();
        original_hook(panic_info);
    }));

    // set current working directory to the directory of the executable
    let current_dir = std::env::current_exe().expect("Failed to get current executable");
    std::env::set_current_dir(current_dir.parent().expect("Failed to get parent directory"))
        .expect("Failed to set current working directory");

    let _guard = setup_logging();
    if let Err(e) = _guard {
        rinf::debug_print!("Failed to setup logging: {e}");
    }

    let adb_handler = AdbHandler::new();
    let downloader = Downloader::new().await;
    let _task_manager = TaskManager::new(adb_handler.clone(), downloader.clone());

    // Keep the main function running until Dart shutdown.
    rinf::dart_shutdown().await;
}

fn setup_logging() -> Result<WorkerGuard> {
    // log to file
    std::fs::create_dir_all("logs").context("Failed to create logs directory")?;
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(10)
        .build("logs/rql_native.log")
        .context("Failed to initialize file appender")?;
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_max_level(Level::DEBUG)
        .event_format(format::Format::default().compact().with_source_location(true))
        .with_writer(non_blocking)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global subscriber")?;
    Ok(_guard)
}
