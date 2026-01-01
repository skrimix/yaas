//! This `hub` crate is the
//! entry point of the Rust logic.

use std::{
    panic::catch_unwind,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use adb::AdbHandler;
use anyhow::{Context, Result};
use logging::SignalLayer;
use mimalloc::MiMalloc;
use models::signals::system::{AppVersionInfo, MediaConfigChanged, RustPanic};
use rinf::RustSignal;
use settings::SettingsHandler;
use task::TaskManager;
use tokio::{sync::Notify, time::timeout};
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, info, instrument};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt};

use crate::{
    backups_catalog::BackupsCatalog,
    casting::CastingManager,
    downloader::{downloads_catalog::DownloadsCatalog, manager::DownloaderManager},
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

// Keep logging guard alive for the whole process lifetime
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

rinf::write_interface!();

pub(crate) mod adb;
pub(crate) mod apk;
pub(crate) mod archive;
pub(crate) mod backups_catalog;
pub(crate) mod casting;
pub(crate) mod downloader;
pub(crate) mod logging;
pub(crate) mod models;
pub(crate) mod settings;
pub(crate) mod task;
pub(crate) mod utils;

pub(crate) mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub(crate) const USER_AGENT: &str = concat!("YAAS/", env!("CARGO_PKG_VERSION"));

fn main() {
    let portable_mode = std::env::args().any(|arg| arg == "--portable");

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

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();

    let _ = catch_unwind(|| {
        runtime.block_on(async move {
            let init_start = Instant::now();
            // Initialize everything
            timeout(Duration::from_secs(10), init(portable_mode))
                .await
                .expect("Core initialization timed out");
            info!("Core initialization completed in {:?}", init_start.elapsed());

            tokio::select! {
                _ = rinf::dart_shutdown() => {},
                _ = panic_notify.notified() => {},
            }
        })
    });

    runtime.shutdown_timeout(Duration::from_secs(3));
}

#[instrument]
async fn init(portable_mode: bool) {
    // Set working directory to the app's data directory
    let app_dir = resolve_app_dir(portable_mode);
    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir).expect("Failed to create app directory");
    }
    std::env::set_current_dir(&app_dir).expect("Failed to set current working directory");

    if let Err(e) = setup_logging() {
        rinf::debug_print!("Failed to setup logging: {:#}", e);
    }
    // Log and send version/build info
    info!(
        "Starting YAAS core {}| version={} | commit={}{} | profile={} | rustc={} | built={}",
        if portable_mode { "(portable mode)" } else { "" },
        built_info::PKG_VERSION,
        built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("unknown"),
        if built_info::GIT_DIRTY.unwrap_or(false) { " (dirty)" } else { "" },
        built_info::PROFILE,
        built_info::RUSTC_VERSION,
        built_info::BUILT_TIME_UTC
    );
    AppVersionInfo {
        core_version: built_info::PKG_VERSION.to_string(),
        profile: built_info::PROFILE.to_string(),
        rustc_version: built_info::RUSTC_VERSION.to_string(),
        built_time_utc: built_info::BUILT_TIME_UTC.to_string(),
        git_commit_hash: built_info::GIT_COMMIT_HASH.map(|s| s.to_string()),
        git_commit_hash_short: built_info::GIT_COMMIT_HASH_SHORT.map(|s| s.to_string()),
        git_dirty: built_info::GIT_DIRTY,
    }
    .send_signal_to_dart();

    debug!("Creating settings handler");
    let settings_handler = SettingsHandler::new(app_dir.clone(), portable_mode)
        .expect("Failed to create settings handler");

    // Prepare media cache directory and send media configuration to Flutter
    let media_cache_dir = app_dir.join("media_cache");
    if let Err(e) = std::fs::create_dir_all(&media_cache_dir) {
        rinf::debug_print!("Failed to create media cache directory: {:#}", e);
    }
    let media_base_url = "https://webdav.5698452.xyz/media/".to_string();
    MediaConfigChanged { media_base_url, cache_dir: media_cache_dir.display().to_string() }
        .send_signal_to_dart();

    debug!("Creating adb handler");
    let adb_handler = AdbHandler::new(WatchStream::new(settings_handler.subscribe())).await;
    debug!("Creating downloads catalog");
    let downloads_catalog = DownloadsCatalog::new(WatchStream::new(settings_handler.subscribe()));
    debug!("Creating downloader manager");
    let downloader_manager = DownloaderManager::new(None);
    debug!("Creating task manager");
    let _task_manager = TaskManager::new(
        adb_handler.clone(),
        downloader_manager.clone(),
        downloads_catalog.clone(),
        WatchStream::new(settings_handler.subscribe()),
    );
    debug!("Starting downloader manager");
    downloader_manager.clone().start(app_dir.clone(), settings_handler.clone());

    // Backups-related requests
    debug!("Creating backups catalog");
    let _backups_handler = BackupsCatalog::start(WatchStream::new(settings_handler.subscribe()));

    // Casting-related requests (Windows-only)
    debug!("Creating casting manager");
    CastingManager::start();

    // Log-related requests from Flutter
    debug!("Starting signal layer request handler");
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

fn resolve_portable_app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            let exe_path = PathBuf::from(appimage);
            if let Some(dir) = exe_path.parent() {
                return Some(dir.join("_portable_data"));
            }
        }
    }

    let exe_path = std::env::current_exe().ok()?;
    let dir = exe_path.parent()?;
    Some(dir.join("_portable_data"))
}

fn resolve_app_dir(portable_mode: bool) -> PathBuf {
    if portable_mode && cfg!(any(target_os = "windows", target_os = "linux")) {
        if let Some(portable_dir) = resolve_portable_app_dir() {
            return portable_dir;
        } else {
            panic!("--portable requested but failed to resolve executable path");
        }
    }

    let data_dir = dirs::data_dir().expect("Failed to get data directory");
    if cfg!(target_os = "macos") {
        data_dir.join("io.github.skrimix.yaas")
    } else {
        data_dir.join("YAAS")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::init;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn core_init_stable() {
        init(true).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
