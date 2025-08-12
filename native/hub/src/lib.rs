//! This `hub` crate is the
//! entry point of the Rust logic.

use adb::AdbHandler;
use anyhow::{Context, Result};
use downloader::Downloader;
use mimalloc::MiMalloc;
use models::signals::system::RustPanic;
use rinf::RustSignal;
use settings::SettingsHandler;
use task::TaskManager;
use tokio_stream::wrappers::WatchStream;
use tracing::{Level, error, info};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::fmt;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

rinf::write_interface!();

pub mod adb;
pub mod downloader;
pub mod models;
pub mod settings;
pub mod task;
pub mod utils;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let message = format!("{panic_info}\n{backtrace}");
        error!(message, "Rust panic");
        RustPanic { message }.send_signal_to_dart();
        original_hook(panic_info);
    }));

    // Set working directory to the app's data directory
    let data_dir = dirs::data_dir().expect("Failed to get data directory");
    let app_dir = if cfg!(target_os = "macos") {
        data_dir.join("com.github.skrimix.RQL")
    } else {
        data_dir.join("RQL")
    };
    if !app_dir.exists() {
        std::fs::create_dir(&app_dir).expect("Failed to create app directory");
    }
    std::env::set_current_dir(&app_dir).expect("Failed to set current working directory");

    let _guard = setup_logging();
    if let Err(e) = _guard {
        rinf::debug_print!("Failed to setup logging: {:#}", e);
    }

    info!("Starting RQL backend");

    let settings_handler = SettingsHandler::new(app_dir);

    let adb_handler = AdbHandler::new(WatchStream::new(settings_handler.subscribe())).await;
    let downloader = Downloader::new(WatchStream::new(settings_handler.subscribe())).await;
    let _task_manager = TaskManager::new(adb_handler.clone(), downloader.clone());

    // Keep the main function running until Dart shutdown.
    rinf::dart_shutdown().await;
}

fn setup_logging() -> Result<WorkerGuard> {
    // Log to file
    std::fs::create_dir_all("logs").context("Failed to create logs directory")?;
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(10)
        .filename_prefix("rql")
        .filename_suffix("log")
        .build("logs/rql_native.log")
        .context("Failed to initialize file appender")?;
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::DEBUG)
        .with_ansi(false) // Disable ANSI colors
        .event_format(fmt::format().pretty())
        .with_writer(non_blocking)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global subscriber")?;
    Ok(_guard)
}
