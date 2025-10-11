use anyhow::{Context, Result};
use futures::TryStreamExt as _;
use tokio::fs::File;
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, instrument};

use super::rclone::RcloneStorage;
use crate::models::CloudApp;

#[instrument(skip(storage, cancellation_token), fields(count))]
pub async fn fetch_app_list(
    storage: RcloneStorage,
    list_path: String,
    cache_dir: std::path::PathBuf,
    cancellation_token: CancellationToken,
) -> Result<Vec<CloudApp>> {
    let path = storage
        .download_file(list_path, cache_dir, Some(cancellation_token))
        .await
        .context("Failed to download game list file")?;

    debug!(path = %path.display(), "App list file downloaded, parsing...");
    let file = File::open(&path).await.context("could not open game list file")?;
    let mut reader = csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
    let records = reader.deserialize();
    let cloud_apps: Vec<CloudApp> =
        records.try_collect().await.context("Failed to parse game list file")?;

    Span::current().record("count", cloud_apps.len());
    Ok(cloud_apps)
}
