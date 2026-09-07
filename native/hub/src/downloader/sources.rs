use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, ensure};
use tracing::{debug, info, warn};

use crate::{
    downloader::{SensitiveUrl, config::DownloaderConfig, http_cache},
    models::{InstalledDownloaderConfig, Settings},
    settings::SettingsHandler,
};

pub(crate) const LEGACY_CONFIG_FILENAME: &str = "downloader.json";
const MANAGED_CONFIGS_DIR: &str = "downloader_configs";

#[derive(Debug, Clone, Default)]
pub(crate) struct SourceSnapshot {
    pub(crate) configs: Vec<DownloaderConfig>,
    pub(crate) installed_configs: Vec<InstalledDownloaderConfig>,
    pub(crate) active_config_id: Option<String>,
}

impl SourceSnapshot {
    pub(crate) fn active_config(&self) -> Option<DownloaderConfig> {
        let active_config_id = self.active_config_id.as_deref()?;
        self.configs.iter().find(|cfg| cfg.id == active_config_id).cloned()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RefreshReport {
    pub(crate) refreshed: usize,
    pub(crate) failed: Vec<String>,
}

impl RefreshReport {
    pub(crate) fn warning_message(&self) -> Option<String> {
        if self.failed.is_empty() {
            None
        } else {
            Some(format!("Failed to refresh some downloader sources: {}", self.failed.join("; ")))
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceStore {
    app_dir: PathBuf,
    settings_handler: Arc<SettingsHandler>,
}

impl SourceStore {
    pub(crate) fn new(app_dir: PathBuf, settings_handler: Arc<SettingsHandler>) -> Self {
        Self { app_dir, settings_handler }
    }

    pub(crate) fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    pub(crate) fn load(&self) -> Result<SourceSnapshot> {
        let loaded = read_configs(&self.app_dir)?;
        let active_config_id = resolve_active_config_id(
            &loaded.configs,
            current_active_config_id(&self.settings_handler),
        );

        Ok(SourceSnapshot {
            configs: loaded.configs,
            installed_configs: loaded.installed_configs,
            active_config_id,
        })
    }

    pub(crate) fn persist_active_config(&self, sources: &SourceSnapshot) -> Result<()> {
        let current_active_id = current_active_config_id(&self.settings_handler);
        if sources.active_config_id.as_deref() == Some(current_active_id.as_str()) {
            return Ok(());
        }

        save_active_config_id(&self.settings_handler, sources.active_config_id.as_deref())
    }

    pub(crate) fn select_active(&self, config_id: &str) -> Result<()> {
        ensure!(!config_id.is_empty(), "Downloader config ID must not be empty");

        let loaded = read_configs(&self.app_dir)?;
        ensure!(
            loaded.configs.iter().any(|cfg| cfg.id == config_id),
            "Downloader config is not installed: {config_id}"
        );

        save_active_config_id(&self.settings_handler, Some(config_id))
    }

    pub(crate) fn remove(&self, config_id: &str) -> Result<()> {
        ensure!(!config_id.is_empty(), "Downloader config ID must not be empty");
        ensure!(
            !config_id.contains('/')
                && !config_id.contains('\\')
                && config_id != "."
                && config_id != "..",
            "Downloader config ID must be a safe file name"
        );

        let path = managed_config_path(&self.app_dir, config_id);
        ensure!(path.exists(), "Downloader config is not installed: {config_id}");

        fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))
    }

    pub(crate) fn inactive_configs(&self, sources: &SourceSnapshot) -> Vec<DownloaderConfig> {
        let active_id = sources.active_config_id.as_deref();
        sources.configs.iter().filter(|cfg| Some(cfg.id.as_str()) != active_id).cloned().collect()
    }

    pub(crate) async fn migrate_legacy_config_if_needed(&self) -> Option<anyhow::Error> {
        migrate_legacy_config_if_needed(&self.app_dir, &self.settings_handler).await
    }

    pub(crate) fn delete_cache_dir(&self, config_id: &str) -> Result<()> {
        delete_config_cache_dir(&self.app_dir, config_id)
    }
}

/// Files owned by one configuration fetch until the controller accepts it.
pub(super) struct ConfigFetch {
    temp: tempfile::TempDir,
    url: String,
    expected_id: Option<String>,
}

impl ConfigFetch {
    /// Called under the controller lock to snapshot the configuration cache.
    pub(super) fn new(app_dir: &Path, url: &str, expected_id: Option<&str>) -> Result<Self> {
        let temp = tempfile::tempdir()?;
        if let Some(id) = expected_id {
            let cache = runtime_cache_dir(app_dir, id).join("source");
            for name in ["downloader_config.json", "meta.json"] {
                let path = cache.join(name);
                if path.exists() {
                    fs::copy(path, temp.path().join(name))?;
                }
            }
        }
        Ok(Self { temp, url: url.to_string(), expected_id: expected_id.map(str::to_string) })
    }

    pub(super) async fn fetch(self) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        http_cache::update_file_cached(
            &client,
            &self.url,
            &self.temp.path().join("downloader_config.json"),
            self.temp.path(),
            None,
        )
        .await?;
        let cfg =
            DownloaderConfig::load_from_path(self.temp.path().join("downloader_config.json"))?;
        cfg.validate_managed_remote(
            self.expected_id.is_none().then(|| SensitiveUrl::new(&self.url)),
        )?;
        if let Some(id) = &self.expected_id {
            ensure!(
                cfg.id == *id,
                "Downloaded downloader config changed ID: expected {id}, got {}",
                cfg.id
            );
        }
        Ok(self)
    }

    /// Commits only after the controller has checked the source revision.
    pub(super) fn commit(self, app_dir: &Path) -> Result<DownloaderConfig> {
        let cfg = write_managed_config(
            app_dir,
            &self.temp.path().join("downloader_config.json"),
            self.expected_id.is_none().then(|| SensitiveUrl::new(&self.url)),
            self.expected_id.as_deref(),
            self.expected_id.is_none(),
        )?;
        let cache = runtime_cache_dir(app_dir, &cfg.id).join("source");
        // A cache failure must not turn a successful source mutation into a failed install.
        if let Err(error) = (|| -> Result<()> {
            fs::create_dir_all(&cache)?;
            for name in ["downloader_config.json", "meta.json"] {
                let path = self.temp.path().join(name);
                if path.exists() {
                    fs::copy(path, cache.join(name))?;
                }
            }
            Ok(())
        })() {
            warn!(%error, "Failed to save source download cache");
        }
        Ok(cfg)
    }
}

pub(super) struct CacheLease(pub Arc<tokio::sync::Notify>);

impl Drop for CacheLease {
    fn drop(&mut self) {
        self.0.notify_waiters();
    }
}

struct ReadConfigs {
    configs: Vec<DownloaderConfig>,
    installed_configs: Vec<InstalledDownloaderConfig>,
}

pub(crate) fn managed_configs_dir(app_dir: &Path) -> PathBuf {
    app_dir.join(MANAGED_CONFIGS_DIR)
}

pub(crate) fn managed_config_path(app_dir: &Path, config_id: &str) -> PathBuf {
    managed_configs_dir(app_dir).join(format!("{config_id}.json"))
}

pub(crate) fn runtime_cache_dir(app_dir: &Path, config_id: &str) -> PathBuf {
    app_dir.join("downloader_cache").join(config_id)
}

fn config_download_cache_path(app_dir: &Path, cache_key: &str) -> (PathBuf, PathBuf) {
    let cache_dir = runtime_cache_dir(app_dir, cache_key);
    let cached_cfg_path = cache_dir.join("downloader_config.json");
    (cache_dir, cached_cfg_path)
}

fn current_settings(settings_handler: &Arc<SettingsHandler>) -> Settings {
    let rx = settings_handler.subscribe();
    rx.borrow().clone()
}

pub(crate) fn current_active_config_id(settings_handler: &Arc<SettingsHandler>) -> String {
    current_settings(settings_handler).active_downloader_config_id.trim().to_string()
}

fn save_active_config_id(
    settings_handler: &Arc<SettingsHandler>,
    config_id: Option<&str>,
) -> Result<()> {
    settings_handler.update_active_downloader(config_id.unwrap_or_default())
}

fn resolve_active_config_id(configs: &[DownloaderConfig], desired_id: String) -> Option<String> {
    if !desired_id.is_empty() && configs.iter().any(|cfg| cfg.id == desired_id) {
        return Some(desired_id);
    }

    configs.iter().min_by(|left, right| left.id.cmp(&right.id)).map(|cfg| cfg.id.clone())
}

pub(crate) fn warnings_to_message(warnings: &[String]) -> Option<String> {
    if warnings.is_empty() { None } else { Some(warnings.join("\n")) }
}

fn is_http_url(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.starts_with("http://") || v.starts_with("https://")
}

fn read_configs(app_dir: &Path) -> Result<ReadConfigs> {
    let dir = managed_configs_dir(app_dir);
    if !dir.exists() {
        return Ok(ReadConfigs { configs: Vec::new(), installed_configs: Vec::new() });
    }

    let mut configs = Vec::new();
    let mut installed_configs = Vec::new();

    for entry in fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("Failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if entry.file_type()?.is_dir() {
            continue;
        }
        let id = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();

        match DownloaderConfig::load_from_path(&path).and_then(|cfg| {
            cfg.validate_managed_remote(None)?;
            ensure!(cfg.id == id, "Downloader config ID does not match its file name: {}", cfg.id);
            Ok(cfg)
        }) {
            Ok(cfg) => {
                installed_configs.push(InstalledDownloaderConfig {
                    id: cfg.id.clone(),
                    display_name: cfg.effective_display_name(),
                    description: cfg.effective_description(),
                    error: None,
                });
                configs.push(cfg);
            }
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    path = %path.display(),
                    "Invalid managed downloader config"
                );
                let metadata = fs::read(&path)
                    .ok()
                    .and_then(|content| serde_json::from_slice::<serde_json::Value>(&content).ok());
                let display_name = metadata
                    .as_ref()
                    .and_then(|value| value.get("display_name"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        path.file_name().unwrap_or_default().to_string_lossy().into_owned()
                    });
                installed_configs.push(InstalledDownloaderConfig {
                    id,
                    display_name,
                    description: String::new(),
                    error: Some(format!("{e:#}")),
                });
            }
        }
    }

    configs.sort_by(|left, right| left.id.cmp(&right.id));
    installed_configs.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(ReadConfigs { configs, installed_configs })
}

async fn cache_config_from_url(
    app_dir: &Path,
    cache_key: &str,
    url: SensitiveUrl<'_>,
) -> Result<PathBuf> {
    ensure!(is_http_url(url.as_str()), "Config update URL must start with http:// or https://");
    debug!(
        update_url = %url,
        cache_key = %cache_key,
        "Downloading downloader config from URL"
    );

    let (cache_dir, cached_cfg_path) = config_download_cache_path(app_dir, cache_key);

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .context("Failed to build HTTP client for downloader config update")?;

    let _ =
        http_cache::update_file_cached(&client, url.as_str(), &cached_cfg_path, &cache_dir, None)
            .await
            .with_context(|| format!("Failed to download downloader config from {url}"))?;

    Ok(cached_cfg_path)
}

async fn fetch_managed_config(
    app_dir: &Path,
    cache_key: &str,
    url: SensitiveUrl<'_>,
    source_url: Option<SensitiveUrl<'_>>,
    expected_id: Option<&str>,
    refuse_existing: bool,
) -> Result<DownloaderConfig> {
    let remote_cfg_path = cache_config_from_url(app_dir, cache_key, url).await?;
    write_managed_config(app_dir, &remote_cfg_path, source_url, expected_id, refuse_existing)
}

fn write_managed_config(
    app_dir: &Path,
    src: &Path,
    source_url: Option<SensitiveUrl<'_>>,
    expected_id: Option<&str>,
    refuse_existing: bool,
) -> Result<DownloaderConfig> {
    let cfg = DownloaderConfig::load_from_path(src)?;
    cfg.validate_managed_remote(source_url)?;
    if let Some(expected_id) = expected_id {
        ensure!(
            cfg.id == expected_id,
            "Downloaded downloader config changed ID: expected {expected_id}, got {}",
            cfg.id
        );
    }

    let dst_dir = managed_configs_dir(app_dir);
    fs::create_dir_all(&dst_dir)
        .with_context(|| format!("Failed to create {}", dst_dir.display()))?;

    let dst = managed_config_path(app_dir, &cfg.id);
    if refuse_existing {
        ensure!(!dst.exists(), "Downloader config ID already installed: {}", cfg.id);
    }

    let tmp = dst_dir.join(format!("{}.json.tmp", cfg.id));
    let content =
        fs::read_to_string(src).with_context(|| format!("Failed to read {}", src.display()))?;
    fs::write(&tmp, content).with_context(|| format!("Failed to write {}", tmp.display()))?;
    fs::rename(&tmp, &dst).with_context(|| format!("Failed to replace {}", dst.display()))?;

    Ok(cfg)
}

#[cfg(test)]
async fn refresh_configs(app_dir: &Path, configs: &[DownloaderConfig]) -> RefreshReport {
    let mut report = RefreshReport::default();

    for cfg in configs {
        let Some(update_url) = cfg.config_update_url.as_deref().map(str::trim) else {
            report.failed.push(format!("{}: missing config_update_url", cfg.id));
            continue;
        };

        let refresh_result = async {
            let update_url = SensitiveUrl::new(update_url);
            let _ = fetch_managed_config(app_dir, &cfg.id, update_url, None, Some(&cfg.id), false)
                .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match refresh_result {
            Ok(()) => report.refreshed += 1,
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    config_id = %cfg.id,
                    "Failed to refresh downloader config"
                );
                report.failed.push(format!("{}: {:#}", cfg.id, e));
            }
        }
    }

    report
}

fn delete_config_cache_dir(app_dir: &Path, config_id: &str) -> Result<()> {
    let cache_dir = runtime_cache_dir(app_dir, config_id);
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir)
            .with_context(|| format!("Failed to delete {}", cache_dir.display()))?;
    }

    Ok(())
}

async fn migrate_legacy_config_if_needed(
    app_dir: &Path,
    settings_handler: &Arc<SettingsHandler>,
) -> Option<anyhow::Error> {
    let legacy_path = app_dir.join(LEGACY_CONFIG_FILENAME);
    if !legacy_path.exists() {
        return None;
    }

    info!(path = %legacy_path.display(), "Migrating legacy downloader config");

    let migration_result = async {
        let legacy_cfg = DownloaderConfig::load_from_path(&legacy_path)?;
        let update_url = legacy_cfg
            .config_update_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("Legacy downloader config has no usable config_update_url")?;

        let select_as_active = current_active_config_id(settings_handler).is_empty();

        if managed_config_path(app_dir, &legacy_cfg.id).exists() {
            if select_as_active {
                save_active_config_id(settings_handler, Some(&legacy_cfg.id))?;
            }
            return Ok::<(), anyhow::Error>(());
        }

        let update_url = SensitiveUrl::new(update_url);
        let _ =
            fetch_managed_config(app_dir, "_bootstrap", update_url, Some(update_url), None, true)
                .await?;
        if select_as_active {
            save_active_config_id(settings_handler, Some(&legacy_cfg.id))?;
        }
        Ok(())
    }
    .await;

    match migration_result {
        Ok(()) => {
            if let Err(e) = fs::remove_file(&legacy_path) {
                warn!(
                    error = &e as &dyn Error,
                    path = %legacy_path.display(),
                    "Failed to delete legacy downloader config"
                );
                return Some(anyhow!("Failed to delete legacy downloader config: {e}"));
            }
            None
        }
        Err(e) => Some(e),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::downloader::config::RepoLayoutKind;

    fn managed_config_json(id: &str, update_url: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "display_name": "Display {id}",
                "description": "Description {id}",
                "layout": "ffa",
                "rclone_path": "/bin/echo",
                "rclone_config_path": "/tmp/rclone.conf",
                "config_update_url": "{update_url}"
            }}"#
        )
    }

    fn legacy_config_json_without_update_url(id: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "layout": "ffa",
                "rclone_path": "/bin/echo",
                "rclone_config_path": "/tmp/rclone.conf"
            }}"#
        )
    }

    #[test]
    fn write_managed_config_requires_matching_update_url() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, managed_config_json("test", "https://example.com/downloader.json"))
            .unwrap();

        let err = write_managed_config(
            dir.path(),
            &src,
            Some(SensitiveUrl::new("https://other.example/config.json")),
            None,
            true,
        )
        .unwrap_err();
        assert!(format!("{:#}", err).contains("Config update URL mismatch"));
    }

    #[test]
    fn write_managed_config_rejects_duplicate_id() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, managed_config_json("test", "https://example.com/downloader.json"))
            .unwrap();

        let first = write_managed_config(
            dir.path(),
            &src,
            Some(SensitiveUrl::new("https://example.com/downloader.json")),
            None,
            true,
        )
        .unwrap();
        assert_eq!(first.id, "test");

        let err = write_managed_config(
            dir.path(),
            &src,
            Some(SensitiveUrl::new("https://example.com/downloader.json")),
            None,
            true,
        )
        .unwrap_err();
        assert!(format!("{:#}", err).contains("Downloader config ID already installed"));
    }

    #[tokio::test]
    async fn invalid_sources_can_be_removed_and_reinstalled() {
        for (id, content, name, error) in [
            ("broken", "{ invalid json".to_string(), "broken.json", "Failed to parse"),
            (
                "missing-id",
                r#"{"display_name": "Missing ID"}"#.to_string(),
                "Missing ID",
                "missing field",
            ),
            (
                "invalid",
                legacy_config_json_without_update_url("invalid"),
                "invalid.json",
                "config_update_url is required",
            ),
            (
                "stored",
                managed_config_json("healthy", "https://example.com/healthy.json"),
                "Display healthy",
                "ID does not match its file name",
            ),
        ] {
            let dir = tempdir().unwrap();
            let app_dir = dir.path().to_path_buf();
            let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
            let sources = SourceStore::new(app_dir.clone(), settings.clone());
            fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
            let healthy = managed_config_path(&app_dir, "healthy");
            fs::write(&healthy, managed_config_json("healthy", "https://example.com/healthy.json"))
                .unwrap();
            let invalid = managed_config_path(&app_dir, id);
            fs::write(&invalid, content).unwrap();
            save_active_config_id(&settings, Some(id)).unwrap();

            let snapshot = sources.load().unwrap();
            assert_eq!(snapshot.configs.len(), 1);
            assert_eq!(snapshot.active_config_id.as_deref(), Some("healthy"));
            assert!(sources.inactive_configs(&snapshot).is_empty());
            assert_eq!(snapshot.installed_configs.len(), 2);
            let entry = snapshot.installed_configs.iter().find(|cfg| cfg.id == id).unwrap();
            assert_eq!(entry.display_name, name);
            assert!(entry.error.as_deref().unwrap().contains(error));
            assert!(sources.select_active(id).is_err());

            let src = app_dir.join("replacement.json");
            fs::write(&src, managed_config_json(id, "https://example.com/config.json")).unwrap();
            assert!(write_managed_config(&app_dir, &src, None, None, true).is_err());

            sources.remove(&entry.id).unwrap();
            assert!(!invalid.exists());
            assert!(healthy.exists());
            let remaining = sources.load().unwrap();
            assert_eq!(remaining.installed_configs.len(), 1);
            assert!(remaining.installed_configs[0].error.is_none());

            write_managed_config(&app_dir, &src, None, None, true).unwrap();
            let reinstalled = sources.load().unwrap();
            assert_eq!(reinstalled.configs.len(), 2);
            assert!(reinstalled.installed_configs.iter().all(|cfg| cfg.error.is_none()));
            sources.select_active(id).unwrap();
            assert_eq!(sources.load().unwrap().active_config_id.as_deref(), Some(id));
        }
    }

    #[tokio::test]
    async fn removing_only_invalid_source_leaves_no_sources() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        let sources = SourceStore::new(app_dir.clone(), settings.clone());
        fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        fs::write(managed_config_path(&app_dir, "broken"), "invalid json").unwrap();
        save_active_config_id(&settings, Some("broken")).unwrap();

        let snapshot = sources.load().unwrap();
        assert_eq!(snapshot.installed_configs.len(), 1);
        assert!(snapshot.active_config().is_none());
        sources.persist_active_config(&snapshot).unwrap();
        assert!(current_active_config_id(&settings).is_empty());

        sources.remove("broken").unwrap();
        let snapshot = sources.load().unwrap();
        assert!(snapshot.installed_configs.is_empty());
        assert!(snapshot.active_config().is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_all_configs_allows_update_url_change() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path();
        let managed_dir = managed_configs_dir(app_dir);
        std::fs::create_dir_all(&managed_dir).unwrap();

        let server = MockServer::start().await;
        let original_url = format!("{}/downloader.json", server.uri());
        let installed_path = managed_config_path(app_dir, "test");
        std::fs::write(&installed_path, managed_config_json("test", &original_url)).unwrap();

        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(managed_config_json(
                "test",
                "https://other.example/downloader.json",
            )))
            .mount(&server)
            .await;

        let cfg = DownloaderConfig::load_from_path(&installed_path).expect("load installed config");
        let report = refresh_configs(app_dir, &[cfg]).await;

        assert_eq!(report.refreshed, 1);
        assert!(report.failed.is_empty());
        let installed = std::fs::read_to_string(&installed_path).unwrap();
        assert!(installed.contains("https://other.example/downloader.json"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn migrate_legacy_config_imports_remote_and_deletes_file() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();

        let server = MockServer::start().await;
        let url = format!("{}/downloader.json", server.uri());
        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(managed_config_json("legacy", &url)),
            )
            .mount(&server)
            .await;

        let legacy_path = app_dir.join(LEGACY_CONFIG_FILENAME);
        std::fs::write(&legacy_path, managed_config_json("legacy", &url)).unwrap();

        let warning = migrate_legacy_config_if_needed(&app_dir, &settings).await;
        assert!(warning.is_none());
        assert!(!legacy_path.exists());
        assert!(managed_config_path(&app_dir, "legacy").exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn migrate_legacy_without_update_url_keeps_file() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        let legacy_path = app_dir.join(LEGACY_CONFIG_FILENAME);
        std::fs::write(&legacy_path, legacy_config_json_without_update_url("legacy")).unwrap();

        let warning = migrate_legacy_config_if_needed(&app_dir, &settings).await;
        assert!(warning.is_some());
        assert!(legacy_path.exists());
        assert!(!managed_config_path(&app_dir, "legacy").exists());
    }

    #[test]
    fn resolve_active_config_id_falls_back_to_first_sorted_config() {
        let configs = vec![
            DownloaderConfig {
                id: "b".into(),
                display_name: None,
                description: None,
                rclone_path: Some(crate::downloader::config::RclonePath::Single(
                    "/bin/echo".into(),
                )),
                rclone_config_path: Some("/tmp/rclone.conf".into()),
                remote_name_filter_regex: None,
                disable_randomize_remote: false,
                donation_remote_name: None,
                donation_remote_path: None,
                donation_blacklist_path: None,
                layout: RepoLayoutKind::Ffa,
                base_url: None,
                root_dir: "Quest Games".into(),
                list_path: "FFA.txt".into(),
                config_update_url: Some("https://example.com/b.json".into()),
            },
            DownloaderConfig {
                id: "a".into(),
                display_name: None,
                description: None,
                rclone_path: Some(crate::downloader::config::RclonePath::Single(
                    "/bin/echo".into(),
                )),
                rclone_config_path: Some("/tmp/rclone.conf".into()),
                remote_name_filter_regex: None,
                disable_randomize_remote: false,
                donation_remote_name: None,
                donation_remote_path: None,
                donation_blacklist_path: None,
                layout: RepoLayoutKind::Ffa,
                base_url: None,
                root_dir: "Quest Games".into(),
                list_path: "FFA.txt".into(),
                config_update_url: Some("https://example.com/a.json".into()),
            },
        ];

        assert_eq!(resolve_active_config_id(&configs, String::new()).as_deref(), Some("a"));
        assert_eq!(resolve_active_config_id(&configs, "missing".into()).as_deref(), Some("a"));
        assert_eq!(resolve_active_config_id(&configs, "a".into()).as_deref(), Some("a"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_reassigns_active_source() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        let sources = SourceStore::new(app_dir.clone(), settings.clone());

        let alpha = managed_config_path(&app_dir, "alpha");
        let beta = managed_config_path(&app_dir, "beta");
        std::fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        std::fs::write(&alpha, managed_config_json("alpha", "https://example.com/alpha.json"))
            .unwrap();
        std::fs::write(&beta, managed_config_json("beta", "https://example.com/beta.json"))
            .unwrap();

        save_active_config_id(&settings, Some("beta")).unwrap();

        sources.remove("beta").unwrap();
        sources.persist_active_config(&sources.load().unwrap()).unwrap();

        assert!(!beta.exists());
        assert!(alpha.exists());
        assert_eq!(current_active_config_id(&settings), "alpha");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_clears_active_source_when_last_removed() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        let sources = SourceStore::new(app_dir.clone(), settings.clone());

        let only = managed_config_path(&app_dir, "only");
        std::fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        std::fs::write(&only, managed_config_json("only", "https://example.com/only.json"))
            .unwrap();

        save_active_config_id(&settings, Some("only")).unwrap();

        sources.remove("only").unwrap();
        sources.persist_active_config(&sources.load().unwrap()).unwrap();

        assert!(!only.exists());
        assert_eq!(current_active_config_id(&settings), "");
    }

    #[test]
    fn delete_config_cache_dir_removes_runtime_directory() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let cache_dir = runtime_cache_dir(&app_dir, "test");

        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("downloader_config.json"), "cached").unwrap();

        delete_config_cache_dir(&app_dir, "test").unwrap();

        assert!(!cache_dir.exists());
    }
}
