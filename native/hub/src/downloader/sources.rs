use std::{error::Error, fs, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, ensure};
use tracing::{debug, info, warn};

use crate::{
    downloader::{
        SensitiveUrl,
        config::{DownloaderConfig, is_safe_id},
        is_http_url,
    },
    models::InstalledDownloaderConfig,
    settings::SettingsHandler,
};

const LEGACY_CONFIG_FILENAME: &str = "downloader.json";
const MANAGED_CONFIGS_DIR: &str = "downloader_configs";
const RUNTIME_CACHES_DIR: &str = "downloader_cache";

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

    pub(crate) fn inactive_configs(&self) -> Vec<DownloaderConfig> {
        let active_id = self.active_config_id.as_deref();
        self.configs.iter().filter(|cfg| Some(cfg.id.as_str()) != active_id).cloned().collect()
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

    pub(super) fn managed_configs_dir(&self) -> PathBuf {
        self.app_dir.join(MANAGED_CONFIGS_DIR)
    }

    pub(super) fn managed_config_path(&self, config_id: &str) -> PathBuf {
        self.managed_configs_dir().join(format!("{config_id}.json"))
    }

    pub(super) fn runtime_cache_dir(&self, config_id: &str) -> PathBuf {
        self.app_dir.join(RUNTIME_CACHES_DIR).join(config_id)
    }

    pub(crate) fn load(&self) -> Result<SourceSnapshot> {
        let loaded = self.read_configs()?;
        let active_config_id =
            resolve_active_config_id(&loaded.configs, self.current_active_config_id());

        Ok(SourceSnapshot {
            configs: loaded.configs,
            installed_configs: loaded.installed_configs,
            active_config_id,
        })
    }

    pub(crate) fn persist_active_config(&self, sources: &SourceSnapshot) -> Result<()> {
        self.settings_handler
            .update_active_downloader(sources.active_config_id.as_deref().unwrap_or_default())
    }

    pub(crate) fn select_active(&self, config_id: &str) -> Result<()> {
        ensure!(!config_id.is_empty(), "Downloader config ID must not be empty");

        let loaded = self.read_configs()?;
        ensure!(
            loaded.configs.iter().any(|cfg| cfg.id == config_id),
            "Downloader config is not installed: {config_id}"
        );

        self.settings_handler.update_active_downloader(config_id)
    }

    /// Writes a fetched configuration to the managed configs directory.
    ///
    /// Called by the controller only after it has checked the source revision.
    pub(super) fn commit(&self, fetched: FetchedConfig) -> Result<DownloaderConfig> {
        let dst_dir = self.managed_configs_dir();
        fs::create_dir_all(&dst_dir)
            .with_context(|| format!("Failed to create {}", dst_dir.display()))?;

        let dst = self.managed_config_path(&fetched.config.id);
        if fetched.is_install {
            ensure!(!dst.exists(), "Downloader config ID already installed: {}", fetched.config.id);
        }

        let tmp = dst_dir.join(format!("{}.json.tmp", fetched.config.id));
        fs::write(&tmp, &fetched.content)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &dst).with_context(|| format!("Failed to replace {}", dst.display()))?;

        Ok(fetched.config)
    }

    pub(crate) fn remove(&self, config_id: &str) -> Result<()> {
        ensure!(!config_id.is_empty(), "Downloader config ID must not be empty");
        ensure!(is_safe_id(config_id), "Downloader config ID must be a safe file name");

        let path = self.managed_config_path(config_id);
        ensure!(path.exists(), "Downloader config is not installed: {config_id}");

        fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))
    }

    pub(crate) async fn migrate_legacy_config_if_needed(&self) -> Option<anyhow::Error> {
        let legacy_path = self.app_dir.join(LEGACY_CONFIG_FILENAME);
        if !legacy_path.exists() {
            return None;
        }

        info!(path = %legacy_path.display(), "Migrating legacy downloader config");

        let migration_result = async {
            let legacy_cfg = DownloaderConfig::load_from_path(&legacy_path)?;
            let select_as_active = self.current_active_config_id().is_empty();

            let id = if self.managed_config_path(&legacy_cfg.id).exists() {
                legacy_cfg.id
            } else {
                let url = SensitiveUrl::new(&legacy_cfg.config_update_url);
                self.commit(fetch_config(url, FetchMode::Install).await?)?.id
            };
            if select_as_active {
                self.settings_handler.update_active_downloader(&id)?;
            }
            Ok::<(), anyhow::Error>(())
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

    /// Deletes runtime caches of sources that are no longer installed.
    ///
    /// Must run before any session starts, so that no session is using a deleted cache.
    pub(crate) fn remove_orphaned_caches(&self) -> Result<()> {
        let root = self.app_dir.join(RUNTIME_CACHES_DIR);
        if !root.exists() {
            return Ok(());
        }
        let installed = self.read_configs()?.installed_configs;

        for entry in
            fs::read_dir(&root).with_context(|| format!("Failed to read {}", root.display()))?
        {
            let entry =
                entry.with_context(|| format!("Failed to read entry in {}", root.display()))?;
            if !entry.file_type()?.is_dir()
                || installed.iter().any(|cfg| entry.file_name() == cfg.id.as_str())
            {
                continue;
            }
            let path = entry.path();
            info!(path = %path.display(), "Deleting orphaned downloader cache");
            if let Err(e) = fs::remove_dir_all(&path) {
                warn!(
                    error = &e as &dyn Error,
                    path = %path.display(),
                    "Failed to delete orphaned downloader cache"
                );
            }
        }

        Ok(())
    }

    fn current_active_config_id(&self) -> String {
        self.settings_handler.subscribe().borrow().active_downloader_config_id.trim().to_string()
    }

    fn read_configs(&self) -> Result<ReadConfigs> {
        let dir = self.managed_configs_dir();
        if !dir.exists() {
            return Ok(ReadConfigs { configs: Vec::new(), installed_configs: Vec::new() });
        }

        let mut configs = Vec::new();
        let mut installed_configs = Vec::new();

        for entry in
            fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))?
        {
            let entry =
                entry.with_context(|| format!("Failed to read entry in {}", dir.display()))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            if entry.file_type()?.is_dir() {
                continue;
            }
            let id = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();

            match DownloaderConfig::load_from_path(&path).and_then(|cfg| {
                ensure!(
                    cfg.id == id,
                    "Downloader config ID does not match its file name: {}",
                    cfg.id
                );
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
                    let metadata = fs::read(&path).ok().and_then(|content| {
                        serde_json::from_slice::<serde_json::Value>(&content).ok()
                    });
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
}

#[derive(Debug, Clone, Copy)]
pub(super) enum FetchMode<'a> {
    /// Installs a new source; its `config_update_url` must match the fetched URL.
    Install,
    /// Refreshes an installed source; the downloaded ID must match this ID.
    Refresh(&'a str),
}

/// A downloaded and validated configuration that has not been installed yet.
pub(super) struct FetchedConfig {
    config: DownloaderConfig,
    content: String,
    is_install: bool,
}

/// Downloads and validates a managed configuration.
pub(super) async fn fetch_config(
    url: SensitiveUrl<'_>,
    mode: FetchMode<'_>,
) -> Result<FetchedConfig> {
    ensure!(is_http_url(url.as_str()), "Config URL must start with http:// or https://");
    debug!(%url, ?mode, "Downloading downloader config");

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .context("Failed to build HTTP client for downloader config")?;
    let content = async { client.get(url.as_str()).send().await?.error_for_status()?.text().await }
        .await
        .map_err(reqwest::Error::without_url)
        .with_context(|| format!("Failed to download downloader config from {url}"))?;

    let config = DownloaderConfig::parse(&content)?;
    match mode {
        FetchMode::Install => ensure!(
            config.config_update_url == url.as_str(),
            "Config update URL mismatch: expected {url}, got {}",
            SensitiveUrl::new(&config.config_update_url)
        ),
        FetchMode::Refresh(expected_id) => ensure!(
            config.id == expected_id,
            "Downloaded downloader config changed ID: expected {expected_id}, got {}",
            config.id
        ),
    }

    Ok(FetchedConfig { config, content, is_install: matches!(mode, FetchMode::Install) })
}

struct ReadConfigs {
    configs: Vec<DownloaderConfig>,
    installed_configs: Vec<InstalledDownloaderConfig>,
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    fn test_store() -> (tempfile::TempDir, SourceStore) {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        (dir, SourceStore::new(app_dir, settings))
    }

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

    fn fetched_install(content: String) -> FetchedConfig {
        FetchedConfig {
            config: DownloaderConfig::parse(&content).unwrap(),
            content,
            is_install: true,
        }
    }

    #[tokio::test]
    async fn fetch_config_install_requires_matching_update_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(managed_config_json(
                "test",
                "https://other.example/config.json?downloaded-secret",
            )))
            .mount(&server)
            .await;

        let url = format!("{}/downloader.json?entered-secret", server.uri());
        let err = fetch_config(SensitiveUrl::new(&url), FetchMode::Install).await.err().unwrap();
        let message = format!("{err:#}");
        assert!(message.contains("Config update URL mismatch"));
        assert!(message.contains("https://other.example/config.json?redacted"));
        assert!(!message.contains("secret"));
    }

    #[tokio::test]
    async fn install_rejects_duplicate_id() {
        let (_dir, sources) = test_store();
        let content = managed_config_json("test", "https://example.com/downloader.json");

        let first = sources.commit(fetched_install(content.clone())).unwrap();
        assert_eq!(first.id, "test");

        let err = sources.commit(fetched_install(content)).unwrap_err();
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
                "missing field `config_update_url`",
            ),
            (
                "stored",
                managed_config_json("healthy", "https://example.com/healthy.json"),
                "Display healthy",
                "ID does not match its file name",
            ),
        ] {
            let (_dir, sources) = test_store();
            fs::create_dir_all(sources.managed_configs_dir()).unwrap();
            let healthy = sources.managed_config_path("healthy");
            fs::write(&healthy, managed_config_json("healthy", "https://example.com/healthy.json"))
                .unwrap();
            let invalid = sources.managed_config_path(id);
            fs::write(&invalid, content).unwrap();
            sources.settings_handler.update_active_downloader(id).unwrap();

            let snapshot = sources.load().unwrap();
            assert_eq!(snapshot.configs.len(), 1);
            assert_eq!(snapshot.active_config_id.as_deref(), Some("healthy"));
            assert!(snapshot.inactive_configs().is_empty());
            assert_eq!(snapshot.installed_configs.len(), 2);
            let entry = snapshot.installed_configs.iter().find(|cfg| cfg.id == id).unwrap();
            assert_eq!(entry.display_name, name);
            assert!(entry.error.as_deref().unwrap().contains(error));
            assert!(sources.select_active(id).is_err());

            let replacement = managed_config_json(id, "https://example.com/config.json");
            assert!(sources.commit(fetched_install(replacement.clone())).is_err());

            sources.remove(&entry.id).unwrap();
            assert!(!invalid.exists());
            assert!(healthy.exists());
            let remaining = sources.load().unwrap();
            assert_eq!(remaining.installed_configs.len(), 1);
            assert!(remaining.installed_configs[0].error.is_none());

            sources.commit(fetched_install(replacement)).unwrap();
            let reinstalled = sources.load().unwrap();
            assert_eq!(reinstalled.configs.len(), 2);
            assert!(reinstalled.installed_configs.iter().all(|cfg| cfg.error.is_none()));
            sources.select_active(id).unwrap();
            assert_eq!(sources.load().unwrap().active_config_id.as_deref(), Some(id));
        }
    }

    #[tokio::test]
    async fn removing_only_invalid_source_leaves_no_sources() {
        let (_dir, sources) = test_store();
        fs::create_dir_all(sources.managed_configs_dir()).unwrap();
        fs::write(sources.managed_config_path("broken"), "invalid json").unwrap();
        sources.settings_handler.update_active_downloader("broken").unwrap();

        let snapshot = sources.load().unwrap();
        assert_eq!(snapshot.installed_configs.len(), 1);
        assert!(snapshot.active_config().is_none());
        sources.persist_active_config(&snapshot).unwrap();
        assert!(sources.current_active_config_id().is_empty());

        sources.remove("broken").unwrap();
        let snapshot = sources.load().unwrap();
        assert!(snapshot.installed_configs.is_empty());
        assert!(snapshot.active_config().is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_allows_update_url_change() {
        let (_dir, sources) = test_store();
        fs::create_dir_all(sources.managed_configs_dir()).unwrap();

        let server = MockServer::start().await;
        let original_url = format!("{}/downloader.json", server.uri());
        let installed_path = sources.managed_config_path("test");
        fs::write(&installed_path, managed_config_json("test", &original_url)).unwrap();

        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(managed_config_json(
                "test",
                "https://other.example/downloader.json",
            )))
            .mount(&server)
            .await;

        let fetched = fetch_config(SensitiveUrl::new(&original_url), FetchMode::Refresh("test"))
            .await
            .unwrap();
        sources.commit(fetched).unwrap();

        let installed = fs::read_to_string(&installed_path).unwrap();
        assert!(installed.contains("https://other.example/downloader.json"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn migrate_legacy_config_imports_remote_and_deletes_file() {
        let (_dir, sources) = test_store();

        let server = MockServer::start().await;
        let url = format!("{}/downloader.json", server.uri());
        Mock::given(method("GET"))
            .and(path("/downloader.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(managed_config_json("legacy", &url)),
            )
            .mount(&server)
            .await;

        let legacy_path = sources.app_dir.join(LEGACY_CONFIG_FILENAME);
        fs::write(&legacy_path, managed_config_json("legacy", &url)).unwrap();

        let warning = sources.migrate_legacy_config_if_needed().await;
        assert!(warning.is_none());
        assert!(!legacy_path.exists());
        assert!(sources.managed_config_path("legacy").exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn migrate_legacy_without_update_url_keeps_file() {
        let (_dir, sources) = test_store();
        let legacy_path = sources.app_dir.join(LEGACY_CONFIG_FILENAME);
        fs::write(&legacy_path, legacy_config_json_without_update_url("legacy")).unwrap();

        let warning = sources.migrate_legacy_config_if_needed().await;
        assert!(warning.is_some());
        assert!(legacy_path.exists());
        assert!(!sources.managed_config_path("legacy").exists());
    }

    #[test]
    fn resolve_active_config_id_falls_back_to_first_sorted_config() {
        let configs =
            ["b", "a"].map(|id| DownloaderConfig { id: id.into(), ..Default::default() }).to_vec();

        assert_eq!(resolve_active_config_id(&configs, String::new()).as_deref(), Some("a"));
        assert_eq!(resolve_active_config_id(&configs, "missing".into()).as_deref(), Some("a"));
        assert_eq!(resolve_active_config_id(&configs, "a".into()).as_deref(), Some("a"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_reassigns_active_source() {
        let (_dir, sources) = test_store();

        let alpha = sources.managed_config_path("alpha");
        let beta = sources.managed_config_path("beta");
        fs::create_dir_all(sources.managed_configs_dir()).unwrap();
        fs::write(&alpha, managed_config_json("alpha", "https://example.com/alpha.json")).unwrap();
        fs::write(&beta, managed_config_json("beta", "https://example.com/beta.json")).unwrap();

        sources.settings_handler.update_active_downloader("beta").unwrap();

        sources.remove("beta").unwrap();
        sources.persist_active_config(&sources.load().unwrap()).unwrap();

        assert!(!beta.exists());
        assert!(alpha.exists());
        assert_eq!(sources.current_active_config_id(), "alpha");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_clears_active_source_when_last_removed() {
        let (_dir, sources) = test_store();

        let only = sources.managed_config_path("only");
        fs::create_dir_all(sources.managed_configs_dir()).unwrap();
        fs::write(&only, managed_config_json("only", "https://example.com/only.json")).unwrap();

        sources.settings_handler.update_active_downloader("only").unwrap();

        sources.remove("only").unwrap();
        sources.persist_active_config(&sources.load().unwrap()).unwrap();

        assert!(!only.exists());
        assert_eq!(sources.current_active_config_id(), "");
    }

    #[tokio::test]
    async fn remove_orphaned_caches_keeps_installed_sources() {
        let (_dir, sources) = test_store();
        fs::create_dir_all(sources.managed_configs_dir()).unwrap();
        fs::write(
            sources.managed_config_path("valid"),
            managed_config_json("valid", "https://example.com/valid.json"),
        )
        .unwrap();
        fs::write(sources.managed_config_path("broken"), "invalid json").unwrap();
        for id in ["valid", "broken", "removed", "_bootstrap"] {
            fs::create_dir_all(sources.runtime_cache_dir(id)).unwrap();
            fs::write(sources.runtime_cache_dir(id).join("rclone.conf"), "cached").unwrap();
        }

        sources.remove_orphaned_caches().unwrap();

        assert!(sources.runtime_cache_dir("valid").join("rclone.conf").exists());
        assert!(sources.runtime_cache_dir("broken").join("rclone.conf").exists());
        assert!(!sources.runtime_cache_dir("removed").exists());
        assert!(!sources.runtime_cache_dir("_bootstrap").exists());
    }
}
