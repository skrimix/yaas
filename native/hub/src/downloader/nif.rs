use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use futures::{StreamExt, TryStreamExt};
use opendal::{
    EntryMode, FuturesBytesStream, Operator,
    layers::{LoggingLayer, TimeoutLayer},
    raw::build_rel_path,
    services::Webdav,
};
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{debug, info, trace, warn};

use crate::{models::CloudApp, utils::AverageSpeed};

/// Storage implementation for transfers with NIF cloud storage.
#[derive(Debug, Clone)]
pub struct NifStorage {
    operator: Operator,
}

/// File entry on a remote WebDAV server.
#[derive(Debug, Clone)]
struct RemoteFile {
    path: String,
    size: u64,
    last_modified: DateTime<Utc>,
}

/// Result of a directory comparison.
#[derive(Debug, Clone)]
struct CompareResult {
    total_bytes: u64,
    total_files: u64,
    to_delete: Vec<String>,
    to_download: Vec<RemoteFile>,
}

/// Progress of a directory download.
#[derive(Debug, Clone)]
pub struct DirDownloadProgress {
    /// Number of bytes that are to be downloaded.
    pub total_bytes: u64,
    /// Number of files that are to be downloaded.
    pub total_files: u64,
    /// Number of bytes that have been downloaded.
    pub downloaded_bytes: u64,
    /// Number of files that have been downloaded.
    pub downloaded_files: u64,
    /// Current total transfer speed in bytes per second.
    pub speed: u64,
}

impl NifStorage {
    // #[instrument(level = "debug")]
    pub async fn new() -> Result<Self> {
        // TODO: make configurable
        let creds = tokio::fs::read_to_string("/home/skrimix/work/webdav-creds.txt")
            .await
            .context("failed to read webdav credentials")?;
        let creds = creds.trim().split(":").collect::<Vec<&str>>();
        let builder = Webdav::default()
            .endpoint("https://drive.5698452.xyz:33456")
            .root("Quest Games/")
            .username(creds[0])
            .password(creds[1]);

        let operator = Operator::new(builder)
            .expect("failed to create operator") // TODO: handle error
            .layer(LoggingLayer::default()) // TODO: add ThrottleLayer?
            .layer(TimeoutLayer::default())
            .finish();
        operator.check().await?;

        Ok(Self { operator })
    }

    /// Creates a reader from the remote file and a buffered writer for the local file.
    // #[instrument(err, level = "debug")]
    async fn prepare_file_download(
        &self,
        source_path: &str,
        destination: &Path,
        concurrency: usize,
    ) -> Result<(PathBuf, BufWriter<tokio::fs::File>, FuturesBytesStream)> {
        ensure!(destination.is_dir(), "destination path does not exist or is not a directory");

        let file_name = Path::new(source_path)
            .file_name()
            .context("failed to get source file name")?
            .to_str()
            .context("directory entry is not a valid utf-8 string")?;

        trace!("creating reader");
        let stream = self
            .operator
            .reader_with(source_path)
            .concurrent(concurrency)
            .chunk(8 * 1024 * 1024) // 8 MiB
            .await
            .context("failed to create reader")?
            .into_bytes_stream(..)
            .await
            .context("failed to create byte stream");
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                rinf::debug_print!("error: {:?}", e);
                return Err(e);
            }
        };

        let target_path = destination.join(file_name);
        trace!(path = ?target_path, "creating local file");
        // TODO: lock file for writing
        let target_file = tokio::fs::File::create(&target_path).await?;
        let writer = BufWriter::new(target_file);

        Ok((target_path, writer, stream))
    }

    /// Downloads a single file from remote server.
    // #[instrument(err, ret, level = "debug")]
    pub async fn download_file(
        &self,
        remote_path: &str,
        destination: PathBuf,
        concurrency: usize,
        bytes_transferred: Arc<AtomicU64>, // TODO: make optional?
    ) -> Result<String> {
        // FIXME: disallow concurrent for same remote path
        let (target_path, mut writer, mut stream) =
            self.prepare_file_download(remote_path, &destination, concurrency).await?;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("error reading chunk")?;

            // writer.write_all(&chunk).await.context("error writing chunk to file")?;
            // bytes_transferred.fetch_add(chunk.len() as u64, Ordering::Relaxed);

            // split chunk into smaller chunks to avoid delays between progress updates
            // (does webdav always return chunks of 16kb?)
            // TODO: detect stalled stream
            for sub_chunk in chunk.chunks(1024 * 16) {
                writer.write_all(sub_chunk).await.context("error writing chunk to file")?;
                bytes_transferred.fetch_add(sub_chunk.len() as u64, Ordering::Relaxed);
            }
        }

        writer.flush().await.context("error flushing file writer")?;

        let file_path_str =
            target_path.to_str().context("target file path is not valid utf-8")?.to_string();
        Ok(file_path_str)
    }

    /// Compares the contents of a remote directory with the local directory and returns a `CompareResult`
    /// containing summary information and a list of files to download and delete.
    // #[instrument(err, ret, level = "debug")]
    async fn compare_dirs(
        &self,
        remote_dir: &str,
        destination_dir: &Path,
    ) -> Result<CompareResult> {
        let mut total_bytes: u64 = 0;
        let mut total_files: u64 = 0;
        let mut to_check = Vec::new();
        let mut to_download: Vec<RemoteFile> = Vec::new();
        let mut to_delete = Vec::new();

        ensure!(
            self.operator
                .exists(remote_dir)
                .await
                .context("failed to check source directory exists")?,
            "source directory not found"
        );

        let remote_entries = self.list_remote_entries(remote_dir).await?;
        for entry in remote_entries {
            if entry.metadata().mode() == EntryMode::FILE {
                // collect size and last modified date of all files in the remote directory
                let remote_path = entry.path().to_string();
                let content_length = entry.metadata().content_length();
                let last_modified = entry
                    .metadata()
                    .last_modified()
                    .context("failed to get last modified date for remote file")?;
                to_check.push(RemoteFile {
                    path: remote_path,
                    size: content_length,
                    last_modified,
                });
                total_bytes += content_length;
                total_files += 1;
            }
        }

        let local_files = list_local_files(destination_dir)?;
        for local_file in local_files {
            if local_file.is_dir() {
                continue;
            }
            let rel_path = build_rel_path(
                destination_dir
                    .parent()
                    .context("failed to get parent of destination directory")?
                    .to_str()
                    .context("destination path is not valid utf-8")?,
                local_file.to_str().context("local file path is not valid utf-8")?,
            )
            .trim_start_matches("/")
            .to_string();

            // look for a matching file in the remote directory
            match find_remote_file(&to_check, &rel_path) {
                Some((idx, remote_file)) => {
                    if needs_update(&local_file, remote_file)? {
                        to_download.push(remote_file.clone());
                    }
                    to_check.remove(idx);
                }
                None => {
                    to_delete.push(
                        local_file
                            .to_str()
                            .context("to_delete path is not valid utf-8")?
                            .to_string(),
                    );
                }
            }
        }

        // all remaining remote files are to be downloaded
        for file in to_check {
            to_download.push(file.clone());
        }

        Ok(CompareResult { total_bytes, total_files, to_delete, to_download })
    }

    // #[instrument(err, ret, level = "debug")]
    pub async fn download_dir(
        &self,
        remote_dir: String,
        destination_dir: PathBuf,
        transfers: usize,
        progress_channel: tokio::sync::mpsc::UnboundedSender<DirDownloadProgress>,
    ) -> Result<String> {
        let source_dir = if remote_dir.ends_with('/') {
            remote_dir.to_string()
        } else {
            format!("{remote_dir}/")
        };

        if !destination_dir.is_dir() {
            std::fs::create_dir(&destination_dir)
                .context("failed to create destination directory")?;
        }

        let CompareResult { total_bytes, total_files, to_download, to_delete } =
            self.compare_dirs(&source_dir, &destination_dir).await?;

        debug!(
            total_bytes = ?total_bytes,
            total_files = ?total_files,
            to_download = ?to_download.len(),
            to_delete = ?to_delete.len(),
            "downloading files"
        );

        let bytes_transferred = Arc::new(AtomicU64::new(0));
        let files_downloaded = Arc::new(AtomicU64::new(total_files - to_download.len() as u64));
        let (progress_stop_tx, mut progress_stop_rx) = tokio::sync::oneshot::channel::<()>();

        // report progress every second
        tokio::spawn({
            let bytes_transferred = bytes_transferred.clone();
            let files_downloaded = files_downloaded.clone();
            let progress_channel = progress_channel.clone();
            async move {
                debug!("starting download progress report task");
                let mut average_speed = AverageSpeed::new(Duration::from_millis(5500));
                loop {
                    if progress_stop_rx.try_recv().is_ok() {
                        debug!("received stop signal in progress download report task");
                        break;
                    }

                    let new_bytes_value = bytes_transferred.load(Ordering::Relaxed);
                    let files_downloaded_value = files_downloaded.load(Ordering::Relaxed);
                    average_speed.add_from_total(new_bytes_value);
                    let speed = average_speed.average();
                    debug!(downloaded_bytes = new_bytes_value, speed, "download progress");

                    if let Err(e) = progress_channel.send(DirDownloadProgress {
                        downloaded_files: files_downloaded_value,
                        total_files,
                        downloaded_bytes: new_bytes_value,
                        speed,
                        total_bytes,
                    }) {
                        warn!(
                            error = &e as &dyn std::error::Error,
                            "failed to send file download progress update"
                        );
                        break;
                    }

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        // run download tasks in parallel
        tokio_stream::iter(to_download)
            .map(|entry| async {
                let path = entry.path;
                let local_path =
                    destination_dir.join(build_rel_path(&format!("/{source_dir}"), &path));
                let download_destination = local_path
                    .parent()
                    .context("failed to get download destination")?
                    .to_path_buf();
                std::fs::create_dir_all(&download_destination)
                    .context("failed to create parent directory for local file")?;

                self.download_file(&path, download_destination, 4, bytes_transferred.clone())
                    .await?; // TODO: configure?
                files_downloaded.fetch_add(1, Ordering::Relaxed);

                Ok::<_, anyhow::Error>(())
            })
            .buffer_unordered(transfers)
            .try_collect::<()>()
            .await
            .context("failed to download files")?;

        progress_stop_tx.send(()).expect("stop channel closed unexpectedly");

        // delete local files that are not in the remote directory
        for path in to_delete {
            std::fs::remove_file(destination_dir.join(&path))
                .context("failed to delete old local file")?;
        }

        Ok(destination_dir.to_str().context("destination path is not valid utf-8")?.to_string())
    }

    /// Lists the contents of a remote directory, including size and last modified date.
    // #[instrument(err, ret, level = "trace")]
    async fn list_remote_entries(&self, source_dir: &str) -> Result<Vec<opendal::Entry>> {
        self.operator
            .list_with(source_dir)
            .recursive(true)
            .await
            .context("failed to list directory")
    }

    pub async fn get_app_list(&self) -> Result<Vec<CloudApp>> {
        // TODO: change hardcoded paths
        let path = self
            .download_file(
                "FFA.txt",
                PathBuf::from("/home/skrimix/work/test"),
                4,
                Arc::new(AtomicU64::new(0)),
            )
            .await?;
        let file = File::open(path).await?;
        let mut reader =
            csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
        let records = reader.deserialize();
        let cloud_apps: Vec<CloudApp> = records.map_ok(|r| r).try_collect().await?;
        info!("Loaded {} cloud apps", cloud_apps.len());
        Ok(cloud_apps)
    }
}

/// Finds a file in a list of remote files by its relative path.
// #[instrument(ret, level = "trace")]
fn find_remote_file<'a>(entries: &'a [RemoteFile], path: &str) -> Option<(usize, &'a RemoteFile)> {
    entries.iter().enumerate().find(|(_, e)| e.path == path)
}

/// Lists the contents of a local directory.
// #[instrument(err, ret, level = "trace")]
fn list_local_files(destination_dir: &Path) -> Result<Vec<PathBuf>> {
    trace!(path = ?destination_dir, "listing local directory");
    glob::glob(destination_dir.join("**/*").to_str().context("failed to create glob pattern")?)
        .context("failed to glob directory")?
        .collect::<Result<Vec<_>, _>>()
        .context("failed to get local file metadata")
}

/// Compares the size and last modified date of a local file with the remote file.
// #[instrument(err, ret, level = "trace")]
fn needs_update(local_file: &Path, remote_file: &RemoteFile) -> Result<bool> {
    let meta = std::fs::metadata(local_file).context("failed to get metadata for local file")?;
    let size = meta.len();
    let last_modified: DateTime<Utc> = meta.modified()?.into();
    Ok(size != remote_file.size || last_modified < remote_file.last_modified)
}

#[cfg(test)]
mod tests {
    // use test_log::test;

    // use super::*;

    // TODO: test with https://www.dlp-test.com/webdav_pub/ or something like that
}
