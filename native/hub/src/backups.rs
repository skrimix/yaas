use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use glob::glob;
use rinf::{DartSignal, RustSignal};
use tokio::fs;
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tracing::{Span, debug, error, info, instrument, trace};

use crate::models::{Settings, signals::backups::*};

/// Handles backup-related requests (list, delete)
#[derive(Debug, Clone)]
pub struct BackupsHandler {
    backups_dir: Arc<tokio::sync::RwLock<PathBuf>>,
}

impl BackupsHandler {
    pub fn start(mut settings_stream: WatchStream<Settings>) -> Arc<Self> {
        let initial_settings = futures::executor::block_on(settings_stream.next())
            .expect("Settings stream closed on backups handler init");

        let handler = Arc::new(Self {
            backups_dir: Arc::new(tokio::sync::RwLock::new(PathBuf::from(
                initial_settings.backups_location,
            ))),
        });

        // Watch settings updates
        {
            let handler = handler.clone();
            tokio::spawn(async move {
                while let Some(settings) = settings_stream.next().await {
                    info!(dir = %settings.backups_location, "Backups location updated");
                    *handler.backups_dir.write().await = PathBuf::from(settings.backups_location);
                }
                panic!("Settings stream closed for BackupsHandler");
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
        let list_receiver = GetBackupsRequest::get_dart_signal_receiver();
        let delete_receiver = DeleteBackupRequest::get_dart_signal_receiver();
        let get_dir_receiver = GetBackupsDirectoryRequest::get_dart_signal_receiver();

        loop {
            tokio::select! {
                // Handle list backup requests
                signal = list_receiver.recv() => {
                    if let Some(_signal) = signal {
                        debug!("Received GetBackupsRequest");
                        match self.list_backups().await {
                            Ok(mut entries) => {
                                // Newest first
                                entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                                GetBackupsResponse { entries, error: None }.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(error = %format!("{e:#}"), "Failed to list backups");
                                GetBackupsResponse { entries: vec![], error: Some(format!("{e:#}")) }
                                    .send_signal_to_dart();
                            }
                        }
                    } else {
                        panic!("GetBackupsRequest receiver ended");
                    }
                }

                // Handle delete backup requests
                request = delete_receiver.recv() => {
                    if let Some(request) = request {
                        let path = request.message.path.clone();
                        debug!(%path, "Received DeleteBackupRequest");
                        let result = self.delete_backup(Path::new(&path)).await;
                        match result {
                            Ok(()) => {
                                info!(%path, "Deleted backup successfully");
                                DeleteBackupResponse { path, error: None }.send_signal_to_dart();
                                BackupsChanged {}.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(%path, error = %format!("{e:#}"), "Failed to delete backup");
                                DeleteBackupResponse { path, error: Some(format!("{e:#}")) }
                                    .send_signal_to_dart();
                            }
                        }
                    } else {
                        panic!("DeleteBackupRequest receiver ended");
                    }
                }

                // Handle get directory requests
                _request = get_dir_receiver.recv() => {
                    if let Some(_request) = _request {
                        let dir = self.backups_dir.read().await.clone();
                        debug!(dir = %dir.display(), "Sending backups directory path");
                        GetBackupsDirectoryResponse { path: dir.to_string_lossy().into_owned() }
                            .send_signal_to_dart();
                    } else {
                        panic!("GetBackupsDirectoryRequest receiver ended");
                    }
                }
            }
        }
    }

    #[instrument(skip(self), err)]
    async fn list_backups(&self) -> Result<Vec<BackupEntry>> {
        let dir = self.backups_dir.read().await.clone();
        let dir_path = Path::new(&dir);
        debug!(dir = %dir_path.display(), "Listing backups in directory");
        ensure!(
            dir_path.exists() && dir_path.is_dir(),
            "Backups directory does not exist: {}",
            dir_path.display()
        );

        let mut entries = Vec::new();
        let pattern = dir_path.join("*").join(".backup").to_string_lossy().to_string();
        for item in glob(&pattern).context("Invalid glob pattern for backups scan")? {
            match item {
                Ok(marker_path) => {
                    if let Some(dir) = marker_path.parent() {
                        trace!(path = %dir.display(), "Found backup candidate");
                        if let Some(entry) = self.build_entry(dir).await? {
                            entries.push(entry);
                        }
                    }
                }
                Err(e) => {
                    debug!(error = %e, "Glob match error while scanning backups");
                }
            }
        }
        debug!(count = entries.len(), "Finished scanning backups");
        Ok(entries)
    }

    #[instrument(skip(self), fields(dir = %dir.display()), err)]
    async fn build_entry(&self, dir: &Path) -> Result<Option<BackupEntry>> {
        if !dir.is_dir() {
            return Ok(None);
        }
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| dir.to_string_lossy().into_owned());

        let mut timestamp = 0u64;
        let mut display_name = name.clone();

        // Parse prefix: YYYY-MM-DD_HH-MM-SS_...
        if name.len() > 20 && name.as_bytes()[19] == b'_' {
            let ts_str = &name[0..19];
            display_name = name[20..].to_string();
            let parts: Vec<&str> = ts_str.split(|c: char| !c.is_ascii_digit()).collect();
            if parts.len() >= 6
                && let (Ok(y), Ok(m), Ok(d), Ok(h), Ok(min), Ok(s)) = (
                    parts[0].parse::<i32>(),
                    parts[1].parse::<u32>(),
                    parts[2].parse::<u32>(),
                    parts[3].parse::<u32>(),
                    parts[4].parse::<u32>(),
                    parts[5].parse::<u32>(),
                )
            {
                // Convert to unix millis using chrono-less approach
                // Use time crate would be nicer, but avoid extra deps here
                // Fallback to file mtime if conversion fails
                // TODO: use time crate
                // TODO: log errors
                timestamp = datetime_to_unix_millis(y, m, d, h, min, s).unwrap_or(0);
            }
        }

        if timestamp == 0 {
            // Fallback to directory modified time
            if let Ok(meta) = fs::metadata(dir).await
                && let Ok(modified) = meta.modified()
            {
                timestamp = system_time_to_millis(modified);
            }
        }

        // Part flags (check existence quickly)
        let has_apk = has_any_apk_immediate(dir).await?;
        let has_private_data = dir.join("data_private").exists();
        let has_shared_data = dir.join("data").exists();
        let has_obb = dir.join("obb").exists();
        let total_size = dir_size(dir).await.unwrap_or(0);

        trace!(
            name = %display_name,
            ts_millis = timestamp,
            total_size,
            has_apk,
            has_private_data,
            has_shared_data,
            has_obb,
            "Built backup entry"
        );

        Ok(Some(BackupEntry {
            path: dir.to_string_lossy().to_string(),
            name: display_name,
            timestamp,
            total_size,
            has_apk,
            has_private_data,
            has_shared_data,
            has_obb,
        }))
    }

    #[instrument(skip(self))]
    async fn delete_backup(&self, path: &Path) -> Result<()> {
        // Security: ensure path is inside backups directory
        let root = self.backups_dir.read().await.clone();
        trace!("Canonicalizing paths for deletion");
        let canon_root = fs::canonicalize(root).await?;
        let canon_req = fs::canonicalize(path).await?;
        debug!(root = %canon_root.display(), target = %canon_req.display(), "Canonicalized paths for deletion");

        ensure!(canon_req.starts_with(&canon_root), "Requested path is outside backups directory");
        ensure!(canon_req.is_dir(), "Backup path is not a directory");
        ensure!(canon_req.join(".backup").exists(), "Backup marker not found (.backup)");

        info!(path = %canon_req.display(), "Deleting backup directory");
        fs::remove_dir_all(&canon_req).await.context("Failed to delete backup directory")?;
        Ok(())
    }
}

fn system_time_to_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn datetime_to_unix_millis(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> Option<u64> {
    let month: u8 = m.try_into().ok()?;
    let date =
        time::Date::from_calendar_date(y, time::Month::try_from(month).ok()?, d.try_into().ok()?)
            .ok()?;
    let time_of_day =
        time::Time::from_hms(h.try_into().ok()?, min.try_into().ok()?, s.try_into().ok()?).ok()?;
    let odt = time::PrimitiveDateTime::new(date, time_of_day).assume_offset(time::UtcOffset::UTC);
    Some((odt.unix_timestamp_nanos() / 1_000_000) as u64)
}

#[instrument(level = "debug", err)]
async fn has_any_apk_immediate(dir: &Path) -> Result<bool> {
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        let p = entry.path();
        if entry.file_type().await?.is_file()
            && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("apk"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[instrument(level = "debug", fields(dir = %dir.display(), size), err)]
async fn dir_size(dir: &Path) -> Result<u64> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let mut rd = match fs::read_dir(&path).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        while let Some(entry) = rd.next_entry().await? {
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
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
