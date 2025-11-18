use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, instrument, warn};

use crate::models::CloudApp;

#[derive(serde::Serialize)]
struct DownloadMetadata {
    #[serde(default)]
    format_version: u32,
    full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    downloaded_at: String,
}

#[derive(Debug)]
pub(crate) struct DownloadMetadataInfo {
    pub(crate) downloaded_at: Option<u64>,
    pub(crate) package_name: Option<String>,
    pub(crate) version_code: Option<u32>,
}

#[instrument(level = "debug", skip(cached), fields(app_full_name = %app_full_name, dir = %dst_dir.display()), err)]
pub(super) async fn write_download_metadata(
    cached: Option<CloudApp>,
    app_full_name: &str,
    dst_dir: &PathBuf,
    write_legacy_release: bool,
) -> Result<()> {
    let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_else(|_| "".to_string());

    let meta = DownloadMetadata {
        format_version: 1,
        full_name: app_full_name.to_string(),
        app_name: cached.as_ref().map(|a| a.app_name.clone()),
        package_name: cached.as_ref().map(|a| a.package_name.clone()),
        version_code: cached.as_ref().map(|a| a.version_code),
        last_updated: cached.as_ref().map(|a| a.last_updated.clone()),
        size: cached.as_ref().map(|a| a.size),
        downloaded_at: now,
    };

    let json = serde_json::to_string_pretty(&meta)?;
    let download_path = dst_dir.join("metadata.json");
    tokio::fs::write(&download_path, json)
        .await
        .with_context(|| format!("Failed to write {}", download_path.display()))?;
    info!(path = %download_path.display(), "Wrote download metadata");

    if write_legacy_release {
        if let Some(app) = cached {
            #[derive(serde::Serialize)]
            struct LegacyReleaseJson<'a> {
                #[serde(rename = "GameName")]
                game_name: &'a str,
                #[serde(rename = "ReleaseName")]
                release_name: String,
                #[serde(rename = "PackageName")]
                package_name: &'a str,
                #[serde(rename = "VersionCode")]
                version_code: u32,
                #[serde(rename = "LastUpdated")]
                last_updated: &'a str,
                #[serde(rename = "GameSize")]
                game_size: u64,
            }

            let size_mb = app.size / 1_000_000;
            let legacy = LegacyReleaseJson {
                game_name: &app.app_name,
                release_name: app_full_name.to_string(),
                package_name: &app.package_name,
                version_code: app.version_code,
                last_updated: &app.last_updated,
                game_size: size_mb,
            };

            let legacy_json = serde_json::to_string_pretty(&legacy)?;
            let legacy_path = dst_dir.join("release.json");
            tokio::fs::write(&legacy_path, legacy_json)
                .await
                .with_context(|| format!("Failed to write {}", legacy_path.display()))?;
            info!(path = %legacy_path.display(), "Wrote legacy release.json metadata");
        } else {
            warn!(app_full_name, "Could not write legacy release.json: app not found in cache");
        }
    }

    Ok(())
}

#[instrument(level = "debug", err, ret)]
pub(crate) async fn read_download_metadata(dir: &Path) -> Result<DownloadMetadataInfo> {
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
        && let Ok(text) = tokio::fs::read_to_string(&meta_path).await
        && let Ok(meta) = serde_json::from_str::<DownloadMetaPartial>(&text)
    {
        package_name = meta.package_name;
        version_code = meta.version_code;
        if let Some(dt) = meta.downloaded_at {
            ts_millis = Some(rfc3339_to_millis(&dt));
        }
    } else if meta_path_alt.exists()
        && let Ok(text) = tokio::fs::read_to_string(&meta_path_alt).await
        && let Ok(meta) = serde_json::from_str::<DownloadMetaPartial>(&text)
    {
        package_name = meta.package_name;
        version_code = meta.version_code;
        if let Some(dt) = meta.downloaded_at {
            ts_millis = Some(rfc3339_to_millis(&dt));

            // Only our metadata has downloaded_at, rename to metadata.json to avoid conflicts with QL
            let new_path = dir.join("metadata.json");
            if let Err(e) = tokio::fs::rename(&meta_path_alt, &new_path).await {
                warn!(error = %e, "Failed to rename our release.json to metadata.json");
            }
        }
    }

    Ok(DownloadMetadataInfo { downloaded_at: ts_millis, package_name, version_code })
}

fn rfc3339_to_millis(s: &str) -> u64 {
    // Parse RFC3339 in UTC
    match OffsetDateTime::parse(s, &Rfc3339) {
        Ok(dt) => (dt.unix_timestamp_nanos() / 1_000_000) as u64,
        Err(_) => 0,
    }
}
