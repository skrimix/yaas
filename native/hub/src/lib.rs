//! This `hub` crate is the
//! entry point of the Rust logic.

use adb::AdbHandler;
use anyhow::{Context, Result};
use downloader::Downloader;
use logging::SignalLayer;
use mimalloc::MiMalloc;
use models::signals::system::{MediaConfigChanged, RustPanic};
use rinf::RustSignal;
use settings::SettingsHandler;
use task::TaskManager;
use tokio_stream::wrappers::WatchStream;
use tracing::{error, info};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{fmt, layer::SubscriberExt};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

rinf::write_interface!();

pub mod adb;
pub mod apk;
pub mod backups;
pub mod downloader;
pub mod downloads;
pub mod logging;
pub mod models;
pub mod settings;
pub mod task;
pub mod utils;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

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
        data_dir.join("com.github.skrimix.yaas")
    } else {
        data_dir.join("YAAS")
    };
    if !app_dir.exists() {
        std::fs::create_dir(&app_dir).expect("Failed to create app directory");
    }
    std::env::set_current_dir(&app_dir).expect("Failed to set current working directory");

    let _guard = setup_logging();
    if let Err(e) = _guard {
        rinf::debug_print!("Failed to setup logging: {:#}", e);
    }

    info!("Starting YAAS backend");

    let settings_handler = SettingsHandler::new(app_dir.clone());

    // Prepare media cache directory and send media configuration to Flutter
    let media_cache_dir = app_dir.join("media_cache");
    if let Err(e) = std::fs::create_dir_all(&media_cache_dir) {
        rinf::debug_print!("Failed to create media cache directory: {:#}", e);
    }
    let media_base_url = "https://webdav.5698452.xyz/media/".to_string();
    MediaConfigChanged { media_base_url, cache_dir: media_cache_dir.display().to_string() }
        .send_signal_to_dart();

    let adb_handler = AdbHandler::new(WatchStream::new(settings_handler.subscribe())).await;
    let downloader = Downloader::new(WatchStream::new(settings_handler.subscribe())).await;
    let downloads_catalog =
        downloads::DownloadsCatalog::start(WatchStream::new(settings_handler.subscribe()));
    let _task_manager = TaskManager::new(
        adb_handler.clone(),
        downloader.clone(),
        downloads_catalog.clone(),
        WatchStream::new(settings_handler.subscribe()),
    );

    // Backups-related requests
    let _backups_handler =
        backups::BackupsHandler::start(WatchStream::new(settings_handler.subscribe()));

    // Log-related requests from Flutter
    SignalLayer::start_request_handler(app_dir.join("logs"));

    // Keep the main function running until Dart shutdown.
    rinf::dart_shutdown().await;
}

fn setup_logging() -> Result<WorkerGuard> {
    // Log to file
    std::fs::create_dir_all("logs").context("Failed to create logs directory")?;
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(10)
        .filename_prefix("yaas")
        .filename_suffix("log")
        .build("logs/yaas_native.log")
        .context("Failed to initialize file appender")?;
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Real-time logging to Flutter
    let (signal_layer, log_receiver) = SignalLayer::new();
    SignalLayer::start_forwarder(log_receiver);

    let subscriber = tracing_subscriber::registry()
        .with(signal_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking)
                // .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .event_format(fmt::format().pretty()),
        )
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global subscriber")?;
    Ok(_guard)
}
