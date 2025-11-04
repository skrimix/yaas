//! This `hub` crate is the
//! entry point of the Rust logic.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, OnceLock},
    time::Duration,
};

use adb::AdbHandler;
use anyhow::{Context, Result};
use logging::SignalLayer;
use mimalloc::MiMalloc;
use models::signals::system::{AppVersionInfo, MediaConfigChanged, RustPanic};
use rinf::RustSignal;
use settings::SettingsHandler;
use task::TaskManager;
use tokio::sync::Notify;
use tokio_stream::wrappers::WatchStream;
use tracing::{error, info};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt};

use crate::{
    backups_list::BackupsListHandler, casting::CastingManager,
    downloader_manager::DownloaderManager, downloads_catalog::DownloadsCatalog,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

// Keep logging guard alive for the whole process lifetime
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

rinf::write_interface!();

pub mod adb;
pub mod apk;
pub mod archive;
pub mod backups_list;
pub mod casting;
pub mod downloader;
pub mod downloader_manager;
pub mod downloads_catalog;
pub mod logging;
pub mod models;
pub mod settings;
pub mod task;
pub mod utils;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub const USER_AGENT: &str = concat!("YAAS/", env!("CARGO_PKG_VERSION"));

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();

    let panic_notify = Arc::new(Notify::new());
    let hook_notify = panic_notify.clone();

    // Report all our panics to Flutter
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let message = format!("{panic_info}\n{backtrace}");
        error!(message, "Rust panic");
        RustPanic { message }.send_signal_to_dart();

        // Request shutdown, as we're in an unrecoverable state
        hook_notify.notify_waiters();

        original_hook(panic_info);
    }));

    let _ = catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(async move {
            // Initialize everything
            init().await;

            tokio::select! {
                _ = rinf::dart_shutdown() => {},
                _ = panic_notify.notified() => {},
            }
        })
    }));

    runtime.shutdown_timeout(Duration::from_secs(3));
}

async fn init() {
    // Set working directory to the app's data directory
    let data_dir = dirs::data_dir().expect("Failed to get data directory");
    let app_dir = if cfg!(target_os = "macos") {
        data_dir.join("io.github.skrimix.yaas")
    } else {
        data_dir.join("YAAS")
    };
    if !app_dir.exists() {
        std::fs::create_dir(&app_dir).expect("Failed to create app directory");
    }
    std::env::set_current_dir(&app_dir).expect("Failed to set current working directory");

    if let Err(e) = setup_logging() {
        rinf::debug_print!("Failed to setup logging: {:#}", e);
    }
    // Log and send version/build info
    info!(
        "Starting YAAS backend | version={} | commit={}{} | profile={} | rustc={} | built={}",
        built_info::PKG_VERSION,
        built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("unknown"),
        if built_info::GIT_DIRTY.unwrap_or(false) { " (dirty)" } else { "" },
        built_info::PROFILE,
        built_info::RUSTC_VERSION,
        built_info::BUILT_TIME_UTC
    );
    AppVersionInfo {
        backend_version: built_info::PKG_VERSION.to_string(),
        profile: built_info::PROFILE.to_string(),
        rustc_version: built_info::RUSTC_VERSION.to_string(),
        built_time_utc: built_info::BUILT_TIME_UTC.to_string(),
        git_commit_hash: built_info::GIT_COMMIT_HASH.map(|s| s.to_string()),
        git_commit_hash_short: built_info::GIT_COMMIT_HASH_SHORT.map(|s| s.to_string()),
        git_dirty: built_info::GIT_DIRTY,
    }
    .send_signal_to_dart();

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
    let downloads_catalog = DownloadsCatalog::start(WatchStream::new(settings_handler.subscribe()));
    let downloader_manager = DownloaderManager::new(None);
    let _task_manager = TaskManager::new(
        adb_handler.clone(),
        downloader_manager.clone(),
        downloads_catalog.clone(),
        WatchStream::new(settings_handler.subscribe()),
    );
    downloader_manager.clone().start(app_dir.clone(), settings_handler.clone());

    // Backups-related requests
    let _backups_handler =
        BackupsListHandler::start(WatchStream::new(settings_handler.subscribe()));

    // Casting-related requests (Windows-only)
    CastingManager::start();

    // Log-related requests from Flutter
    SignalLayer::start_request_handler(app_dir.join("logs"));
}

fn setup_logging() -> Result<()> {
    // Log to file
    std::fs::create_dir_all("logs").context("Failed to create logs directory")?;
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(10)
        .filename_prefix("yaas")
        .filename_suffix("log")
        .build("logs/yaas_native")
        .context("Failed to initialize file appender")?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

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
        .with(EnvFilter::new("debug,hyper_util=info"));

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global subscriber")?;

    let _ = LOG_GUARD.set(guard);
    Ok(())
}
