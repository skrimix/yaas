use std::{
    collections::HashSet,
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use lazy_regex::regex;
use rinf::{DartSignal, RustSignal};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::fs;
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tracing::{Span, debug, error, info, instrument, trace, warn};

use crate::models::{DownloadCleanupPolicy, Settings, signals::downloads_local::*};

#[derive(Debug, Clone)]
pub struct DownloadsCatalog {
    root: Arc<tokio::sync::RwLock<PathBuf>>,
}

impl DownloadsCatalog {
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
                panic!("Settings stream closed");
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
        let delete_receiver = DeleteDownloadRequest::get_dart_signal_receiver();
        let delete_all_receiver = DeleteAllDownloadsRequest::get_dart_signal_receiver();

        loop {
            tokio::select! {
                signal = list_receiver.recv() => {
                    if signal.is_some() {
                        debug!("Received GetDownloadsRequest");
                        match self.list_downloads().await {
                            Ok(mut entries) => {
                                entries.sort_by(|a, b| a.name.cmp(&b.name));
                                GetDownloadsResponse { entries, error: None }.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(error = %format!("{e:#}"), "Failed to list downloads");
                                GetDownloadsResponse { entries: vec![], error: Some(format!("{e:#}")) }.send_signal_to_dart();
                            }
                        }
                    } else {
                        panic!("GetDownloadsRequest receiver closed");
                    }
                }
                request = get_dir_receiver.recv() => {
                    if request.is_some() {
                        debug!("Received GetDownloadsDirectoryRequest");
                        let dir = self.root.read().await.clone();
                        debug!(dir = %dir.display(), "Sending downloads directory path");
                        GetDownloadsDirectoryResponse { path: dir.to_string_lossy().into_owned() }.send_signal_to_dart();
                    } else {
                        panic!("GetDownloadsDirectoryRequest receiver closed");
                    }
                }
                request = delete_receiver.recv() => {
                    if let Some(request) = request {
                        let path = request.message.path.clone();
                        debug!(%path, "Received DeleteDownloadRequest");
                        let result = self.delete_download(Path::new(&path)).await;
                        match result {
                            Ok(()) => {
                                DeleteDownloadResponse { path, error: None }.send_signal_to_dart();
                                DownloadsChanged {}.send_signal_to_dart();
                            }
                            Err(e) => {
                                error!(%path, error = %format!("{e:#}"), "Failed to delete download");
                                DeleteDownloadResponse { path, error: Some(format!("{e:#}")) }.send_signal_to_dart();
                            }
                        }
                    } else {
                        panic!("DeleteDownloadRequest receiver closed");
                    }
                }
                request = delete_all_receiver.recv() => {
                    if request.is_some() {
                        debug!("Received DeleteAllDownloadsRequest");
                        match self.delete_all_downloads().await {
                            Ok((removed, skipped)) => {
                                DeleteAllDownloadsResponse { removed, skipped, error: None }.send_signal_to_dart();
                                if removed > 0 { DownloadsChanged {}.send_signal_to_dart(); }
                            }
                            Err(e) => {
                                error!(error = %format!("{e:#}"), "Failed to delete all downloads");
                                DeleteAllDownloadsResponse { removed: 0, skipped: 0, error: Some(format!("{e:#}")) }.send_signal_to_dart();
                            }
                        }
                    } else {
                        panic!("DeleteAllDownloadsRequest receiver closed");
                    }
                }
            }
        }
    }

    #[instrument(skip(self), err)]
    async fn list_downloads(&self) -> Result<Vec<DownloadEntry>> {
        let root = self.root.read().await.clone();
        let mut entries: Vec<DownloadEntry> = Vec::new();
        let mut rd = fs::read_dir(&root)
            .await
            .with_context(|| format!("Failed to read {}", root.display()))?;
        while let Some(entry) = rd.next_entry().await? {
            let p = entry.path();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_dir() {
                continue;
            }
            if let Some(e) = self.try_build_download_entry(&p).await? {
                entries.push(e);
            }
        }
        Ok(entries)
    }

    #[instrument(skip(self), fields(dir = %dir.display()), err)]
    async fn try_build_download_entry(&self, dir: &Path) -> Result<Option<DownloadEntry>> {
        if !dir.is_dir() {
            return Ok(None);
        }

        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if name.is_empty() {
            return Ok(None);
        }

        let meta = read_download_metadata(dir).await?;
        let package_name = meta.package_name;
        let version_code = meta.version_code;
        let mut ts_millis = meta.downloaded_at.unwrap_or(0);

        if ts_millis == 0
            && let Ok(meta) = fs::metadata(dir).await
            && let Ok(modified) = meta.modified()
        {
            ts_millis = system_time_to_millis(modified);
        }

        let total_size = dir_size(dir).await.unwrap_or(0);

        trace!(name = %name, ts_millis, total_size, pkg = ?package_name, ver = ?version_code, "Built download entry");
        Ok(Some(DownloadEntry {
            path: dir.to_string_lossy().to_string(),
            name,
            timestamp: ts_millis,
            total_size,
            package_name,
            version_code,
        }))
    }
}

fn system_time_to_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn rfc3339_to_millis(s: &str) -> u64 {
    // Parse RFC3339 in UTC
    match OffsetDateTime::parse(s, &Rfc3339) {
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

struct DownloadMetadata {
    downloaded_at: Option<u64>,
    package_name: Option<String>,
    version_code: Option<u32>,
}

async fn read_download_metadata(dir: &Path) -> Result<DownloadMetadata> {
    #[derive(serde::Deserialize)]
    struct DownloadMetaPartial {
        downloaded_at: Option<String>,
        #[serde(alias = "PackageName")]
        package_name: Option<String>,
        #[serde(alias = "VersionCode")]
        version_code: Option<u32>,
    }

    let meta_path = dir.join("metadata.json");
    let meta_path_alt = dir.join("release.json");
    let mut package_name: Option<String> = None;
    let mut version_code: Option<u32> = None;
    let mut ts_millis: Option<u64> = None;
    if meta_path.exists()
        && let Ok(text) = fs::read_to_string(&meta_path).await
        && let Ok(meta) = serde_json::from_str::<DownloadMetaPartial>(&text)
    {
        package_name = meta.package_name;
        version_code = meta.version_code;
        if let Some(dt) = meta.downloaded_at {
            ts_millis = Some(rfc3339_to_millis(&dt));
        }
    } else if meta_path_alt.exists()
        && let Ok(text) = fs::read_to_string(&meta_path_alt).await
        && let Ok(meta) = serde_json::from_str::<DownloadMetaPartial>(&text)
    {
        package_name = meta.package_name;
        version_code = meta.version_code;
        if let Some(dt) = meta.downloaded_at {
            ts_millis = Some(rfc3339_to_millis(&dt));

            // Only our metadata has downloaded_at, rename to metadata.json to avoid conflicts with QL
            let new_path = dir.join("metadata.json");
            if let Err(e) = fs::rename(&meta_path_alt, &new_path).await {
                warn!(error = %e, "Failed to rename our release.json to metadata.json");
            }
        }
    }

    Ok(DownloadMetadata { downloaded_at: ts_millis, package_name, version_code })
}

impl DownloadsCatalog {
    /// Applies the cleanup policy after an app installation.
    ///
    /// Downloads are grouped by their directory name using the `{name} v{version}+{build}`
    /// convention (regex: `(?m)^(.+) v\d+\+.+$`). Entries that do not match this pattern are
    /// left untouched and a warning is logged.
    #[instrument(skip(self), fields(policy = ?policy, installed = %installed_full_name), err)]
    pub async fn apply_cleanup_policy(
        &self,
        policy: DownloadCleanupPolicy,
        installed_full_name: &str,
        installed_path: &str,
    ) -> Result<()> {
        use DownloadCleanupPolicy as Policy;

        match policy {
            Policy::KeepAllVersions => {
                info!("Cleanup policy: keep all versions, nothing to do");
                return Ok(());
            }
            Policy::DeleteAfterInstall => {
                info!("Cleanup policy: delete after install, removing downloaded directory");
                let path = Path::new(installed_path);
                if !path.exists() {
                    debug!(missing = %path.display(), "Downloaded directory no longer exists");
                    return Ok(());
                }

                if let Err(err) = self.delete_download(path).await {
                    return Err(err.context("Failed to remove downloaded directory after install"));
                }

                info!(removed = %path.display(), "Removed downloaded directory after install");
                return Ok(());
            }
            Policy::KeepOneVersion | Policy::KeepTwoVersions => {
                let keep_total = match policy {
                    Policy::KeepOneVersion => 1,
                    Policy::KeepTwoVersions => 2,
                    _ => unreachable!(),
                };

                let pattern = regex!(r"^(.+) v\d+\+.+$");
                let Some(captures) = pattern.captures(installed_full_name) else {
                    warn!(
                        installed = installed_full_name,
                        "Installed release name does not follow `{{name}} vX+Y` convention, \
                         skipping cleanup"
                    );
                    return Ok(());
                };
                let base_name = captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");

                if base_name.is_empty() {
                    warn!(
                        installed = installed_full_name,
                        "Unable to determine base name for cleanup, skipping"
                    );
                    return Ok(());
                }

                let entries = self.list_downloads().await?;
                let mut matching = Vec::new();
                for entry in entries {
                    if let Some(caps) = pattern.captures(&entry.name) {
                        let entry_base = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                        if entry_base == base_name {
                            matching.push(entry);
                        }
                    } else {
                        debug!(name = %entry.name, "Ignoring download with non-standard name during cleanup");
                    }
                }

                if matching.is_empty() {
                    debug!(%base_name, "No matching downloads found for cleanup");
                    return Ok(());
                }

                matching.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                let mut keep: Vec<String> = Vec::with_capacity(keep_total as usize);
                keep.push(installed_full_name.to_string());
                for entry in &matching {
                    if keep.len() >= keep_total as usize {
                        break;
                    }
                    if entry.name != installed_full_name {
                        keep.push(entry.name.clone());
                    }
                }
                let keep_set: HashSet<String> = keep.into_iter().collect();

                info!(
                    base = %base_name,
                    keep_count = keep_set.len(),
                    desired_keep = keep_total,
                    kept = ?keep_set,
                    "Applying versioned downloads cleanup"
                );

                for entry in matching {
                    if keep_set.contains(&entry.name) {
                        continue;
                    }

                    let path = Path::new(&entry.path);
                    if !path.exists() {
                        debug!(missing = %path.display(), "Skipping cleanup for missing download directory");
                        continue;
                    }

                    if let Err(err) = self.delete_download(path).await {
                        warn!(error = err.as_ref() as &dyn Error, path = %path.display(), "Failed to remove older downloaded version");
                    } else {
                        info!(removed = %path.display(), "Removed older downloaded version");
                    }
                }

                Ok(())
            }
        }
    }

    #[instrument(skip(self), err)]
    async fn delete_download(&self, path: &Path) -> Result<()> {
        let root = self.root.read().await.clone();
        let canon_root = fs::canonicalize(root).await?;
        let canon_req = fs::canonicalize(path).await?;
        ensure!(
            canon_req.starts_with(&canon_root),
            "Requested path is outside downloads directory"
        );
        ensure!(canon_req.is_dir(), "Download path is not a directory");
        info!(path = %canon_req.display(), "Deleting download directory");
        fs::remove_dir_all(&canon_req).await.context("Failed to delete download directory")?;
        Ok(())
    }

    #[instrument(skip(self), err, ret)]
    async fn delete_all_downloads(&self) -> Result<(u32, u32)> {
        let root = self.root.read().await.clone();
        let mut removed: u32 = 0;
        let mut skipped: u32 = 0;
        let mut rd = fs::read_dir(&root).await?;
        while let Some(entry) = rd.next_entry().await? {
            let dir = entry.path();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_dir() {
                continue;
            }
            // Only delete directories that contain metadata.json or release.json
            let meta_path = dir.join("metadata.json");
            let meta_path_alt = dir.join("release.json");
            if !meta_path.exists() && !meta_path_alt.exists() {
                warn!(path = %dir.display(), "No deleting download: no metadata file found");
                continue;
            }
            if dir.exists() {
                match fs::remove_dir_all(&dir).await {
                    Ok(()) => {
                        removed += 1;
                    }
                    Err(_) => {
                        skipped += 1;
                    }
                }
            }
        }
        Ok((removed, skipped))
    }
}
