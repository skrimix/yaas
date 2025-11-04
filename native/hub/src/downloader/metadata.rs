use std::path::PathBuf;

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

#[instrument(skip(cached), fields(app_full_name = %app_full_name, dir = %dst_dir.display()), err)]
pub async fn write_download_metadata(
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
