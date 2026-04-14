use std::{
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use derive_more::Debug;
use futures::StreamExt as _;
use tokio::{
    fs,
    io::{AsyncWriteExt, DuplexStream},
    sync::{Mutex, mpsc::UnboundedSender},
};
use tokio_util::sync::CancellationToken;
use tracing::{instrument, warn};
use yarc::{
    app_list::{AppList, AppRelease},
    container::YarcReader,
    manifest::ReleaseManifest,
};

use super::{
    BuildStorageArgs, BuildStorageResult, Repo, RepoAppList, RepoCapabilities, RepoStorage,
};
use crate::{
    downloader::{AppDownloadProgress, TransferStats, config::DownloaderConfig, http_cache},
    models::CloudApp,
};

const YAAS_KEY_HEADER: &str = "x-yaas-key";
const STREAM_PROGRESS_INTERVAL_MILLIS: u128 = 500;

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
        let yarc_key =
            match fetch_yarc_key(http_client, &storage.list_url(), &cancellation_token).await {
                Ok(key) => key,
                Err(error) => {
                    if let Some(existing) = storage.current_key().await {
                        warn!(
                            error = error.as_ref() as &dyn Error,
                            "Failed to refresh NewRepo YARC key, reusing cached key"
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
            "new_repo/list.yarc",
            &cancellation_token,
        )
        .await
        .context("Failed to cache NewRepo app list")?;
        ensure_not_cancelled(&cancellation_token)?;

        let list_bytes = fs::read(&list_path)
            .await
            .with_context(|| format!("Failed to read {}", list_path.display()))?;
        let (app_list, _) = AppList::from_yarc(list_bytes.as_slice(), yarc_key)
            .await
            .context("Failed to decode NewRepo app list")?;

        let mut apps = Vec::with_capacity(app_list.releases.len());
        for release in &app_list.releases {
            match cloud_app_from_release(release) {
                Ok(app) => apps.push(app),
                Err(error) => {
                    warn!(
                        release_name = release.release_name,
                        error = error.as_ref() as &dyn Error,
                        "Skipping malformed NewRepo release"
                    );
                }
            }
        }

        storage.update_index(&app_list.releases, yarc_key).await;
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
        cache_dir: &Path,
        http_client: &reqwest::Client,
        progress_tx: UnboundedSender<AppDownloadProgress>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let RepoStorage::NewRepo(storage) = storage else {
            unreachable!("ffa storage passed to new-repo backend");
        };

        ensure_not_cancelled(&cancellation_token)?;
        send_status(&progress_tx, "Resolving release...");
        let release = storage.release_for_download(app_full_name).await.ok_or_else(|| {
            anyhow!(
                "No NewRepo release metadata found for `{app_full_name}`. Refresh the cloud app \
                 list and try again."
            )
        })?;
        let yarc_key = match storage.current_key().await {
            Some(key) => key,
            None => {
                send_status(&progress_tx, "Fetching decryption key...");
                let key =
                    fetch_yarc_key(http_client, &storage.list_url(), &cancellation_token).await?;
                storage.set_key(key).await;
                key
            }
        };

        let manifest_rel_path = format!("new_repo/manifests/{}.yarc", release.manifest_hash);
        send_status(&progress_tx, "Fetching manifest...");
        let manifest_path = cache_remote_file(
            http_client,
            &storage.manifest_url(&release.manifest_hash),
            cache_dir,
            &manifest_rel_path,
            &cancellation_token,
        )
        .await
        .context("Failed to cache NewRepo manifest")?;
        send_status(&progress_tx, "Decoding manifest...");
        let manifest_bytes = fs::read(&manifest_path)
            .await
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let (manifest, _) = ReleaseManifest::from_yarc(manifest_bytes.as_slice(), yarc_key)
            .await
            .context("Failed to decode NewRepo manifest")?;

        ensure!(
            manifest.release_key == release.release_name,
            "Manifest release key mismatch for `{}`: expected `{}`, got `{}`",
            app_full_name,
            release.release_name,
            manifest.release_key
        );

        let destination_parent = destination_dir
            .parent()
            .ok_or_else(|| anyhow!("Download destination must have a parent directory"))?;
        fs::create_dir_all(destination_parent)
            .await
            .with_context(|| format!("Failed to create {}", destination_parent.display()))?;

        let temp_dir = unique_temp_dir(destination_parent, app_full_name);
        let download_result = async {
            send_status(&progress_tx, "Requesting archive...");
            let response = send_with_cancellation(
                http_client.get(storage.blob_url(&manifest.yarc_id)),
                &cancellation_token,
            )
            .await
            .context("Failed to download NewRepo archive")?;
            let response = response.error_for_status().context("NewRepo archive request failed")?;

            let total_bytes = response.content_length().unwrap_or(manifest.yarc_size);
            fs::create_dir_all(&temp_dir)
                .await
                .with_context(|| format!("Failed to create {}", temp_dir.display()))?;

            let (writer, reader) = tokio::io::duplex(256 * 1024);
            send_status(&progress_tx, "Downloading archive...");
            let transfer_progress_tx = progress_tx.clone();
            let stream_task = tokio::spawn(stream_archive_to_pipe(
                response,
                writer,
                transfer_progress_tx,
                total_bytes,
                cancellation_token.clone(),
            ));

            let extract_result = YarcReader::new(yarc_key)
                .extract_to_directory(reader, &temp_dir)
                .await
                .context("Failed to extract YARC archive");
            let stream_result = join_transfer_task(stream_task).await;

            match (extract_result, stream_result) {
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Ok(_), Ok(())) => Ok(()),
            }?;

            ensure_not_cancelled(&cancellation_token)?;
            send_status(&progress_tx, "Restoring timestamps...");
            manifest
                .apply_metadata_to_directory(&temp_dir)
                .await
                .context("Failed to restore extracted YARC metadata")?;
            send_status(&progress_tx, "Verifying files...");
            ensure!(
                manifest
                    .verify_directory(&temp_dir)
                    .await
                    .context("Failed to verify extracted YARC archive")?,
                "Downloaded archive contents did not match the manifest"
            );

            send_status(&progress_tx, "Finalizing download...");
            replace_directory(&temp_dir, destination_dir).await
        }
        .await;

        if let Err(error) = cleanup_temp_dir(&temp_dir).await {
            warn!(
                path = %temp_dir.display(),
                error = error.as_ref() as &dyn Error,
                "Failed to clean up temporary YARC directory"
            );
        }

        download_result
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
    let response = send_with_cancellation(client.head(url), cancellation_token)
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
    tokio::select! {
        _ = cancellation_token.cancelled() => bail!("Operation cancelled"),
        result = http_cache::update_file_cached(client, url, &destination, cache_dir, None) => {
            result.with_context(|| format!("Failed to update cache for {url}"))?;
        }
    }
    Ok(destination)
}

async fn stream_archive_to_pipe(
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

    loop {
        let maybe_chunk = tokio::select! {
            _ = cancellation_token.cancelled() => bail!("Operation cancelled"),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = maybe_chunk else {
            break;
        };
        let chunk = chunk.context("Failed to stream NewRepo archive chunk")?;
        tokio::select! {
            _ = cancellation_token.cancelled() => bail!("Operation cancelled"),
            result = writer.write_all(&chunk) => {
                result.context("Failed to pipe NewRepo archive chunk")?;
            }
        }
        downloaded_bytes += chunk.len() as u64;

        let elapsed = started_at.elapsed();
        let elapsed_millis = elapsed.as_millis();
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
    writer.shutdown().await.context("Failed to finalize NewRepo archive stream")?;
    Ok(())
}

async fn send_with_cancellation(
    request: reqwest::RequestBuilder,
    cancellation_token: &CancellationToken,
) -> Result<reqwest::Response> {
    tokio::select! {
        _ = cancellation_token.cancelled() => bail!("Operation cancelled"),
        response = request.send() => Ok(response?),
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
    task.await.context("NewRepo transfer task failed to join")?
}

fn ensure_not_cancelled(cancellation_token: &CancellationToken) -> Result<()> {
    ensure!(!cancellation_token.is_cancelled(), "Operation cancelled");
    Ok(())
}

fn unique_temp_dir(destination_parent: &Path, app_full_name: &str) -> PathBuf {
    destination_parent.join(format!(
        ".{}.newrepo-{}",
        sanitize_filename::sanitize(app_full_name),
        uuid::Uuid::new_v4()
    ))
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
}
