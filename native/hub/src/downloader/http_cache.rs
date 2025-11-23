use std::{collections::HashMap, error::Error, path::Path};

use anyhow::{Context, Result};
use fs_err::tokio::{self as fs, File, OpenOptions};
use fs4::fs_err3_tokio::AsyncFileExt as _;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::{Duration, Instant, sleep},
};
use tokio_stream::StreamExt as _;
use tracing::{debug, instrument, warn};

/// Per-URL metadata kept for caching decisions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct MetaEntry {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub md5: String,
    pub size: u64,
}

/// Metadata store persisted in `cache_dir/meta.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MetaStore {
    entries: HashMap<String, MetaEntry>,
}

impl MetaStore {
    fn get(&self, url: &str) -> Option<&MetaEntry> {
        self.entries.get(url)
    }
    fn update(&mut self, url: String, entry: MetaEntry) {
        self.entries.insert(url, entry);
    }
}

async fn load_meta(dir: &Path) -> Result<MetaStore> {
    let path = dir.join("meta.json");
    if !path.exists() {
        return Ok(MetaStore::default());
    }
    let content = fs::read_to_string(&path)
        .await
        .with_context(|| format!("Failed to read {}", path.display()))?;
    match serde_json::from_str::<MetaStore>(&content) {
        Ok(meta) => Ok(meta),
        Err(e) => {
            warn!(
                error = &e as &dyn Error,
                path = %path.display(),
                "Invalid cache metadata, starting with empty store"
            );
            Ok(MetaStore::default())
        }
    }
}

async fn save_meta(dir: &Path, meta: &MetaStore) -> Result<()> {
    let path = dir.join("meta.json");
    let json = serde_json::to_string_pretty(meta)?;
    fs::write(&path, json).await.with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Apply `If-None-Match` and `If-Modified-Since` headers to a request based on previous metadata.
fn apply_conditional_headers(
    mut req: reqwest::RequestBuilder,
    prev: Option<&MetaEntry>,
) -> reqwest::RequestBuilder {
    use reqwest::header::{IF_MODIFIED_SINCE, IF_NONE_MATCH};
    if let Some(prev) = prev {
        if let Some(etag) = &prev.etag {
            req = req.header(IF_NONE_MATCH, etag);
        }
        if let Some(lm) = &prev.last_modified {
            req = req.header(IF_MODIFIED_SINCE, lm);
        }
    }
    req
}

#[derive(Debug, Clone, Default)]
struct HeaderMeta {
    etag: Option<String>,
    last_modified: Option<String>,
}

fn extract_header_meta(headers: &reqwest::header::HeaderMap) -> HeaderMeta {
    use reqwest::header::{ETAG, LAST_MODIFIED};
    let etag = headers.get(ETAG).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let last_modified =
        headers.get(LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    HeaderMeta { etag, last_modified }
}

/// Result of a conditional binary download.
#[derive(Debug)]
pub(crate) enum DownloadResult {
    NotModified,
    Downloaded(MetaEntry),
}

/// Download a file or use cached copy if available.
#[instrument(
    level = "debug",
    skip(client, progress),
    fields(url = url, dst = %dst.display(), cache_dir = %cache_dir.display())
)]
pub(crate) async fn update_file_cached(
    client: &reqwest::Client,
    url: &str,
    dst: &Path,
    cache_dir: &Path,
    progress: Option<fn(u64, Option<u64>)>,
) -> Result<DownloadResult> {
    fs::create_dir_all(cache_dir)
        .await
        .with_context(|| format!("Failed to create cache directory {}", cache_dir.display()))?;

    let meta_before = load_meta(cache_dir).await.unwrap_or_default();
    let prev = meta_before.get(url);

    let had_prev = prev.is_some();
    let local_file_missing = !dst.exists();
    let local_consistent = local_is_consistent(dst, prev).await.unwrap_or(false);
    let used_if_none_match = local_consistent && prev.and_then(|m| m.etag.as_ref()).is_some();
    let used_if_modified_since =
        local_consistent && prev.and_then(|m| m.last_modified.as_ref()).is_some();
    let attempted_conditional = used_if_none_match || used_if_modified_since;

    let mut resp = if local_consistent {
        apply_conditional_headers(client.get(url), prev).send().await?
    } else {
        client.get(url).send().await?
    };

    let initial_status = resp.status();
    let initial_status_code = initial_status.as_u16();
    let mut server_status = initial_status_code;

    if resp.status() == StatusCode::NOT_MODIFIED {
        if local_file_missing || !local_consistent {
            resp = client.get(url).send().await?;
            server_status = resp.status().as_u16();
        } else {
            debug!(
                has_previous_meta = had_prev,
                local_file_missing = local_file_missing,
                local_file_changed = !local_consistent && !local_file_missing,
                server_status = initial_status_code,
                remote_file_changed =
                    attempted_conditional && initial_status != StatusCode::NOT_MODIFIED,
                "Using cached file"
            );
            return Ok(DownloadResult::NotModified);
        }
    }

    let resp = resp.error_for_status()?;
    let header_meta = extract_header_meta(resp.headers());

    debug!(
        has_previous_meta = had_prev,
        local_file_missing = local_file_missing,
        local_file_changed = !local_consistent && !local_file_missing,
        remote_file_changed = attempted_conditional && initial_status != StatusCode::NOT_MODIFIED,
        server_status,
        content_length = ?resp.content_length(),
        etag = ?header_meta.etag,
        last_modified = ?header_meta.last_modified,
        "Downloading file"
    );

    let tmp = dst.with_extension("tmp");
    let mut tmp_file = fs::File::create(&tmp)
        .await
        .with_context(|| format!("Failed to create {}", tmp.display()))?;
    let mut downloaded: u64 = 0;
    let mut md5_ctx = md5::Context::new();
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    // Throttle progress updates to reduce UI/log noise.
    let mut last_emit = Instant::now();
    let min_interval = Duration::from_millis(200);
    let mut last_reported: u64 = 0;
    while let Some(item) = stream.next().await {
        let chunk = item?;
        tmp_file.write_all(&chunk).await?;
        md5_ctx.consume(&chunk);
        downloaded += chunk.len() as u64;
        if let Some(cb) = progress {
            let now = Instant::now();
            if now.duration_since(last_emit) >= min_interval && downloaded != last_reported {
                cb(downloaded, total);
                last_emit = now;
                last_reported = downloaded;
            }
        }
    }
    tmp_file.flush().await?;
    drop(tmp_file);
    if let Some(cb) = progress {
        // Ensure a final update at completion.
        cb(downloaded, total);
    }

    let completed_md5 = format!("{:x}", md5_ctx.finalize());
    let new_meta = MetaEntry {
        etag: header_meta.etag,
        last_modified: header_meta.last_modified,
        md5: completed_md5,
        size: downloaded,
    };
    swap_and_persist(cache_dir, &tmp, dst, url, &new_meta).await?;
    Ok(DownloadResult::Downloaded(new_meta))
}

/// Simple cross-process lock guarding metadata updates and the final rename.
struct MetaFileLock(File);
impl MetaFileLock {
    async fn acquire(cache_dir: &Path) -> Result<Self> {
        let lock_path = cache_dir.join("meta.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&lock_path)
            .await?;
        loop {
            match file.try_lock_exclusive()? {
                true => break,
                false => sleep(Duration::from_millis(20)).await,
            }
        }
        Ok(Self(file))
    }
}
impl Drop for MetaFileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Check if the local file matches the previously stored metadata (size + md5).
async fn local_is_consistent(dst: &Path, prev: Option<&MetaEntry>) -> Result<bool> {
    if !dst.exists() {
        return Ok(false);
    }
    let Some(prev) = prev else { return Ok(false) };

    let stored_size = prev.size;
    let meta = match fs::metadata(dst).await {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };
    if meta.len() != stored_size {
        return Ok(false);
    }

    let stored_md5 = &prev.md5;
    let current = match compute_md5_file(dst).await {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    Ok(&current == stored_md5)
}

/// Swap the temp file into place and persist metadata under the meta lock.
#[instrument(level = "debug", skip(new_meta))]
async fn swap_and_persist(
    cache_dir: &Path,
    tmp: &Path,
    dst: &Path,
    url: &str,
    new_meta: &MetaEntry,
) -> Result<()> {
    let _lock = MetaFileLock::acquire(cache_dir).await?;
    fs::rename(tmp, dst).await.with_context(|| format!("Failed to replace {}", dst.display()))?;
    let mut meta_after = load_meta(cache_dir).await.unwrap_or_default();
    meta_after.update(url.to_string(), new_meta.clone());
    if let Err(e) = save_meta(cache_dir, &meta_after).await {
        warn!(error = e.as_ref() as &dyn Error, dir = %cache_dir.display(), "Failed to persist meta.json");
    }
    Ok(())
}

pub(super) async fn compute_md5_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .await
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut buf = vec![0u8; 1024 * 64];
    let mut ctx = md5::Context::new();
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        ctx.consume(&buf[..n]);
    }
    Ok(format!("{:x}", ctx.finalize()))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;

    fn client() -> reqwest::Client {
        reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heals_missing_local_file_when_server_returns_304() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/file.bin";
        let etag = "\"etag-1\"";
        let last_modified = "Wed, 21 Oct 2015 07:28:00 GMT";

        // First: serve content with caching headers
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(b"DATA1")
                    .insert_header("ETag", etag)
                    .insert_header("Last-Modified", last_modified),
            )
            .mount(&server)
            .await;

        // On subsequent conditional request, server would reply 304
        Mock::given(method("GET"))
            .and(path(url_path))
            .and(header("If-None-Match", etag))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;

        // And for unconditional GET, still return 200 with body
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"DATA1"))
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("file.bin");

        // First download
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        match r {
            DownloadResult::Downloaded(_) => {}
            _ => panic!("expected Downloaded"),
        }
        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "DATA1");

        // Remove cached file, keep meta
        fs::remove_file(&dst).await.unwrap();
        assert!(!dst.exists());

        // Second call: should detect missing file and fetch unconditionally, despite server 304 branch
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        match r {
            DownloadResult::Downloaded(_) => {}
            _ => panic!("expected Downloaded on healing"),
        }
        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "DATA1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heals_changed_local_file_same_size() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/file2.bin";
        let etag = "\"etag-2\"";
        let last_modified = "Wed, 21 Oct 2015 07:28:00 GMT";

        // First: serve 4 bytes
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(b"ABCD")
                    .insert_header("ETag", etag)
                    .insert_header("Last-Modified", last_modified),
            )
            .mount(&server)
            .await;

        // 304 for conditional, 200 for unconditional (fallback)
        Mock::given(method("GET"))
            .and(path(url_path))
            .and(header("If-None-Match", etag))
            .and(header("If-Modified-Since", last_modified))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ABCD"))
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("file2.bin");

        // First download
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        match r {
            DownloadResult::Downloaded(_) => {}
            _ => panic!("expected Downloaded"),
        }
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "ABCD");

        // Corrupt file locally with the same size (4 bytes)
        fs::write(&dst, b"WXYZ").await.unwrap();

        // Second call must detect change (by md5) and fetch unconditionally
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        match r {
            DownloadResult::Downloaded(_) => {}
            _ => panic!("expected Downloaded after change"),
        }
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "ABCD");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn uses_cached_when_not_modified_and_local_consistent() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/cached.bin";
        let etag = "\"etag-3\"";
        let last_modified = "Wed, 21 Oct 2015 07:28:00 GMT";

        // Initial 200 with validators (scoped so it doesn't match second call)
        let _g1 = Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(b"CACHE")
                    .insert_header("ETag", etag)
                    .insert_header("Last-Modified", last_modified),
            )
            .mount_as_scoped(&server)
            .await;

        // Note: no 304 mock yet; installed after first successful download.

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("cached.bin");

        // First download (while _g1 is alive)
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        matches!(r, DownloadResult::Downloaded(_));
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "CACHE");

        drop(_g1); // ensure the generic 200 mock is removed

        // Install 304 for the conditional request on second call
        let _g304 = Mock::given(method("GET"))
            .and(path(url_path))
            .and(header("If-None-Match", etag))
            .respond_with(ResponseTemplate::new(304))
            .mount_as_scoped(&server)
            .await;

        // Second call should short‑circuit to NotModified and keep file intact
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        match r {
            DownloadResult::NotModified => {}
            _ => panic!("expected NotModified when validators match"),
        }
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "CACHE");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn updates_when_remote_changes() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/update.bin";

        let etag1 = "\"etag-A\"";
        let lm1 = "Wed, 21 Oct 2015 07:28:00 GMT";
        let etag2 = "\"etag-B\"";
        let lm2 = "Thu, 22 Oct 2015 07:28:00 GMT";

        // Initial download: OLD content (scoped so it doesn't match second call)
        let _g1 = Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(b"OLD")
                    .insert_header("ETag", etag1)
                    .insert_header("Last-Modified", lm1),
            )
            .mount_as_scoped(&server)
            .await;

        // Conditional fetch detects change and returns 200 NEW with new validators
        Mock::given(method("GET"))
            .and(path(url_path))
            .and(header("If-None-Match", etag1))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(b"NEW")
                    .insert_header("ETag", etag2)
                    .insert_header("Last-Modified", lm2),
            )
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("update.bin");

        // First download (while _g1 is alive)
        let _ = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "OLD");

        drop(_g1); // remove generic GET mock

        // Second should update
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        match r {
            DownloadResult::Downloaded(entry) => {
                assert_eq!(entry.etag.as_deref(), Some(etag2));
                assert_eq!(entry.last_modified.as_deref(), Some(lm2));
            }
            _ => panic!("expected Downloaded after remote change"),
        }
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "NEW");

        // Verify meta.json persisted with new validators
        let meta = load_meta(dir.path()).await.unwrap();
        let m = meta.get(&url).expect("meta entry exists");
        assert_eq!(m.etag.as_deref(), Some(etag2));
        assert_eq!(m.last_modified.as_deref(), Some(lm2));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn downloads_unconditionally_when_no_prev_meta_but_local_exists() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/nometa.bin";

        // Prepare local file without any meta.json
        let dst = dir.path().join("nometa.bin");
        fs::write(&dst, b"LOCAL").await.unwrap();

        // Server serves REMOTE content; since there is no meta yet, request is unconditional
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"REMOTE"))
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        matches!(r, DownloadResult::Downloaded(_));
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "REMOTE");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn recovers_from_invalid_meta_json() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/badmeta.bin";

        // Write invalid meta.json
        fs::create_dir_all(dir.path()).await.unwrap();
        fs::write(dir.path().join("meta.json"), b"not a json").await.unwrap();

        // Server response
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"OK"))
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("badmeta.bin");
        let _ = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "OK");

        // After successful download, meta.json should become valid and contain entry
        let meta = load_meta(dir.path()).await.unwrap();
        assert!(meta.get(&url).is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tolerate_persist_error_when_meta_path_is_dir() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/persist.bin";

        // Create a directory named meta.json to force save error
        fs::create_dir(dir.path().join("meta.json")).await.unwrap();

        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"DATA"))
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("persist.bin");

        // Should succeed even though saving meta fails internally
        let r = update_file_cached(&client, &url, &dst, dir.path(), None).await.unwrap();
        matches!(r, DownloadResult::Downloaded(_));
        assert_eq!(fs::read_to_string(&dst).await.unwrap(), "DATA");
        assert!(dir.path().join("meta.json").is_dir());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn progress_callback_invoked_at_least_once() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CALLS: AtomicU64 = AtomicU64::new(0);
        fn progress_cb(_downloaded: u64, _total: Option<u64>) {
            CALLS.fetch_add(1, Ordering::Relaxed);
        }

        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let url_path = "/progress.bin";

        // Any small body is fine; final callback is guaranteed
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"1234567890"))
            .mount(&server)
            .await;

        let client = client();
        let url = format!("{}{}", server.uri(), url_path);
        let dst = dir.path().join("progress.bin");
        CALLS.store(0, Ordering::Relaxed);
        let _ =
            update_file_cached(&client, &url, &dst, dir.path(), Some(progress_cb)).await.unwrap();
        assert!(CALLS.load(Ordering::Relaxed) >= 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn local_consistency_checks() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("file.txt");
        fs::write(&p, b"HELLO").await.unwrap();

        // With no previous meta, treat as inconsistent (unknown)
        assert!(!local_is_consistent(&p, None).await.unwrap());

        // Mismatch size -> inconsistent
        let bad_prev =
            MetaEntry { etag: None, last_modified: None, md5: String::from("deadbeef"), size: 999 };
        assert!(!local_is_consistent(&p, Some(&bad_prev)).await.unwrap());

        // Same size but different md5 -> inconsistent
        let bad_prev2 =
            MetaEntry { etag: None, last_modified: None, md5: String::from("deadbeef"), size: 5 };
        assert!(!local_is_consistent(&p, Some(&bad_prev2)).await.unwrap());

        // Correct md5 and size -> consistent
        let md5_now = compute_md5_file(&p).await.unwrap();
        let ok_prev = MetaEntry { etag: None, last_modified: None, md5: md5_now, size: 5 };
        assert!(local_is_consistent(&p, Some(&ok_prev)).await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn compute_md5_known_vector() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("vec.txt");
        fs::write(&p, b"abc").await.unwrap();
        let h = compute_md5_file(&p).await.unwrap();
        assert_eq!(h, "900150983cd24fb0d6963f7d28e17f72");
    }
}
