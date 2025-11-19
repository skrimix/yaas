use std::{
    error::Error,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use rinf::RustSignal;
use tokio::fs;
use tracing::{debug, info, instrument, warn};

use super::{
    DownloaderConfig,
    http_cache::{self, DownloadResult},
};
use crate::{
    archive::{extract_single_from_archive, list_archive_file_paths},
    downloader::{http_cache::compute_md5_file, repo},
    models::signals::downloader::progress::DownloaderInitProgress,
};

fn is_http_url(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.starts_with("http://") || v.starts_with("https://")
}

fn is_zip_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let path = lower.split('?').next().unwrap_or(&lower);
    path.ends_with(".zip")
}

#[instrument(skip(cache_dir, cfg), fields(cache_dir = %cache_dir.display()))]
pub(crate) async fn prepare_artifacts(
    cache_dir: &Path,
    cfg: &DownloaderConfig,
) -> Result<(PathBuf, PathBuf)> {
    let bin_source = cfg.rclone_path.resolve_for_current_platform()?;
    let maybe_config_source = cfg.rclone_config_path.as_deref();

    let bin_is_url = is_http_url(&bin_source);
    let config_is_url = maybe_config_source.map(is_http_url).unwrap_or(false);

    if maybe_config_source.is_none() {
        let repo = repo::make_repo_from_config(cfg);
        // If the repo provides its own config we only handle the binary here.
        if let Some(conf_name) = repo.generated_config_filename() {
            if !bin_is_url {
                let conf_dst = cache_dir.join(conf_name);
                return Ok((PathBuf::from(bin_source), conf_dst));
            } else {
                // Remote rclone binary, download it, but skip config (generated later by repo).
                let bin_dst = cache_dir.join(if cfg!(windows) { "rclone.exe" } else { "rclone" });
                let client = build_http_client()?;
                if is_zip_url(&bin_source) {
                    ensure_remote_rclone_from_zip(&client, &bin_source, cache_dir, &bin_dst)
                        .await?;
                } else {
                    ensure_remote_file(
                        &client,
                        &bin_source,
                        &bin_dst,
                        cache_dir,
                        true,
                        "rclone binary",
                    )
                    .await?;
                }
                let conf_dst = cache_dir.join(conf_name);
                return Ok((bin_dst, conf_dst));
            }
        }
    }

    let config_source = match maybe_config_source {
        Some(v) => v,
        None => {
            bail!("rclone_config_path is required for this repository layout")
        }
    };
    ensure!(
        bin_is_url == config_is_url,
        "rclone_path and rclone_config_path must both be local or both be URLs"
    );

    if !bin_is_url {
        info!("Using local rclone binary and config");
        return Ok((PathBuf::from(bin_source), PathBuf::from(config_source)));
    }

    let bin_dst = cache_dir.join(if cfg!(windows) { "rclone.exe" } else { "rclone" });
    let conf_dst = cache_dir.join("rclone.conf");

    let client = build_http_client()?;

    ensure_remote_file(&client, config_source, &conf_dst, cache_dir, false, "rclone config")
        .await?;

    if is_zip_url(&bin_source) {
        ensure_remote_rclone_from_zip(&client, &bin_source, cache_dir, &bin_dst).await?;
    } else {
        ensure_remote_file(&client, &bin_source, &bin_dst, cache_dir, true, "rclone binary")
            .await?;
    }

    Ok((bin_dst, conf_dst))
}

fn init_progress(bytes: u64, total: Option<u64>) {
    DownloaderInitProgress { bytes, total_bytes: total }.send_signal_to_dart();
}

fn build_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()?)
}

#[instrument(level = "debug", skip(client), fields(src = %src, dst = %dst.display(), label = label), err)]
async fn ensure_remote_file(
    client: &reqwest::Client,
    src: &str,
    dst: &Path,
    cache_dir: &Path,
    set_executable: bool,
    label: &str,
) -> Result<()> {
    match http_cache::update_file_cached(client, src, dst, cache_dir, Some(init_progress)).await {
        Ok(DownloadResult::NotModified) => {
            debug!("{} not modified, using cached copy", label);
        }
        Ok(DownloadResult::Downloaded(_entry)) => {
            debug!(path = %dst.display(), "Updated {} cache", label);
            if set_executable {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(dst).await?.permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(dst, perms).await.ok();
                }
            }
        }
        Err(e) => {
            warn!(
                error = e.as_ref() as &dyn Error,
                "Failed to update {}, using cached copy if available", label
            );
            ensure!(
                dst.exists(),
                "Failed to download {} and no cached copy available: {:#}",
                label,
                e
            );
        }
    }

    Ok(())
}

#[instrument(level = "debug", skip(client), fields(url = %url, bin = %bin_dst.display()), err)]
async fn ensure_remote_rclone_from_zip(
    client: &reqwest::Client,
    url: &str,
    cache_dir: &Path,
    bin_dst: &Path,
) -> Result<()> {
    let zip_path = cache_dir.join("rclone.zip");
    let md5_path = cache_dir.join("rclone.bin.md5");

    match http_cache::update_file_cached(client, url, &zip_path, cache_dir, Some(init_progress))
        .await
    {
        Ok(DownloadResult::NotModified) => {
            debug!("rclone.zip not modified");
            if bin_dst.exists() && md5_path.exists() {
                // TODO: maybe we can remove this and similar checks, now that we have a cache dir per config?
                match (compute_md5_file(bin_dst).await, fs::read_to_string(&md5_path).await) {
                    (Ok(current), Ok(expected)) if current.trim() == expected.trim() => {
                        debug!("Existing rclone binary matches stored checksum");
                        return Ok(());
                    }
                    _ => {
                        warn!("Local rclone binary checksum missing/mismatch, re-extracting");
                    }
                }
            } else if bin_dst.exists() {
                debug!("Checksum file missing, re-extracting to ensure correctness");
            }

            info!("Extracting rclone binary from cached zip");
            extract_rclone_from_zip(&zip_path, cache_dir, bin_dst).await?;
            // Update checksum after (re)extraction
            if let Err(e) = write_md5_file(bin_dst, &md5_path).await {
                warn!(error = e.as_ref() as &dyn Error, "Failed to write rclone MD5 stamp");
            }
        }
        Ok(DownloadResult::Downloaded(_)) => {
            info!(path = %zip_path.display(), "Fetched rclone zip, extracting binary");
            extract_rclone_from_zip(&zip_path, cache_dir, bin_dst).await?;
            // Persist checksum for future NotModified fast‑path
            if let Err(e) = write_md5_file(bin_dst, &md5_path).await {
                warn!(error = e.as_ref() as &dyn Error, "Failed to write rclone MD5 stamp");
            }
        }
        Err(e) => {
            warn!(
                error = e.as_ref() as &dyn Error,
                "Failed to update rclone.zip, attempting to use cached copy",
            );
            ensure!(
                zip_path.exists(),
                "Failed to download rclone zip and no cached copy available: {:#}",
                e
            );
            if !bin_dst.exists() {
                extract_rclone_from_zip(&zip_path, cache_dir, bin_dst).await?;
                if let Err(e) = write_md5_file(bin_dst, &md5_path).await {
                    warn!(error = e.as_ref() as &dyn Error, "Failed to write rclone MD5 stamp");
                }
            }
        }
    }

    // Ensure executable bit on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(bin_dst).await?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(bin_dst, perms).await.ok();
    }

    Ok(())
}

/// Extract rclone binary from the provided zip into `bin_dst` within `cache_dir`.
async fn extract_rclone_from_zip(zip_path: &Path, cache_dir: &Path, bin_dst: &Path) -> Result<()> {
    let entries = list_archive_file_paths(zip_path)
        .await
        .with_context(|| format!("Failed to list entries of {}", zip_path.display()))?;

    let target_name = if cfg!(windows) { "rclone.exe" } else { "rclone" };

    // Prefer root-level file; otherwise a nested entry like "rclone-v.../rclone"
    let mut candidates: Vec<&str> = entries
        .iter()
        .filter_map(|p| {
            let last = p.rsplit('/').next().unwrap_or(p.as_str());
            if last == target_name { Some(p.as_str()) } else { None }
        })
        .collect();

    ensure!(!candidates.is_empty(), "No '{}' entry found in {}", target_name, zip_path.display());

    // Pick the shortest path to prefer root-level when present
    candidates.sort_by_key(|s| s.len());
    let chosen = candidates[0];

    // Extract only the chosen entry, flattening the path
    extract_single_from_archive(zip_path, cache_dir, chosen)
        .await
        .with_context(|| format!("Failed to extract '{}' from {}", chosen, zip_path.display()))?;

    let extracted_path = cache_dir.join(target_name);
    if extracted_path != bin_dst {
        // Replace existing bin_dst if present
        if bin_dst.exists() {
            let _ = fs::remove_file(bin_dst).await;
        }
        fs::rename(&extracted_path, bin_dst)
            .await
            .with_context(|| format!("Failed to place rclone to {}", bin_dst.display()))?;
    }

    Ok(())
}

async fn write_md5_file(bin_dst: &Path, md5_path: &Path) -> Result<()> {
    let md5 = compute_md5_file(bin_dst).await.context("Failed to compute MD5")?;
    fs::write(md5_path, md5.into_bytes()).await.context("Failed to write MD5 file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::downloader::config::{RclonePath, RepoLayoutKind};

    fn cfg_local(bin: &str, conf: &str) -> DownloaderConfig {
        DownloaderConfig {
            id: "test".to_string(),
            rclone_path: RclonePath::Single(bin.to_string()),
            rclone_config_path: Some(conf.to_string()),
            share_remote_name: None,
            share_remote_path: None,
            remote_name_filter_regex: None,
            disable_randomize_remote: true,
            layout: RepoLayoutKind::Ffa,
            root_dir: "Quest Games".to_string(),
            list_path: "FFA.txt".to_string(),
            vrp_public_url: "".to_string(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_artifacts_returns_local_paths_when_both_local() {
        let dir = tempdir().unwrap();
        let cfg = cfg_local("/bin/echo", "/tmp/rclone.conf");
        let (bin, conf) =
            prepare_artifacts(dir.path(), &cfg).await.expect("Prepare artifacts failed");
        assert_eq!(bin, PathBuf::from("/bin/echo"));
        assert_eq!(conf, PathBuf::from("/tmp/rclone.conf"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_artifacts_errors_when_mixed_url_and_local() {
        let dir = tempdir().unwrap();
        // URL for rclone, local for config
        let cfg = DownloaderConfig {
            id: "test".to_string(),
            rclone_path: RclonePath::Single("http://127.0.0.1/rclone".to_string()),
            rclone_config_path: Some("/tmp/rclone.conf".to_string()),
            share_remote_name: None,
            share_remote_path: None,
            remote_name_filter_regex: None,
            disable_randomize_remote: true,
            layout: RepoLayoutKind::Ffa,
            root_dir: "Quest Games".to_string(),
            list_path: "FFA.txt".to_string(),
            vrp_public_url: "".to_string(),
        };
        let err =
            prepare_artifacts(dir.path(), &cfg).await.expect_err("Prepare artifacts should fail");
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
            id: "test".to_string(),
            rclone_path: RclonePath::Single(format!("{}{}", server.uri(), bin_path)),
            rclone_config_path: Some(format!("{}{}", server.uri(), conf_path)),
            share_remote_name: None,
            share_remote_path: None,
            remote_name_filter_regex: None,
            disable_randomize_remote: true,
            layout: RepoLayoutKind::Ffa,
            root_dir: "Quest Games".to_string(),
            list_path: "FFA.txt".to_string(),
            vrp_public_url: "".to_string(),
        };

        // First run downloads both files
        let (bin, conf) = prepare_artifacts(dir.path(), &cfg).await.expect("First run failed");
        assert!(bin.exists());
        assert!(conf.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = tokio::fs::metadata(&bin).await.unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "Binary should be executable");
        }

        // Second run: server replies 304, function should still succeed and use cache
        let (bin2, conf2) = prepare_artifacts(dir.path(), &cfg).await.expect("Second run failed");
        assert_eq!(bin2, bin);
        assert_eq!(conf2, conf);
    }
}
