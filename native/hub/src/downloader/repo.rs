use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::Engine as _;
use derive_more::Debug;
use futures::TryStreamExt as _;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    fs::{self, File},
    sync::OnceCell,
};
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, error, instrument, warn};

use super::{http_cache, rclone::RcloneStorage};
use crate::{
    archive::decompress_archive,
    models::{CloudApp, DownloaderConfig, RepoLayoutKind},
};

/// High-level operations a repository must implement.
#[derive(Debug)]
pub struct BuildStorageResult {
    pub storage: RcloneStorage,
    /// If Some, Downloader should persist this remote name into settings.
    pub persist_remote: Option<String>,
}

#[async_trait]
pub trait Repo: Send + Sync {
    fn id(&self) -> &'static str;

    async fn build_storage(&self, args: BuildStorageArgs<'_>) -> Result<BuildStorageResult>;

    async fn load_app_list(
        &self,
        storage: RcloneStorage,
        list_path: String,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<Vec<CloudApp>>;

    /// Source path under the root directory for download.
    fn source_for_download(&self, app_full_name: &str) -> String;

    /// Optional pre-download check that can decide to skip the transfer.
    /// Default: proceed with download.
    #[allow(unused_variables)]
    async fn pre_download(
        &self,
        storage: &RcloneStorage,
        app_full_name: &str,
        dst_dir: &Path,
        http_client: &reqwest::Client,
        cache_dir: &Path,
        cancellation_token: CancellationToken,
    ) -> Result<PreDownloadDecision> {
        Ok(PreDownloadDecision::Proceed)
    }

    /// Optional post-download hook. Used for any post-processing of the downloaded files.
    #[allow(unused_variables)]
    async fn post_download(
        &self,
        app_full_name: &str,
        dst_dir: &Path,
        http_client: &reqwest::Client,
        cache_dir: &Path,
        // Optional status updates sender for surfacing UI progress while post-processing
        status_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        Ok(())
    }

    /// If the repo generates its own rclone config at runtime, return the
    /// suggested filename to be used. Otherwise None.
    fn generated_config_filename(&self) -> Option<&'static str> {
        None
    }
}

/// Factory: choose a concrete repo based on config.
pub fn make_repo_from_config(cfg: &DownloaderConfig) -> Arc<dyn Repo> {
    match cfg.layout {
        RepoLayoutKind::VRPPublic => Arc::new(VRPPublicRepo::from_config(cfg)),
        RepoLayoutKind::FFA => Arc::new(FFARepo {}),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreDownloadDecision {
    Proceed,
    SkipAlreadyPresent,
}

/// Arguments for building storage, passed to repo implementations.
#[derive(Debug)]
pub struct BuildStorageArgs<'a> {
    pub rclone_path: &'a Path,
    pub rclone_config_path: &'a Path,
    pub root_dir: &'a str,
    /// Remote selected by Downloader. Can be overridden by repo.
    pub remote_name: &'a str,
    pub bandwidth_limit: &'a str,
    pub remote_name_filter_regex: Option<String>,
    #[debug(skip)]
    pub http_client: &'a reqwest::Client,
    pub cache_dir: &'a Path,
}

/// FFA layout – direct files and list under a configurable remote/root.
#[derive(Debug, Clone, Default)]
pub struct FFARepo {}

#[async_trait]
impl Repo for FFARepo {
    fn id(&self) -> &'static str {
        "ffa"
    }

    #[instrument(name = "repo.build_storage", fields(layout = %self.id()))]
    async fn build_storage(&self, args: BuildStorageArgs<'_>) -> Result<BuildStorageResult> {
        debug!("Using repository layout: FFA");
        let storage = RcloneStorage::new(
            args.rclone_path.to_path_buf(),
            args.rclone_config_path.to_path_buf(),
            args.root_dir.to_string(),
            args.remote_name.to_string(),
            args.bandwidth_limit.to_string(),
            args.remote_name_filter_regex.clone(),
        );
        Ok(BuildStorageResult { storage, persist_remote: None })
    }

    #[instrument(name = "repo.load_app_list", skip(storage, _http_client, cancellation_token), fields(layout = %self.id()))]
    async fn load_app_list(
        &self,
        storage: RcloneStorage,
        list_path: String,
        cache_dir: &Path,
        _http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<Vec<CloudApp>> {
        let path = storage
            .download_file(list_path, cache_dir.to_path_buf(), Some(cancellation_token))
            .await
            .context("Failed to download game list file")?;

        debug!(path = %path.display(), "App list file downloaded, parsing...");
        let file = File::open(&path).await.context("Could not open game list file")?;
        let mut reader =
            csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
        let records = reader.deserialize();
        let cloud_apps: Vec<CloudApp> =
            records.try_collect().await.context("Failed to parse game list file")?;

        Span::current().record("count", cloud_apps.len());
        Ok(cloud_apps)
    }

    fn source_for_download(&self, app_full_name: &str) -> String {
        app_full_name.to_string()
    }
}

/// VRP-public layout – compressed+encrypted metadata, renamed+compressed+encrypted releases
#[derive(Debug, Clone)]
pub struct VRPPublicRepo {
    pub public_url: String,
    pub meta_archive: String,
    pub list_filename: String,
    pub remote_name: String,
    state: OnceCell<VRPPublicState>,
}

impl VRPPublicRepo {
    #[instrument(ret)]
    pub fn from_config(cfg: &DownloaderConfig) -> Self {
        Self {
            public_url: cfg.vrp_public_url.clone(),
            meta_archive: "meta.7z".to_string(),
            list_filename: "VRP-GameList.txt".to_string(),
            remote_name: "VRP-Public".to_string(),
            state: OnceCell::new(),
        }
    }

    /// Hashing scheme: md5(release_name + "\n").
    fn hash_for_release(full_name: &str) -> String {
        let s = format!("{}\n", full_name);
        let digest = md5::compute(s.as_bytes());
        format!("{:x}", digest)
    }

    fn source_for(&self, full_name: &str) -> String {
        format!("{}/", Self::hash_for_release(full_name))
    }

    /// Write a minimal rclone config with an HTTP remote bound to `base_uri`.
    #[instrument(level = "debug", ret)]
    async fn write_http_remote_config(&self, dir: &Path, base_uri: &str) -> Result<PathBuf> {
        let filename = self.generated_config_filename().context("No generated config filename")?;
        let path = dir.join(filename);
        let content = format!("[{}]\ntype = http\nurl = {}\n\n", self.remote_name, base_uri);
        fs::write(&path, content)
            .await
            .with_context(|| format!("Failed to write rclone config to {}", path.display()))?;
        Ok(path)
    }

    /// Create a storage handle for this repo using the given rclone binary and cache dir.
    #[instrument(level = "debug")]
    fn make_storage(
        &self,
        rclone_path: PathBuf,
        rclone_config_path: PathBuf,
        bandwidth_limit: &str,
        remote_filter_regex: Option<String>,
    ) -> RcloneStorage {
        RcloneStorage::new(
            rclone_path,
            rclone_config_path,
            String::new(),
            self.remote_name.clone(),
            bandwidth_limit.to_string(),
            remote_filter_regex,
        )
    }

    async fn fetch_public_json(
        &self,
        client: &reqwest::Client,
        cache_dir: &Path,
    ) -> Result<serde_json::Value> {
        let path = cache_dir.join("vrp-public.json");
        http_cache::update_file_cached(client, &self.public_url, &path, cache_dir, None)
            .await
            .context("Failed to download public VRP config")?;
        let body = fs::read_to_string(path).await.context("Failed to read public VRP config")?;
        Ok(serde_json::from_str(&body)?)
    }

    async fn ensure_initialized(
        &self,
        client: &reqwest::Client,
        cache_dir: &Path,
    ) -> Result<&VRPPublicState> {
        self.state
            .get_or_try_init(|| async {
                let json = self.fetch_public_json(client, cache_dir).await?;
                let base_uri = json
                    .get("baseUri")
                    .and_then(|v| v.as_str())
                    .context("baseUri missing in vrp-public.json")?
                    .to_string();
                let pass_b64 = json
                    .get("password")
                    .and_then(|v| v.as_str())
                    .context("password missing in vrp-public.json")?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(pass_b64)
                    .context("Invalid base64 password")?;
                let password =
                    String::from_utf8(bytes).context("Invalid utf8 in decoded password")?;
                Ok(VRPPublicState { base_uri, password })
            })
            .await
    }
}

#[derive(Debug, Clone)]
struct VRPPublicState {
    base_uri: String,
    password: String,
}

#[async_trait]
impl Repo for VRPPublicRepo {
    fn id(&self) -> &'static str {
        "vrp-public"
    }

    #[instrument(
        name = "repo.build_storage",
        fields(layout = %self.id(), remote = %self.remote_name)
    )]
    async fn build_storage(&self, args: BuildStorageArgs<'_>) -> Result<BuildStorageResult> {
        debug!("Using repository layout: VRP-public");
        let state = self.ensure_initialized(args.http_client, args.cache_dir).await?;
        let conf_path = self.write_http_remote_config(args.cache_dir, &state.base_uri).await?;
        let storage = self.make_storage(
            args.rclone_path.to_path_buf(),
            conf_path,
            args.bandwidth_limit,
            args.remote_name_filter_regex.clone(),
        );
        Ok(BuildStorageResult { storage, persist_remote: Some(self.remote_name.clone()) })
    }

    #[instrument(
        name = "repo.load_app_list",
        skip(_list_path, http_client, cancellation_token),
        fields(layout = %self.id(), list = %self.list_filename)
    )]
    async fn load_app_list(
        &self,
        storage: RcloneStorage,
        _list_path: String,
        cache_dir: &Path,
        http_client: &reqwest::Client,
        cancellation_token: CancellationToken,
    ) -> Result<Vec<CloudApp>> {
        let meta_path = storage
            .download_file(
                self.meta_archive.clone(),
                cache_dir.to_path_buf(),
                Some(cancellation_token.clone()),
            )
            .await?;

        let pass = {
            let state = self.ensure_initialized(http_client, cache_dir).await?;
            state.password.clone()
        };

        decompress_archive(
            &meta_path,
            cache_dir,
            Some(&pass),
            Some(&[self.list_filename.as_str()]),
            Some(cancellation_token.clone()),
        )
        .await
        .with_context(|| "Failed to extract meta.7z")?;

        if let Err(e) = fs::remove_file(&meta_path).await {
            warn!(error = &e as &dyn Error, "Failed to remove meta.7z");
        }

        let list_path = cache_dir.join(&self.list_filename);
        let file = File::open(&list_path)
            .await
            .with_context(|| format!("Could not open {}", list_path.display()))?;
        let mut reader =
            csv_async::AsyncReaderBuilder::new().delimiter(b';').create_deserializer(file);
        let records = reader.deserialize();
        let apps: Vec<CloudApp> =
            records.try_collect().await.context("Failed to parse VRP-public list")?;
        Ok(apps)
    }

    fn source_for_download(&self, app_full_name: &str) -> String {
        self.source_for(app_full_name)
    }

    #[allow(unused_variables)]
    #[instrument(
        name = "repo.pre_download",
        skip(storage, http_client, cancellation_token),
        fields(layout = %self.id(), app = %app_full_name)
    )]
    async fn pre_download(
        &self,
        storage: &RcloneStorage,
        app_full_name: &str,
        dst_dir: &Path,
        http_client: &reqwest::Client,
        cache_dir: &Path,
        cancellation_token: CancellationToken,
    ) -> Result<PreDownloadDecision> {
        let stamp_path = dst_dir.join("vrp_stamp.json");
        if !stamp_path.is_file() {
            return Ok(PreDownloadDecision::Proceed);
        }

        // Check that we have something extracted already
        let mut has_non_archive = false;
        if let Ok(mut rd) = fs::read_dir(dst_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                let name = entry.file_name();
                if let Some(n) = name.to_str()
                    && !n.ends_with(".7z")
                    && !n.contains(".7z.")
                {
                    has_non_archive = true;
                    break;
                }
            }
        }
        if !has_non_archive {
            return Ok(PreDownloadDecision::Proceed);
        }

        #[derive(serde::Deserialize, Debug)]
        struct StampPart {
            name: String,
            size: u64,
            mod_time: String,
        }
        #[derive(serde::Deserialize, Debug)]
        #[allow(unused)]
        struct Stamp {
            hash: String,
            parts: Vec<StampPart>,
        }

        let stamp: Stamp = match serde_json::from_slice(
            &fs::read(&stamp_path).await.context("Failed to read vrp_stamp.json")?,
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = &e as &dyn Error, path = %stamp_path.display(), "Invalid vrp_stamp.json, ignoring");
                return Ok(PreDownloadDecision::Proceed);
            }
        };

        // Compare with current remote listing
        let source = self.source_for_download(app_full_name);
        let entries = storage.list_dir_json(source).await.unwrap_or_default();
        #[derive(Clone)]
        struct Part {
            name: String,
            size: u64,
            mod_time: Option<OffsetDateTime>,
        }
        fn parse_rfc3339_opt(s: &str) -> Option<OffsetDateTime> {
            time::OffsetDateTime::parse(s, &Rfc3339).ok()
        }
        let mut remote_parts: Vec<Part> = entries
            .into_iter()
            .filter(|e| !e.is_dir)
            .map(|e| Part {
                name: e.name,
                size: e.size,
                mod_time: e.mod_time.as_deref().and_then(parse_rfc3339_opt),
            })
            .collect();
        remote_parts.sort_by(|a, b| a.name.cmp(&b.name));

        let mut local_parts: Vec<Part> = stamp
            .parts
            .into_iter()
            .map(|p| Part { name: p.name, size: p.size, mod_time: parse_rfc3339_opt(&p.mod_time) })
            .collect();
        local_parts.sort_by(|a, b| a.name.cmp(&b.name));

        if remote_parts.is_empty() || remote_parts.len() != local_parts.len() {
            return Ok(PreDownloadDecision::Proceed);
        }

        // Names+sizes must match exactly
        for (a, b) in remote_parts.iter().zip(local_parts.iter()) {
            if a.name != b.name || a.size != b.size {
                return Ok(PreDownloadDecision::Proceed);
            }
        }
        // If modtimes exist on both sides, treat remote newer as a signal to re-download.
        let remote_is_newer = remote_parts.iter().zip(local_parts.iter()).any(|(a, b)| {
            match (a.mod_time, b.mod_time) {
                (Some(ra), Some(lb)) => ra > lb,
                _ => false,
            }
        });
        if remote_is_newer {
            return Ok(PreDownloadDecision::Proceed);
        }
        Ok(PreDownloadDecision::SkipAlreadyPresent)
    }

    #[instrument(
        name = "repo.post_download",
        skip(http_client, cancellation_token),
        fields(layout = %self.id(), app = %app_full_name)
    )]
    async fn post_download(
        &self,
        app_full_name: &str,
        dst_dir: &Path,
        http_client: &reqwest::Client,
        cache_dir: &Path,
        status_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        if let Ok(state) = self.ensure_initialized(http_client, cache_dir).await {
            // The downloaded dir should contain <hash>.7z.001 as first segment.
            let hash = VRPPublicRepo::hash_for_release(app_full_name);
            let first_part = dst_dir.join(format!("{}.7z.001", hash));
            if !first_part.is_file() {
                return Ok(());
            }

            // Collect multipart info for a later stamp before deleting
            let mut parts: Vec<(String, u64, String)> = Vec::new();
            if let Ok(mut rd) = fs::read_dir(dst_dir).await {
                while let Ok(Some(entry)) = rd.next_entry().await {
                    if let Some(name) = entry.file_name().to_str()
                        && name.starts_with(&format!("{hash}.7z."))
                        && let Ok(meta) = entry.metadata().await
                    {
                        let odt: OffsetDateTime = meta
                            .modified()
                            .with_context(|| {
                                format!(
                                    "Failed to read modification time for {}",
                                    entry.path().display()
                                )
                            })?
                            .into();
                        parts.push((name.to_string(), meta.len(), odt.format(&Rfc3339).unwrap()));
                    }
                }
            }

            if let Some(tx) = &status_tx {
                let _ = tx.send("Extracting files...".into());
            }
            if let Err(e) = decompress_archive(
                &first_part,
                dst_dir,
                Some(&state.password),
                None,
                Some(cancellation_token.clone()),
            )
            .await
            {
                error!(
                    error = e.as_ref() as &dyn Error,
                    dir = %dst_dir.display(),
                    first = %first_part.display(),
                    "VRP-public extraction failed"
                );
                return Err(e.context("VRP-public extraction failed"));
            }
            // If archive created a nested folder with the same full name, flatten it
            if let Some(tx) = &status_tx {
                let _ = tx.send("Finalizing files...".into());
            }
            let nested = dst_dir.join(app_full_name);
            if nested.is_dir() && nested != dst_dir {
                if let Ok(mut rd) = fs::read_dir(&nested).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let from = entry.path();
                        let to = dst_dir.join(entry.file_name());
                        let _ = fs::rename(&from, &to).await;
                    }
                }
                let _ = fs::remove_dir_all(&nested).await;
            }
            // Write a VRP extraction stamp so we can skip future re-downloads for the same app
            #[derive(serde::Serialize)]
            struct StampPart {
                name: String,
                size: u64,
                mod_time: String,
            }
            #[derive(serde::Serialize)]
            struct Stamp {
                hash: String,
                parts: Vec<StampPart>,
            }
            if !parts.is_empty() {
                let stamp = Stamp {
                    hash: hash.clone(),
                    parts: parts
                        .into_iter()
                        .map(|(n, s, m)| StampPart { name: n, size: s, mod_time: m })
                        .collect(),
                };
                if let Ok(json) = serde_json::to_string_pretty(&stamp) {
                    let _ = fs::write(dst_dir.join("vrp_stamp.json"), json).await;
                }
            }
            // Remove multipart .7z parts after successful extraction
            if let Some(tx) = &status_tx {
                let _ = tx.send("Cleaning up...".into());
            }
            if let Ok(mut rd) = fs::read_dir(dst_dir).await {
                while let Ok(Some(entry)) = rd.next_entry().await {
                    if let Some(name) = entry.file_name().to_str()
                        && name.starts_with(&format!("{hash}.7z."))
                    {
                        let _ = fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
        Ok(())
    }

    fn generated_config_filename(&self) -> Option<&'static str> {
        Some("rclone.vrp.conf")
    }
}
