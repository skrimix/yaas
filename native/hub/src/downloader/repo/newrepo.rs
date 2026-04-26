use std::{
    collections::HashMap,
    error::Error,
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use derive_more::Debug;
use futures::StreamExt as _;
use tempfile::TempDir;
use tokio::{
    fs,
    io::{AsyncSeekExt, AsyncWriteExt, DuplexStream},
    sync::{Mutex, mpsc::UnboundedSender},
    time as tokio_time,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};
use yarc::{
    app_list::{AppList, AppRelease},
    container::YarcReader,
    manifest::ReleaseManifest,
};

use super::{
    BuildStorageArgs, BuildStorageResult, Repo, RepoAppList, RepoCapabilities, RepoDownloadResult,
    RepoStorage,
};
use crate::{
    downloader::{AppDownloadProgress, TransferStats, config::DownloaderConfig, http_cache},
    models::{CloudApp, DownloadMode},
};

const YAAS_KEY_HEADER: &str = "x-yaas-key";
const STREAM_PROGRESS_INTERVAL_MILLIS: u128 = 500;
const SLOW_NETWORK_WARNING_THRESHOLD: Duration = Duration::from_secs(8);
const LOCAL_DOWNLOAD_METADATA_PATHS: [&str; 2] = ["metadata.json", "release.json"];
const STAGED_DOWNLOAD_WORKERS: usize = 4;
const STAGED_MIN_PART_SIZE: u64 = 32 * 1024 * 1024;

#[derive(Debug, Default)]
struct NewRepoRuntime {
    yarc_key: Option<[u8; 32]>,
    releases_by_full_name: HashMap<String, AppRelease>,
}

#[derive(Debug, Clone)]
pub(in crate::downloader) struct NewRepoStorage {
    base_url: String,
    runtime: Arc<Mutex<NewRepoRuntime>>,
}

impl NewRepoStorage {
    fn new(base_url: String) -> Self {
        Self { base_url, runtime: Arc::new(Mutex::new(NewRepoRuntime::default())) }
    }

    fn list_url(&self) -> String {
        format!("{}/list", self.base_url)
    }

    fn manifest_url(&self, manifest_hash: &str) -> String {
        format!("{}/manifest/{manifest_hash}", self.base_url)
    }

    fn blob_url(&self, blob_hash: &str) -> String {
        format!("{}/blob/{blob_hash}", self.base_url)
    }

    async fn update_index(&self, releases: &[AppRelease], yarc_key: [u8; 32]) {
        let mut runtime = self.runtime.lock().await;
        runtime.yarc_key = Some(yarc_key);
        runtime.releases_by_full_name = releases
            .iter()
            .map(|release| (release.release_name.clone(), release.clone()))
            .collect();
    }

    async fn current_key(&self) -> Option<[u8; 32]> {
        self.runtime.lock().await.yarc_key
    }

    async fn set_key(&self, yarc_key: [u8; 32]) {
        self.runtime.lock().await.yarc_key = Some(yarc_key);
    }

    async fn release_for_download(&self, app_full_name: &str) -> Option<AppRelease> {
        self.runtime.lock().await.releases_by_full_name.get(app_full_name).cloned()
    }
}

impl PartialEq for NewRepoStorage {
    fn eq(&self, other: &Self) -> bool {
        self.base_url == other.base_url
    }
}

impl Eq for NewRepoStorage {}

#[derive(Debug, Clone)]
pub(super) struct NewRepo {
    base_url: String,
}

impl NewRepo {
    pub(super) fn from_config(cfg: &DownloaderConfig) -> Self {
        let base_url = cfg
            .base_url
            .as_deref()
            .expect("validated new-repo config must have base_url")
            .trim_end_matches('/')
            .to_string();
        Self { base_url }
    }
}

#[async_trait]
impl Repo for NewRepo {
    fn id(&self) -> &'static str {
        "new-repo"
    }

    fn capabilities(&self) -> RepoCapabilities {
        RepoCapabilities {
            supports_remote_selection: false,
            supports_bandwidth_limit: false,
            supports_download_mode_selection: true,
            supports_donation_upload: false,
        }
    }

    async fn build_storage(&self, _args: BuildStorageArgs<'_>) -> Result<BuildStorageResult> {
        Ok(BuildStorageResult {
            storage: RepoStorage::NewRepo(NewRepoStorage::new(self.base_url.clone())),
            persist_remote: None,
        })
    }

    async fn list_remotes(&self, _storage: RepoStorage) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    #[instrument(
        level = "debug",
        name = "repo.load_app_list",
        skip(storage, http_client, cancellation_token),
        fields(layout = %self.id())
    )]
    async fn load_app_list(
        &self,
        storage: RepoStorage,
        _list_path: String,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<RepoAppList> {
        let RepoStorage::NewRepo(storage) = storage else {
            unreachable!("ffa storage passed to new-repo backend");
        };

        ensure_not_cancelled(&cancellation_token)?;
        debug!(url = %storage.list_url(), "Fetching app list decryption key");
        let yarc_key =
            match fetch_yarc_key(http_client, &storage.list_url(), &cancellation_token).await {
                Ok(key) => key,
                Err(error) => {
                    if let Some(existing) = storage.current_key().await {
                        warn!(
                            error = error.as_ref() as &dyn Error,
                            "Failed to refresh decryption key, reusing cached key"
                        );
                        existing
                    } else {
                        return Err(error);
                    }
                }
            };
        storage.set_key(yarc_key).await;

        let list_path = cache_remote_file(
            http_client,
            &storage.list_url(),
            cache_dir,
            "newrepo/list.yarc",
            &cancellation_token,
        )
        .await
        .context("Failed to cache app list")?;
        ensure_not_cancelled(&cancellation_token)?;
        debug!(path = %list_path.display(), "Reading cached app list");

        let list_bytes = fs::read(&list_path)
            .await
            .with_context(|| format!("Failed to read {}", list_path.display()))?;
        let (app_list, _) = AppList::from_yarc(list_bytes.as_slice(), yarc_key)
            .await
            .context("Failed to decode app list")?;

        let mut apps = Vec::with_capacity(app_list.releases.len());
        for release in &app_list.releases {
            match cloud_app_from_release(release) {
                Ok(app) => apps.push(app),
                Err(error) => {
                    warn!(
                        release_name = release.release_name,
                        error = error.as_ref() as &dyn Error,
                        "Skipping malformed release"
                    );
                }
            }
        }

        storage.update_index(&app_list.releases, yarc_key).await;
        info!(app_count = apps.len(), "Loaded app list");
        Ok(RepoAppList { apps, donation_blacklist: Vec::new() })
    }

    #[instrument(
        level = "debug",
        name = "repo.download_app",
        skip(storage, http_client, progress_tx, cancellation_token),
        fields(layout = %self.id(), app_full_name = app_full_name)
    )]
    async fn download_app(
        &self,
        storage: RepoStorage,
        app_full_name: &str,
        destination_dir: &Path,
        _cache_dir: &Path,
        http_client: &reqwest::Client,
        download_mode: DownloadMode,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<RepoDownloadResult> {
        let RepoStorage::NewRepo(storage) = storage else {
            unreachable!("ffa storage passed to new-repo backend");
        };

        ensure_not_cancelled(&cancellation_token)?;
        info!(
            app_full_name,
            destination = %destination_dir.display(),
            "Starting app download"
        );
        let release = storage.release_for_download(app_full_name).await.ok_or_else(|| {
            anyhow!(
                "No release metadata found for `{app_full_name}`. Refresh the cloud app list and \
                 try again."
            )
        })?;
        debug!(
            release_name = %release.release_name,
            manifest_hash = %release.manifest_hash,
            "Resolved release metadata"
        );
        let yarc_key = match storage.current_key().await {
            Some(key) => key,
            None => {
                send_status(&progress_tx, "Fetching decryption key...");
                debug!(url = %storage.list_url(), "Refreshing missing decryption key");
                let key =
                    fetch_yarc_key(http_client, &storage.list_url(), &cancellation_token).await?;
                storage.set_key(key).await;
                key
            }
        };

        send_status(&progress_tx, "Fetching manifest...");
        debug!(
            manifest_hash = %release.manifest_hash,
            "Fetching manifest"
        );
        let manifest_url = storage.manifest_url(&release.manifest_hash);
        let manifest_bytes = send_with_cancellation(
            http_client.get(&manifest_url),
            &manifest_url,
            &cancellation_token,
        )
        .await
        .context("Failed to fetch manifest")?
        .error_for_status()
        .context("Manifest request failed")?
        .bytes()
        .await
        .context("Failed to read manifest response")?;
        let (manifest, _) = ReleaseManifest::from_yarc(manifest_bytes.as_ref(), yarc_key)
            .await
            .context("Failed to decode manifest")?;

        ensure!(
            manifest.release_key == release.release_name,
            "Manifest release key mismatch for `{}`: expected `{}`, got `{}`",
            app_full_name,
            release.release_name,
            manifest.release_key
        );
        debug!(
            blob_id = %manifest.yarc_id,
            entry_count = manifest.entries.len(),
            yarc_size = manifest.yarc_size,
            plaintext_size = manifest.plaintext_size,
            "Decoded manifest"
        );

        if destination_dir.exists() {
            send_status(&progress_tx, "Checking existing files...");
            debug!(
                path = %destination_dir.display(),
                "Checking existing download against manifest"
            );
            match manifest
                .verify_directory_ignoring_paths(destination_dir, &LOCAL_DOWNLOAD_METADATA_PATHS)
                .await
            {
                Ok(true) => {
                    info!(
                        path = %destination_dir.display(),
                        "Skipping download because local files already match the latest manifest"
                    );
                    return Ok(RepoDownloadResult { skipped: true });
                }
                Ok(false) => {
                    debug!(
                        path = %destination_dir.display(),
                        "Existing download does not match the latest manifest, continuing with download"
                    );
                }
                Err(error) => {
                    warn!(
                        path = %destination_dir.display(),
                        error = &error as &dyn Error,
                        "Failed to verify existing download, continuing with download"
                    );
                }
            }
        }

        let destination_parent = destination_dir
            .parent()
            .ok_or_else(|| anyhow!("Download destination must have a parent directory"))?;
        fs::create_dir_all(destination_parent)
            .await
            .with_context(|| format!("Failed to create {}", destination_parent.display()))?;

        let temp_dir = create_temp_dir(destination_parent, app_full_name)?;
        let temp_dir_path = temp_dir.path();
        debug!(path = %temp_dir_path.display(), "Created temporary extraction directory");
        let download_result = async {
            send_status(&progress_tx, "Starting package download...");
            let blob_url = storage.blob_url(&manifest.yarc_id);
            match download_mode {
                DownloadMode::Staged => {
                    let package_path = temp_dir_path.join("package.yarc");
                    debug!(
                        blob_id = %manifest.yarc_id,
                        total_bytes = manifest.yarc_size,
                        path = %package_path.display(),
                        "Starting staged package download"
                    );
                    send_status(&progress_tx, "Downloading package...");
                    download_package_staged(
                        http_client,
                        &blob_url,
                        &package_path,
                        manifest.yarc_size,
                        progress_tx.clone(),
                        cancellation_token.clone(),
                    )
                    .await
                    .context("Failed to download package")?;

                    ensure_not_cancelled(&cancellation_token)?;
                    send_status(&progress_tx, "Extracting package...");
                    debug!(path = %package_path.display(), "Starting staged package extraction");
                    let package_file = fs::File::open(&package_path)
                        .await
                        .with_context(|| format!("Failed to open {}", package_path.display()))?;
                    YarcReader::new(yarc_key)
                        .extract_to_directory(package_file, &temp_dir_path)
                        .await
                        .context("Failed to extract YARC package")?;
                    fs::remove_file(&package_path)
                        .await
                        .with_context(|| format!("Failed to remove {}", package_path.display()))?;
                }
                DownloadMode::Streamed => {
                    download_package_streamed(
                        http_client,
                        &blob_url,
                        temp_dir_path,
                        yarc_key,
                        manifest.yarc_size,
                        progress_tx.clone(),
                        cancellation_token.clone(),
                    )
                    .await
                    .context("Failed to download package")?;
                }
            }

            ensure_not_cancelled(&cancellation_token)?;
            debug!("Restoring extracted file metadata from manifest");
            manifest
                .apply_metadata_to_directory(&temp_dir_path)
                .await
                .context("Failed to restore extracted YARC metadata")?;
            send_status(&progress_tx, "Verifying files...");
            debug!("Verifying extracted directory against manifest");
            ensure!(
                manifest
                    .verify_directory(&temp_dir_path)
                    .await
                    .context("Failed to verify extracted YARC package")?,
                "Downloaded package contents did not match the manifest"
            );

            send_status(&progress_tx, "Finalizing download...");
            debug!(
                from = %temp_dir_path.display(),
                to = %destination_dir.display(),
                "Replacing destination directory with verified extraction"
            );
            replace_directory(temp_dir_path, destination_dir).await
        }
        .await;

        if temp_dir_path.exists() {
            debug!(path = %temp_dir_path.display(), "Cleaning up temporary directory");
            if let Err(error) = cleanup_temp_dir(temp_dir_path).await {
                warn!(
                    path = %temp_dir_path.display(),
                    error = error.as_ref() as &dyn Error,
                    "Failed to clean up temporary directory"
                );
            } else {
                debug!(path = %temp_dir_path.display(), "Finished temporary cleanup");
            }
        }

        match &download_result {
            Ok(()) => info!(app_full_name, "Completed download"),
            Err(error) if cancellation_token.is_cancelled() => {
                info!(app_full_name, error = error.as_ref() as &dyn Error, "Download cancelled");
            }
            Err(error) => {
                warn!(app_full_name, error = error.as_ref() as &dyn Error, "Download failed");
            }
        }

        download_result.map(|()| RepoDownloadResult { skipped: false })
    }

    async fn upload_donation_archive(
        &self,
        _storage: RepoStorage,
        _config: &DownloaderConfig,
        _archive_path: &Path,
        _stats_tx: Option<UnboundedSender<TransferStats>>,
        _cancellation_token: CancellationToken,
    ) -> Result<()> {
        bail!("App donations are not supported for the new-repo repository layout")
    }
}

fn cloud_app_from_release(release: &AppRelease) -> Result<CloudApp> {
    let version_code = release
        .version_code
        .trim()
        .parse::<u32>()
        .with_context(|| format!("Invalid version code: {}", release.version_code))?;
    let size = parse_size_mb_to_bytes(&release.megabytes)?;
    let last_updated = format_last_updated(release.last_modified_time)?;
    Ok(CloudApp::new(
        release.app_name.clone(),
        release.release_name.clone(),
        release.package_name.clone(),
        version_code,
        last_updated,
        size,
    ))
}

fn parse_size_mb_to_bytes(size_mb_str: &str) -> Result<u64> {
    let size_mb =
        size_mb_str.parse::<f64>().with_context(|| format!("Invalid size (MB): {size_mb_str}"))?;
    Ok((size_mb * 1000.0 * 1000.0) as u64)
}

fn format_last_updated(last_modified_time: u64) -> Result<String> {
    let timestamp = time::OffsetDateTime::from_unix_timestamp(last_modified_time as i64)
        .with_context(|| format!("Invalid release timestamp: {last_modified_time}"))?;
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        timestamp.year(),
        u8::from(timestamp.month()),
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute()
    ))
}

async fn fetch_yarc_key(
    client: &reqwest::Client,
    url: &str,
    cancellation_token: &CancellationToken,
) -> Result<[u8; 32]> {
    ensure_not_cancelled(cancellation_token)?;
    let response = send_with_cancellation(client.head(url), url, cancellation_token)
        .await
        .with_context(|| format!("Failed to fetch YARC key from {url}"))?;
    let response = response
        .error_for_status()
        .with_context(|| format!("Failed to fetch YARC key from {url}"))?;
    let key_hex = response
        .headers()
        .get(YAAS_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow!("Worker response is missing `{YAAS_KEY_HEADER}` header"))?;
    parse_yarc_key_hex(key_hex)
}

fn parse_yarc_key_hex(value: &str) -> Result<[u8; 32]> {
    let normalized = value.trim();
    ensure!(normalized.len() == 64, "Invalid YARC key length: expected 64 hex characters");
    let key = const_hex::decode_to_array(normalized).context("Invalid YARC key")?;
    Ok(key)
}

async fn cache_remote_file(
    client: &reqwest::Client,
    url: &str,
    cache_dir: &Path,
    relative_path: &str,
    cancellation_token: &CancellationToken,
) -> Result<PathBuf> {
    let destination = cache_dir.join(relative_path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    debug!(
        url,
        destination = %destination.display(),
        "Updating cached NewRepo file"
    );
    tokio::select! {
        _ = cancellation_token.cancelled() => {
            info!(url, "Cancelled while waiting to update NewRepo cache");
            bail!("Operation cancelled")
        },
        result = http_cache::update_file_cached(client, url, &destination, cache_dir, None) => {
            result.with_context(|| format!("Failed to update cache for {url}"))?;
        }
    }
    Ok(destination)
}

async fn download_package_streamed(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
    yarc_key: [u8; 32],
    expected_bytes: u64,
    progress_tx: UnboundedSender<AppDownloadProgress>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    let response = send_with_cancellation(client.get(url), url, &cancellation_token)
        .await
        .context("Failed to download package")?;
    let response = response.error_for_status().context("Package request failed")?;
    debug!(
        content_length = response.content_length(),
        expected_bytes, "Received package response headers"
    );

    let total_bytes = response.content_length().unwrap_or(expected_bytes);
    let (writer, reader) = tokio::io::duplex(256 * 1024);
    send_status(&progress_tx, "Downloading package...");
    let stream_task = tokio::spawn(stream_package_to_pipe(
        response,
        writer,
        progress_tx,
        total_bytes,
        cancellation_token,
    ));

    debug!("Starting streamed package extraction");
    let extract_result = YarcReader::new(yarc_key)
        .extract_to_directory(reader, destination)
        .await
        .context("Failed to extract YARC package");
    let stream_result = join_transfer_task(stream_task).await;

    match (extract_result, stream_result) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(_), Ok(())) => Ok(()),
    }
}

async fn download_package_staged(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
    total_bytes: u64,
    progress_tx: UnboundedSender<AppDownloadProgress>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    ensure!(total_bytes > 0, "Package size must be greater than zero");
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(destination)
        .await
        .with_context(|| format!("Failed to create {}", destination.display()))?;
    file.set_len(total_bytes)
        .await
        .with_context(|| format!("Failed to preallocate {}", destination.display()))?;
    drop(file);

    let ranges = staged_download_ranges(total_bytes);
    debug!(
        url,
        destination = %destination.display(),
        total_bytes,
        range_count = ranges.len(),
        "Downloading package with ranged staged transfer"
    );

    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let transfer_token = cancellation_token.child_token();
    let progress_token = CancellationToken::new();
    let progress_task = tokio::spawn(staged_progress_loop(
        downloaded_bytes.clone(),
        total_bytes,
        progress_tx,
        progress_token.clone(),
    ));

    let mut tasks = Vec::with_capacity(ranges.len());
    for range in ranges {
        tasks.push(tokio::spawn(download_staged_range(
            client.clone(),
            url.to_string(),
            destination.to_path_buf(),
            range,
            total_bytes,
            downloaded_bytes.clone(),
            transfer_token.clone(),
        )));
    }

    let mut result = Ok(());
    for task in tasks {
        match join_transfer_task(task).await {
            Ok(()) => {}
            Err(error) if result.is_ok() => {
                result = Err(error);
                transfer_token.cancel();
                progress_token.cancel();
            }
            Err(_) => {}
        }
    }

    progress_token.cancel();
    let _ = join_transfer_task(progress_task).await;
    result?;

    let actual_len = fs::metadata(destination)
        .await
        .with_context(|| format!("Failed to read {}", destination.display()))?
        .len();
    ensure!(
        actual_len == total_bytes,
        "Downloaded package size mismatch: expected {total_bytes}, got {actual_len}"
    );
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct StagedRange {
    index: usize,
    start: u64,
    end: u64,
}

impl StagedRange {
    fn len(self) -> u64 {
        self.end - self.start + 1
    }

    fn header_value(self) -> String {
        format!("bytes={}-{}", self.start, self.end)
    }
}

fn staged_download_ranges(total_bytes: u64) -> Vec<StagedRange> {
    let max_parts = total_bytes.div_ceil(STAGED_MIN_PART_SIZE).max(1) as usize;
    let part_count = STAGED_DOWNLOAD_WORKERS.min(max_parts).max(1);
    let part_size = total_bytes.div_ceil(part_count as u64);

    (0..part_count)
        .map(|index| {
            let start = index as u64 * part_size;
            let end = (start + part_size).min(total_bytes) - 1;
            StagedRange { index, start, end }
        })
        .collect()
}

async fn download_staged_range(
    client: reqwest::Client,
    url: String,
    destination: PathBuf,
    range: StagedRange,
    total_bytes: u64,
    downloaded_bytes: Arc<AtomicU64>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    ensure_not_cancelled(&cancellation_token)?;
    let response = send_with_cancellation(
        client.get(&url).header(reqwest::header::RANGE, range.header_value()),
        &url,
        &cancellation_token,
    )
    .await
    .with_context(|| format!("Failed to download package range {}", range.index))?;

    ensure!(
        response.status() == reqwest::StatusCode::PARTIAL_CONTENT,
        "Package range {} returned HTTP {}, expected 206 Partial Content",
        range.index,
        response.status()
    );
    validate_content_range(response.headers(), range, total_bytes)
        .with_context(|| format!("Invalid Content-Range for package range {}", range.index))?;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&destination)
        .await
        .with_context(|| format!("Failed to open {}", destination.display()))?;
    file.seek(SeekFrom::Start(range.start))
        .await
        .with_context(|| format!("Failed to seek {}", destination.display()))?;

    let mut written = 0_u64;
    let mut stream = response.bytes_stream();
    loop {
        let maybe_chunk = tokio::select! {
            _ = cancellation_token.cancelled() => {
                info!(range_index = range.index, "Cancelled while downloading staged package range");
                bail!("Operation cancelled")
            },
            chunk = stream.next() => {
                chunk.transpose().context("Failed to stream staged package range")?
            }
        };
        let Some(chunk) = maybe_chunk else {
            break;
        };
        ensure!(
            written + chunk.len() as u64 <= range.len(),
            "Package range {} exceeded requested length",
            range.index
        );
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                info!(range_index = range.index, "Cancelled while writing staged package range");
                bail!("Operation cancelled")
            },
            result = file.write_all(&chunk) => {
                result.with_context(|| format!("Failed to write package range {}", range.index))?;
            }
        }
        written += chunk.len() as u64;
        downloaded_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    file.shutdown()
        .await
        .with_context(|| format!("Failed to flush package range {}", range.index))?;
    ensure!(
        written == range.len(),
        "Package range {} length mismatch: expected {}, got {}",
        range.index,
        range.len(),
        written
    );
    debug!(
        range_index = range.index,
        start = range.start,
        end = range.end,
        bytes = written,
        "Finished staged package range"
    );
    Ok(())
}

async fn staged_progress_loop(
    downloaded_bytes: Arc<AtomicU64>,
    total_bytes: u64,
    progress_tx: UnboundedSender<AppDownloadProgress>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    let started_at = Instant::now();
    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => break,
            _ = tokio_time::sleep(Duration::from_millis(STREAM_PROGRESS_INTERVAL_MILLIS as u64)) => {
                let bytes = downloaded_bytes.load(Ordering::Relaxed);
                let speed = speed_bytes_per_sec(bytes, started_at.elapsed().as_millis());
                let _ = progress_tx.send(AppDownloadProgress::Transfer(TransferStats {
                    bytes,
                    total_bytes,
                    speed,
                }));
                if bytes >= total_bytes {
                    break;
                }
            }
        }
    }

    let bytes = downloaded_bytes.load(Ordering::Relaxed);
    let speed = speed_bytes_per_sec(bytes, started_at.elapsed().as_millis());
    let _ = progress_tx.send(AppDownloadProgress::Transfer(TransferStats {
        bytes,
        total_bytes,
        speed,
    }));
    Ok(())
}

fn validate_content_range(
    headers: &reqwest::header::HeaderMap,
    range: StagedRange,
    total_bytes: u64,
) -> Result<()> {
    let Some(value) = headers.get(reqwest::header::CONTENT_RANGE) else {
        return Ok(());
    };
    let value = value.to_str().context("Content-Range header is not valid UTF-8")?;
    let expected = format!("bytes {}-{}/{}", range.start, range.end, total_bytes);
    ensure!(value == expected, "Content-Range mismatch: expected `{expected}`, got `{value}`");
    Ok(())
}

async fn stream_package_to_pipe(
    response: reqwest::Response,
    mut writer: DuplexStream,
    progress_tx: UnboundedSender<AppDownloadProgress>,
    total_bytes: u64,
    cancellation_token: CancellationToken,
) -> Result<()> {
    let mut downloaded_bytes = 0_u64;
    let started_at = Instant::now();
    let mut last_emit = 0_u128;
    let mut stream = response.bytes_stream();
    let mut seen_first_chunk = false;

    loop {
        let next_chunk = stream.next();
        tokio::pin!(next_chunk);
        let slow_warning = tokio_time::sleep(SLOW_NETWORK_WARNING_THRESHOLD);
        tokio::pin!(slow_warning);
        let mut warned_slow = false;
        let maybe_chunk = loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!(downloaded_bytes, total_bytes, "Cancelled while streaming YARC package");
                    bail!("Operation cancelled")
                },
                _ = &mut slow_warning, if !warned_slow => {
                    warned_slow = true;
                    warn!(
                        downloaded_bytes,
                        total_bytes,
                        waiting_for = if seen_first_chunk { "YARC chunk" } else { "first YARC byte" },
                        wait_ms = SLOW_NETWORK_WARNING_THRESHOLD.as_millis() as u64,
                        "YARC stream is slow"
                    );
                },
                chunk = &mut next_chunk => break chunk,
            }
        };
        let Some(chunk) = maybe_chunk else {
            break;
        };
        let chunk = chunk.context("Failed to stream YARC chunk")?;
        if !seen_first_chunk {
            seen_first_chunk = true;
            debug!(first_chunk_bytes = chunk.len(), total_bytes, "Received first YARC bytes");
        }
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                info!(downloaded_bytes, total_bytes, "Cancelled while piping YARC chunk");
                bail!("Operation cancelled")
            },
            result = writer.write_all(&chunk) => {
                result.context("Failed to pipe YARC chunk")?;
            }
        }
        downloaded_bytes += chunk.len() as u64;

        let elapsed_millis = started_at.elapsed().as_millis();
        if elapsed_millis.saturating_sub(last_emit) >= STREAM_PROGRESS_INTERVAL_MILLIS {
            let speed = speed_bytes_per_sec(downloaded_bytes, elapsed_millis);
            let _ = progress_tx.send(AppDownloadProgress::Transfer(TransferStats {
                bytes: downloaded_bytes,
                total_bytes,
                speed,
            }));
            last_emit = elapsed_millis;
        }
    }

    let final_speed = speed_bytes_per_sec(downloaded_bytes, started_at.elapsed().as_millis());
    let _ = progress_tx.send(AppDownloadProgress::Transfer(TransferStats {
        bytes: downloaded_bytes,
        total_bytes,
        speed: final_speed,
    }));
    debug!(downloaded_bytes, total_bytes, "Finished streaming YARC package");
    writer.shutdown().await.context("Failed to finalize YARC package stream")?;
    Ok(())
}

async fn send_with_cancellation(
    request: reqwest::RequestBuilder,
    url: &str,
    cancellation_token: &CancellationToken,
) -> Result<reqwest::Response> {
    let response = request.send();
    tokio::pin!(response);
    let slow_warning = tokio_time::sleep(SLOW_NETWORK_WARNING_THRESHOLD);
    tokio::pin!(slow_warning);
    let mut warned_slow = false;

    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                info!(url, "Cancelled while waiting for response headers");
                bail!("Operation cancelled")
            },
            _ = &mut slow_warning, if !warned_slow => {
                warned_slow = true;
                warn!(
                    url,
                    wait_ms = SLOW_NETWORK_WARNING_THRESHOLD.as_millis() as u64,
                    "Still waiting for response headers"
                );
            }
            result = &mut response => break Ok(result?),
        }
    }
}

fn send_status(progress_tx: &UnboundedSender<AppDownloadProgress>, status: impl Into<String>) {
    let _ = progress_tx.send(AppDownloadProgress::Status(status.into()));
}

fn speed_bytes_per_sec(downloaded_bytes: u64, elapsed_millis: u128) -> u64 {
    if elapsed_millis == 0 {
        return 0;
    }
    ((downloaded_bytes as u128 * 1000) / elapsed_millis) as u64
}

async fn join_transfer_task(task: tokio::task::JoinHandle<Result<()>>) -> Result<()> {
    task.await.context("Transfer task failed to join")?
}

fn ensure_not_cancelled(cancellation_token: &CancellationToken) -> Result<()> {
    ensure!(!cancellation_token.is_cancelled(), "Operation cancelled");
    Ok(())
}

fn create_temp_dir(destination_parent: &Path, app_full_name: &str) -> Result<TempDir> {
    tempfile::Builder::new()
        .prefix(&format!(".{}.newrepo-", sanitize_filename::sanitize(app_full_name)))
        .tempdir_in(destination_parent)
        .with_context(|| {
            format!("Failed to create temporary directory in {}", destination_parent.display())
        })
}

async fn replace_directory(temp_dir: &Path, destination_dir: &Path) -> Result<()> {
    if destination_dir.exists() {
        remove_existing_path(destination_dir).await?;
    }

    fs::rename(temp_dir, destination_dir).await.with_context(|| {
        format!("Failed to replace {} with {}", destination_dir.display(), temp_dir.display())
    })?;
    Ok(())
}

async fn cleanup_temp_dir(temp_dir: &Path) -> Result<()> {
    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)
            .await
            .with_context(|| format!("Failed to remove {}", temp_dir.display()))?;
    }
    Ok(())
}

async fn remove_existing_path(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).await.with_context(|| format!("Failed to read {}", path.display()))?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .await
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    } else {
        fs::remove_file(path)
            .await
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_release() -> AppRelease {
        AppRelease {
            app_name: "Sample App".to_string(),
            release_name: "Sample Release".to_string(),
            package_name: "mr.com.example.sample".to_string(),
            version_code: "123".to_string(),
            megabytes: "321.5".to_string(),
            apk_name: "sample.apk".to_string(),
            apk_size: 123_456,
            last_modified_time: 1_700_000_000,
            manifest_hash: "a".repeat(64),
        }
    }

    #[test]
    fn maps_app_release_to_cloud_app() {
        let app = cloud_app_from_release(&sample_release()).expect("map release");
        assert_eq!(app.app_name, "Sample App");
        assert_eq!(app.full_name, "Sample Release");
        assert_eq!(app.package_name, "mr.com.example.sample");
        assert_eq!(app.true_package_name, "com.example.sample");
        assert_eq!(app.version_code, 123);
        assert_eq!(app.last_updated, "2023-11-14 22:13 UTC");
        assert_eq!(app.size, 321_500_000);
    }

    #[test]
    fn staged_ranges_use_single_part_below_min_size() {
        let ranges = staged_download_ranges(STAGED_MIN_PART_SIZE - 1);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, STAGED_MIN_PART_SIZE - 2);
    }

    #[test]
    fn staged_ranges_cap_part_count_to_workers() {
        let total_bytes = STAGED_MIN_PART_SIZE * 10;
        let ranges = staged_download_ranges(total_bytes);

        assert_eq!(ranges.len(), STAGED_DOWNLOAD_WORKERS);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges.last().expect("last range").end, total_bytes - 1);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end + 1, pair[1].start);
        }
    }
}
