use std::{path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use anyhow::{Context, Result};
use rinf::{DartSignal, RustSignal};
use tokio::fs;
use tokio_stream::{wrappers::WatchStream, StreamExt};
use tracing::{debug, error, info, instrument, trace, Span};

use crate::models::{Settings, signals::downloads_local::*};

#[derive(Debug, Clone)]
pub struct DownloadsHandler {
    root: Arc<tokio::sync::RwLock<PathBuf>>,
}

impl DownloadsHandler {
    pub fn start(mut settings_stream: WatchStream<Settings>) -> Arc<Self> {
        let initial_settings = futures::executor::block_on(settings_stream.next())
            .expect("Settings stream closed on downloads handler init");

        let handler = Arc::new(Self {
            root: Arc::new(tokio::sync::RwLock::new(PathBuf::from(
                initial_settings.downloads_location,
            ))),
        });

        // Watch settings updates
        {
            let handler = handler.clone();
            tokio::spawn(async move {
                while let Some(settings) = settings_stream.next().await {
                    info!(dir = %settings.downloads_location, "Downloads location updated");
                    *handler.root.write().await = PathBuf::from(settings.downloads_location);
                }
                panic!("Settings stream closed for DownloadsHandler");
            });
        }

        // Start signal receivers
        {
            let handler = handler.clone();
            tokio::spawn(async move { handler.receive_signals().await });
        }

        handler
    }

    #[instrument(skip(self))]
    async fn receive_signals(self: Arc<Self>) {
        let list_receiver = GetDownloadsRequest::get_dart_signal_receiver();
        let get_dir_receiver = GetDownloadsDirectoryRequest::get_dart_signal_receiver();

        loop {
            tokio::select! {
                signal = list_receiver.recv() => {
                    if let Some(_signal) = signal {
                        debug!("Received GetDownloadsRequest");
                        match self.list_downloads().await {
                            Ok(mut entries) => {
                                entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                                GetDownloadsResponse { entries, error: None }.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(error = %format!("{e:#}"), "Failed to list downloads");
                                GetDownloadsResponse { entries: vec![], error: Some(format!("{e:#}")) }.send_signal_to_dart();
                            }
                        }
                    } else {
                        panic!("GetDownloadsRequest receiver ended");
                    }
                }
                request = get_dir_receiver.recv() => {
                    if let Some(_request) = request {
                        let dir = self.root.read().await.clone();
                        debug!(dir = %dir.display(), "Sending downloads directory path");
                        GetDownloadsDirectoryResponse { path: dir.to_string_lossy().into_owned() }.send_signal_to_dart();
                    } else {
                        panic!("GetDownloadsDirectoryRequest receiver ended");
                    }
                }
            }
        }
    }

    #[instrument(skip(self), err)]
    async fn list_downloads(&self) -> Result<Vec<DownloadEntry>> {
        let root = self.root.read().await.clone();
        let mut entries: Vec<DownloadEntry> = Vec::new();
        let mut rd = fs::read_dir(&root).await.with_context(|| format!("Failed to read {}", root.display()))?;
        while let Some(entry) = rd.next_entry().await? {
            let p = entry.path();
            let meta = match entry.metadata().await { Ok(m) => m, Err(_) => continue };
            if !meta.is_dir() { continue; }
            if let Some(e) = self.try_build_download_entry(&p).await? {
                entries.push(e);
            }
        }
        Ok(entries)
    }

    #[instrument(skip(self), fields(dir = %dir.display()), err)]
    async fn try_build_download_entry(&self, dir: &Path) -> Result<Option<DownloadEntry>> {
        if !dir.is_dir() { return Ok(None); }

        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if name.is_empty() { return Ok(None); }

        // release.json is optional, use it when available
        let release_path = dir.join("release.json");
        let mut package_name: Option<String> = None;
        let mut version_code: Option<u32> = None;
        let mut ts_millis: u64 = 0;
        if release_path.exists() {
            if let Ok(text) = fs::read_to_string(&release_path).await {
                #[derive(serde::Deserialize)]
                struct ReleaseMetaPartial { downloaded_at: Option<String>, package_name: Option<String>, version_code: Option<u32> }
                if let Ok(meta) = serde_json::from_str::<ReleaseMetaPartial>(&text) {
                    package_name = meta.package_name;
                    version_code = meta.version_code;
                    if let Some(dt) = meta.downloaded_at { ts_millis = rfc3339_to_millis(&dt); }
                }
            }
        }

        if ts_millis == 0 {
            if let Ok(meta) = fs::metadata(dir).await
                && let Ok(modified) = meta.modified()
            { ts_millis = system_time_to_millis(modified); }
        }

        let total_size = dir_size(dir).await.unwrap_or(0);

        trace!(name = %name, ts_millis, total_size, pkg = ?package_name, ver = ?version_code, "Built download entry");
        Ok(Some(DownloadEntry { path: dir.to_string_lossy().to_string(), name, timestamp: ts_millis, total_size, package_name, version_code }))
    }
}

fn system_time_to_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn rfc3339_to_millis(s: &str) -> u64 {
    // Parse RFC3339 in UTC
    match time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        Ok(dt) => (dt.unix_timestamp_nanos() / 1_000_000) as u64,
        Err(_) => 0,
    }
}

#[instrument(level = "debug", fields(dir = %dir.display(), size), err)]
async fn dir_size(dir: &Path) -> Result<u64> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let mut rd = match fs::read_dir(&path).await { Ok(r) => r, Err(_) => continue };
        while let Some(entry) = rd.next_entry().await? {
            let meta = match entry.metadata().await { Ok(m) => m, Err(_) => continue };
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            } else if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Span::current().record("size", total);
    Ok(total)
}

