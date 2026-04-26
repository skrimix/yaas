use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, ensure};
use tracing::{debug, info, warn};

use crate::{
    downloader::{config::DownloaderConfig, http_cache},
    models::{InstalledDownloaderConfig, Settings},
    settings::SettingsHandler,
};

pub(crate) const LEGACY_CONFIG_FILENAME: &str = "downloader.json";
const MANAGED_CONFIGS_DIR: &str = "downloader_configs";

#[derive(Debug, Clone, Default)]
pub(crate) struct LoadedSources {
    pub(crate) configs: Vec<DownloaderConfig>,
    pub(crate) active_config_id: Option<String>,
    pub(crate) warnings: Vec<String>,
}

impl LoadedSources {
    pub(crate) fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }

    pub(crate) fn active_config(&self) -> Option<DownloaderConfig> {
        let active_config_id = self.active_config_id.as_deref()?;
        self.configs.iter().find(|cfg| cfg.id == active_config_id).cloned()
    }

    pub(crate) fn installed_configs(&self) -> Vec<InstalledDownloaderConfig> {
        self.configs
            .iter()
            .map(|cfg| InstalledDownloaderConfig {
                id: cfg.id.clone(),
                display_name: cfg.effective_display_name(),
                description: cfg.effective_description(),
            })
            .collect()
    }

    pub(crate) fn warning_message(&self) -> Option<String> {
        warnings_to_message(&self.warnings)
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
pub(crate) struct DownloaderSources {
    app_dir: PathBuf,
    settings_handler: Arc<SettingsHandler>,
}

impl DownloaderSources {
    pub(crate) fn new(app_dir: PathBuf, settings_handler: Arc<SettingsHandler>) -> Self {
        Self { app_dir, settings_handler }
    }

    pub(crate) fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    pub(crate) fn load(
        &self,
        extra_warnings: impl IntoIterator<Item = String>,
    ) -> Result<LoadedSources> {
        let mut loaded = read_configs(&self.app_dir)?;
        loaded.warnings.extend(extra_warnings);
        let active_config_id = resolve_active_config_id(
            &loaded.configs,
            current_active_config_id(&self.settings_handler),
        );

        Ok(LoadedSources { configs: loaded.configs, active_config_id, warnings: loaded.warnings })
    }

    pub(crate) fn persist_active_config(&self, sources: &LoadedSources) -> Result<()> {
        let current_active_id = current_active_config_id(&self.settings_handler);
        if sources.active_config_id.as_deref() == Some(current_active_id.as_str()) {
            return Ok(());
        }

        save_active_config_id(&self.settings_handler, sources.active_config_id.as_deref())
    }

    pub(crate) async fn install_from_url(
        &self,
        url: &str,
        select_as_active: bool,
    ) -> Result<DownloaderConfig> {
        let cfg =
            fetch_managed_config(&self.app_dir, "_bootstrap", url, Some(url), None, true).await?;
        if select_as_active {
            save_active_config_id(&self.settings_handler, Some(&cfg.id))?;
        }
        Ok(cfg)
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

        let path = managed_config_path(&self.app_dir, config_id);
        ensure!(path.exists(), "Downloader config is not installed: {config_id}");

        fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))?;

        let loaded = read_configs(&self.app_dir)?;
        let next_active_id = resolve_active_config_id(
            &loaded.configs,
            current_active_config_id(&self.settings_handler),
        );
        save_active_config_id(&self.settings_handler, next_active_id.as_deref())
    }

    pub(crate) async fn refresh_all(&self, configs: &[DownloaderConfig]) -> RefreshReport {
        refresh_configs(&self.app_dir, configs).await
    }

    pub(crate) async fn refresh_active(&self, sources: &LoadedSources) -> RefreshReport {
        match sources.active_config() {
            Some(active_cfg) => {
                refresh_configs(&self.app_dir, std::slice::from_ref(&active_cfg)).await
            }
            None => RefreshReport::default(),
        }
    }

    pub(crate) fn inactive_configs(&self, sources: &LoadedSources) -> Vec<DownloaderConfig> {
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

struct ReadConfigs {
    configs: Vec<DownloaderConfig>,
    warnings: Vec<String>,
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
    let mut settings = current_settings(settings_handler);
    let new_id = config_id.unwrap_or_default().to_string();
    if settings.active_downloader_config_id == new_id {
        return Ok(());
    }

    settings.active_downloader_config_id = new_id;
    settings_handler.save_settings(&settings)
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
        return Ok(ReadConfigs { configs: Vec::new(), warnings: Vec::new() });
    }

    let mut configs = Vec::new();
    let mut ignored = Vec::new();

    for entry in fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("Failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        match DownloaderConfig::load_from_path(&path).and_then(|cfg| {
            cfg.validate_managed_remote(None)?;
            Ok(cfg)
        }) {
            Ok(cfg) => configs.push(cfg),
            Err(e) => {
                warn!(
                    error = e.as_ref() as &dyn Error,
                    path = %path.display(),
                    "Ignoring invalid managed downloader config"
                );
                ignored.push(format!(
                    "{}: {:#}",
                    path.file_name().and_then(|value| value.to_str()).unwrap_or("unknown"),
                    e
                ));
            }
        }
    }

    configs.sort_by(|left, right| left.id.cmp(&right.id));

    let warnings = if ignored.is_empty() {
        Vec::new()
    } else {
        vec![format!("Ignored invalid downloader sources: {}", ignored.join("; "))]
    };

    Ok(ReadConfigs { configs, warnings })
}

async fn cache_config_from_url(app_dir: &Path, cache_key: &str, url: &str) -> Result<PathBuf> {
    ensure!(is_http_url(url), "Config update URL must start with http:// or https://");
    debug!(update_url = %url, cache_key = %cache_key, "Downloading downloader config from URL");

    let (cache_dir, cached_cfg_path) = config_download_cache_path(app_dir, cache_key);

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .context("Failed to build HTTP client for downloader config update")?;

    let _ =
        http_cache::update_file_cached(&client, url, &cached_cfg_path, &cache_dir, None).await?;

    Ok(cached_cfg_path)
}

async fn fetch_managed_config(
    app_dir: &Path,
    cache_key: &str,
    url: &str,
    source_url: Option<&str>,
    expected_id: Option<&str>,
    refuse_existing: bool,
) -> Result<DownloaderConfig> {
    let remote_cfg_path = cache_config_from_url(app_dir, cache_key, url).await?;
    write_managed_config(app_dir, &remote_cfg_path, source_url, expected_id, refuse_existing)
}

fn write_managed_config(
    app_dir: &Path,
    src: &Path,
    source_url: Option<&str>,
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

async fn refresh_configs(app_dir: &Path, configs: &[DownloaderConfig]) -> RefreshReport {
    let mut report = RefreshReport::default();

    for cfg in configs {
        let Some(update_url) = cfg.config_update_url.as_deref().map(str::trim) else {
            report.failed.push(format!("{}: missing config_update_url", cfg.id));
            continue;
        };

        let refresh_result = async {
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
            Some("https://other.example/config.json"),
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
            Some("https://example.com/downloader.json"),
            None,
            true,
        )
        .unwrap();
        assert_eq!(first.id, "test");

        let err = write_managed_config(
            dir.path(),
            &src,
            Some("https://example.com/downloader.json"),
            None,
            true,
        )
        .unwrap_err();
        assert!(format!("{:#}", err).contains("Downloader config ID already installed"));
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
        let sources = DownloaderSources::new(app_dir.clone(), settings.clone());

        let alpha = managed_config_path(&app_dir, "alpha");
        let beta = managed_config_path(&app_dir, "beta");
        std::fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        std::fs::write(&alpha, managed_config_json("alpha", "https://example.com/alpha.json"))
            .unwrap();
        std::fs::write(&beta, managed_config_json("beta", "https://example.com/beta.json"))
            .unwrap();

        save_active_config_id(&settings, Some("beta")).unwrap();

        sources.remove("beta").unwrap();

        assert!(!beta.exists());
        assert!(alpha.exists());
        assert_eq!(current_active_config_id(&settings), "alpha");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_managed_config_clears_active_source_when_last_removed() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().to_path_buf();
        let settings = SettingsHandler::new(app_dir.clone(), true).unwrap();
        let sources = DownloaderSources::new(app_dir.clone(), settings.clone());

        let only = managed_config_path(&app_dir, "only");
        std::fs::create_dir_all(managed_configs_dir(&app_dir)).unwrap();
        std::fs::write(&only, managed_config_json("only", "https://example.com/only.json"))
            .unwrap();

        save_active_config_id(&settings, Some("only")).unwrap();

        sources.remove("only").unwrap();

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
