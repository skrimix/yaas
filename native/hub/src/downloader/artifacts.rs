use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use rinf::RustSignal;
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};
use tokio_stream::StreamExt as _;
use tracing::{info, instrument, warn};

use super::DownloaderConfig;

fn is_http_url(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.starts_with("http://") || v.starts_with("https://")
}

#[instrument(skip(cache_dir, cfg), fields(cache_dir = %cache_dir.display()))]
pub async fn prepare_artifacts(
    cache_dir: &Path,
    cfg: &DownloaderConfig,
) -> Result<(PathBuf, PathBuf)> {
    let bin_src = cfg.rclone_path.resolve_for_current_platform()?;
    let conf_src = cfg.rclone_config_path.as_str();

    let bin_is_url = is_http_url(&bin_src);
    let conf_is_url = is_http_url(conf_src);
    ensure!(
        bin_is_url == conf_is_url,
        "rclone_path and rclone_config_path must both be local or both be URLs"
    );

    if !bin_is_url {
        info!("Using local rclone binary and config");
        return Ok((PathBuf::from(bin_src), PathBuf::from(conf_src)));
    }

    let bin_dst = cache_dir.join(if cfg!(windows) { "rclone.exe" } else { "rclone" });
    let conf_dst = cache_dir.join("rclone.conf");

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()?;

    // Metadata for tracking file changes
    let mut meta = load_meta(cache_dir).await.unwrap_or_default();

    // Try to update config first
    match download_if_needed(&client, conf_src, &conf_dst, meta.get(conf_src)).await {
        Ok(DownloadResult::NotModified) => {
            info!("rclone config not modified, using cached copy");
        }
        Ok(DownloadResult::Downloaded(entry)) => {
            info!(path = %conf_dst.display(), "Updated rclone config cache");
            meta.update(conf_src.to_string(), entry);
        }
        Err(e) => {
            warn!(
                error = e.as_ref() as &dyn std::error::Error,
                "Failed to update rclone config, using cached copy if available"
            );
            ensure!(
                conf_dst.exists(),
                "Failed to download rclone config and no cached copy available: {:#}",
                e
            );
        }
    }

    // Then binary
    match download_if_needed(&client, &bin_src, &bin_dst, meta.get(&bin_src)).await {
        Ok(DownloadResult::NotModified) => {
            info!("rclone binary not modified, using cached copy");
        }
        Ok(DownloadResult::Downloaded(entry)) => {
            info!(path = %bin_dst.display(), "Updated rclone binary cache");
            // Ensure executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&bin_dst).await?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&bin_dst, perms).await.ok();
            }
            meta.update(bin_src.to_string(), entry);
        }
        Err(e) => {
            warn!(
                error = e.as_ref() as &dyn std::error::Error,
                "Failed to update rclone binary, using cached copy if available"
            );
            ensure!(
                bin_dst.exists(),
                "Failed to download rclone binary and no cached copy available: {:#}",
                e
            );
        }
    }

    let _ = save_meta(cache_dir, &meta).await;

    Ok((bin_dst, conf_dst))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MetaEntry {
    etag: Option<String>,
    last_modified: Option<String>,
}

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
    let content = fs::read_to_string(&path).await?;
    let meta: MetaStore = serde_json::from_str(&content)?;
    Ok(meta)
}

async fn save_meta(dir: &Path, meta: &MetaStore) -> Result<()> {
    let path = dir.join("meta.json");
    let json = serde_json::to_string_pretty(meta)?;
    fs::write(&path, json).await?;
    Ok(())
}

enum DownloadResult {
    NotModified,
    Downloaded(MetaEntry),
}

#[instrument(skip(client, prev), fields(url = %url, dst = %dst.display()), err)]
async fn download_if_needed(
    client: &reqwest::Client,
    url: &str,
    dst: &Path,
    prev: Option<&MetaEntry>,
) -> Result<DownloadResult> {
    use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
    let mut req = client.get(url);
    if let Some(prev) = prev {
        if let Some(etag) = &prev.etag {
            req = req.header(IF_NONE_MATCH, etag);
        }
        if let Some(lm) = &prev.last_modified {
            req = req.header(IF_MODIFIED_SINCE, lm);
        }
    }
    let resp = req.send().await?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(DownloadResult::NotModified);
    }
    let resp = resp.error_for_status()?;
    let etag = resp.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let last_modified =
        resp.headers().get(LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    // write body to file with progress
    let tmp = dst.with_extension("tmp");
    let mut file = fs::File::create(&tmp)
        .await
        .with_context(|| format!("Failed to create {}", tmp.display()))?;
    let mut downloaded: u64 = 0;
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        crate::models::signals::downloader::progress::DownloaderInitProgress {
            bytes: downloaded,
            total_bytes: total,
        }
        .send_signal_to_dart();
    }
    file.flush().await?;
    drop(file);
    fs::rename(&tmp, dst).await.with_context(|| format!("Failed to replace {}", dst.display()))?;
    Ok(DownloadResult::Downloaded(MetaEntry { etag, last_modified }))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::models::RclonePath;

    fn cfg_local(bin: &str, conf: &str) -> DownloaderConfig {
        DownloaderConfig {
            rclone_path: RclonePath::Single(bin.to_string()),
            rclone_config_path: conf.to_string(),
            remote_name_filter_regex: None,
            disable_randomize_remote: true,
            root_dir: "Quest Games".to_string(),
            list_path: "FFA.txt".to_string(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_artifacts_returns_local_paths_when_both_local() {
        let dir = tempdir().unwrap();
        let cfg = cfg_local("/bin/echo", "/tmp/rclone.conf");
        let (bin, conf) = prepare_artifacts(dir.path(), &cfg).await.expect("ok");
        assert_eq!(bin, PathBuf::from("/bin/echo"));
        assert_eq!(conf, PathBuf::from("/tmp/rclone.conf"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_artifacts_errors_when_mixed_url_and_local() {
        let dir = tempdir().unwrap();
        // URL for rclone, local for config
        let cfg = DownloaderConfig {
            rclone_path: RclonePath::Single("http://127.0.0.1/rclone".to_string()),
            rclone_config_path: "/tmp/rclone.conf".to_string(),
            remote_name_filter_regex: None,
            disable_randomize_remote: true,
            root_dir: "Quest Games".to_string(),
            list_path: "FFA.txt".to_string(),
        };
        let err = prepare_artifacts(dir.path(), &cfg).await.unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("must both be local or both be URLs"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_artifacts_downloads_and_caches_http() {
        let dir = tempdir().unwrap();
        let server = MockServer::start().await;
        let conf_path = "/rclone.conf";
        let bin_path = "/rclone";
        let conf_etag = "\"conf-etag\"";
        let bin_etag = "\"bin-etag\"";
        let last_modified = "Wed, 21 Oct 2015 07:28:00 GMT";

        // First-run mocks: 200 with ETag + Last-Modified
        Mock::given(method("GET"))
            .and(path(conf_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("[remote]\nconfig=true")
                    .insert_header("ETag", conf_etag)
                    .insert_header("Last-Modified", last_modified),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(bin_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("binary")
                    .insert_header("ETag", bin_etag)
                    .insert_header("Last-Modified", last_modified),
            )
            .mount(&server)
            .await;

        // Second-run mocks: require caching headers and return 304
        Mock::given(method("GET"))
            .and(path(conf_path))
            .and(header("If-None-Match", conf_etag))
            .and(header("If-Modified-Since", last_modified))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(bin_path))
            .and(header("If-None-Match", bin_etag))
            .and(header("If-Modified-Since", last_modified))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;

        let cfg = DownloaderConfig {
            rclone_path: RclonePath::Single(format!("{}{}", server.uri(), bin_path)),
            rclone_config_path: format!("{}{}", server.uri(), conf_path),
            remote_name_filter_regex: None,
            disable_randomize_remote: true,
            root_dir: "Quest Games".to_string(),
            list_path: "FFA.txt".to_string(),
        };

        // First run downloads both files
        let (bin, conf) = prepare_artifacts(dir.path(), &cfg).await.expect("first run ok");
        assert!(bin.exists());
        assert!(conf.exists());

        // On Unix, ensure executable bit set for binary
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = tokio::fs::metadata(&bin).await.unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "binary should be executable");
        }

        // Second run: server replies 304; function should still succeed and use cache
        let (bin2, conf2) = prepare_artifacts(dir.path(), &cfg).await.expect("second run ok");
        assert_eq!(bin2, bin);
        assert_eq!(conf2, conf);
    }
}
