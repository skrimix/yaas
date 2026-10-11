use std::{collections::HashMap, fmt, fs, path::Path};

use anyhow::{Context, Result, ensure};
use const_format::concatcp;
use serde::Deserialize;
use tracing::error;

use crate::downloader::{SensitiveUrl, is_http_url};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct DownloaderConfig {
    /// ID of the config. Used for cache separation.
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// URL used to update this downloader configuration.
    pub config_update_url: String,
    /// Repository layout and its settings, selected by the `layout` field.
    #[serde(flatten)]
    pub backend: Backend,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "layout")]
pub(crate) enum Backend {
    #[serde(rename = "ffa")]
    Ffa(FfaConfig),
    #[serde(rename = "new-repo")]
    NewRepo(NewRepoConfig),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct FfaConfig {
    // TODO: should this be a URI?
    pub rclone_path: RclonePath,
    pub rclone_config_path: String,
    #[serde(default)]
    pub remote_name_filter_regex: Option<String>,
    #[serde(default)]
    pub disable_randomize_remote: bool,
    /// Optional remote used for app donation uploads. Set together with `donation_remote_path`.
    #[serde(default)]
    pub donation_remote_name: Option<String>,
    /// Optional path within the donation remote where uploaded archives are placed.
    #[serde(default)]
    pub donation_remote_path: Option<String>,
    /// Optional path on the configured rclone remote to a newline-separated donation blacklist.
    #[serde(default)]
    pub donation_blacklist_path: Option<String>,
    #[serde(default = "default_root_dir")]
    pub root_dir: String,
    #[serde(default = "default_list_path")]
    pub list_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct NewRepoConfig {
    /// Repository URL without a trailing slash.
    pub base_url: String,
}

fn default_root_dir() -> String {
    "Quest Games".to_string()
}

fn default_list_path() -> String {
    "FFA.txt".to_string()
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

impl DownloaderConfig {
    pub(crate) fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Self::parse(&content)
    }

    pub(crate) fn parse(content: &str) -> Result<Self> {
        let mut cfg: DownloaderConfig = serde_json::from_str(content)
            .context("Failed to parse downloader.json")
            .inspect_err(|e| {
                error!("Failed to parse downloader.json: {:#}", e);
            })?;
        cfg.config_update_url = cfg.config_update_url.trim().to_string();
        if let Backend::NewRepo(repo) = &mut cfg.backend {
            repo.base_url = repo.base_url.trim().trim_end_matches('/').to_string();
        }
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        let trimmed_id = self.id.trim();
        ensure!(!trimmed_id.is_empty(), "downloader.id must not be empty");
        ensure!(trimmed_id == self.id, "downloader.id must not have surrounding whitespace");
        ensure!(
            !self.id.contains('/') && !self.id.contains('\\') && self.id != "." && self.id != "..",
            "downloader.id must be a safe file name"
        );
        ensure_http_url("config_update_url", &self.config_update_url)?;

        match &self.backend {
            Backend::Ffa(ffa) => {
                ensure!(
                    !ffa.rclone_config_path.trim().is_empty(),
                    "rclone_config_path must not be empty"
                );
                let config_is_url = is_http_url(&ffa.rclone_config_path);
                ensure!(
                    ffa.rclone_path.candidates().all(|path| is_http_url(path) == config_is_url),
                    "rclone_path and rclone_config_path must both be local or both be URLs"
                );
                ensure!(
                    non_empty(&ffa.donation_remote_name).is_some()
                        == non_empty(&ffa.donation_remote_path).is_some(),
                    "donation_remote_name and donation_remote_path must be set together"
                );
            }
            Backend::NewRepo(repo) => ensure_http_url("base_url", &repo.base_url)?,
        }

        Ok(())
    }

    /// Compares the configuration used by a session.
    pub(crate) fn same_runtime(&self, other: &Self) -> bool {
        self.id == other.id && self.backend == other.backend
    }

    /// Returns the remote and path for app donation uploads, if configured.
    pub(crate) fn donation_target(&self) -> Option<(&str, &str)> {
        match &self.backend {
            Backend::Ffa(ffa) => ffa.donation_target(),
            Backend::NewRepo(_) => None,
        }
    }

    pub(crate) fn effective_display_name(&self) -> String {
        self.display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.id)
            .to_string()
    }

    pub(crate) fn effective_description(&self) -> String {
        self.description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string()
    }
}

impl FfaConfig {
    pub(crate) fn donation_target(&self) -> Option<(&str, &str)> {
        Some((non_empty(&self.donation_remote_name)?, non_empty(&self.donation_remote_path)?))
    }

    pub(crate) fn donation_blacklist_path(&self) -> Option<&str> {
        non_empty(&self.donation_blacklist_path)
    }
}

fn ensure_http_url(field: &str, value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "{field} must not be empty");
    let parsed = reqwest::Url::parse(value)
        .with_context(|| format!("Invalid {field}: {}", SensitiveUrl::new(value)))?;
    ensure!(
        parsed.scheme() == "http" || parsed.scheme() == "https",
        "{field} must use http or https"
    );
    Ok(())
}

#[cfg(test)]
impl Default for DownloaderConfig {
    fn default() -> Self {
        Self {
            id: "test".to_string(),
            display_name: None,
            description: None,
            config_update_url: "https://example.com/config.json".to_string(),
            backend: Backend::Ffa(FfaConfig::default()),
        }
    }
}

#[cfg(test)]
impl Default for FfaConfig {
    fn default() -> Self {
        Self {
            rclone_path: RclonePath::Single("/bin/echo".to_string()),
            rclone_config_path: "/tmp/rclone.conf".to_string(),
            remote_name_filter_regex: None,
            disable_randomize_remote: false,
            donation_remote_name: None,
            donation_remote_path: None,
            donation_blacklist_path: None,
            root_dir: default_root_dir(),
            list_path: default_list_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum RclonePath {
    Single(String),
    Map(HashMap<String, String>),
}

const CURRENT_PLATFORM: &str = std::env::consts::OS;

#[cfg(target_arch = "x86_64")]
const CURRENT_ARCH: &str = "x64";
#[cfg(target_arch = "aarch64")]
const CURRENT_ARCH: &str = "arm64";
#[cfg(target_arch = "x86")]
const CURRENT_ARCH: &str = "x86";
#[cfg(target_arch = "arm")]
const CURRENT_ARCH: &str = "arm";

const CURRENT_PLATFORM_ARCH_KEY: &str = concatcp!(CURRENT_PLATFORM, "-", CURRENT_ARCH);

impl RclonePath {
    /// Returns the paths for all platforms.
    fn candidates(&self) -> impl Iterator<Item = &str> {
        let (single, map) = match self {
            RclonePath::Single(s) => (Some(s.as_str()), None),
            RclonePath::Map(map) => (None, Some(map.values().map(String::as_str))),
        };
        single.into_iter().chain(map.into_iter().flatten())
    }

    pub(crate) fn resolve_for_current_platform(&self) -> Result<String> {
        match self {
            RclonePath::Single(s) => Ok(s.clone()),
            RclonePath::Map(map) => {
                if let Some(v) = map.get(CURRENT_PLATFORM_ARCH_KEY) {
                    return Ok(v.clone());
                }
                if let Some(v) = map.get(CURRENT_PLATFORM) {
                    return Ok(v.clone());
                }

                let available: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
                Err(anyhow::anyhow!(
                    "rclone_path missing key: '{}-{}' or '{}' (available: {:?})",
                    CURRENT_PLATFORM,
                    CURRENT_ARCH,
                    CURRENT_PLATFORM,
                    available
                ))
            }
        }
    }
}

impl fmt::Display for RclonePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.resolve_for_current_platform() {
            Ok(s) => write!(f, "{}", s),
            Err(_) => match self {
                RclonePath::Single(s) => write!(f, "{}", s),
                RclonePath::Map(map) => {
                    write!(f, "<map keys={:?}>", map.keys().collect::<Vec<_>>())
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn write_file(path: &Path, content: &str) {
        std::fs::write(path, content).expect("write file");
    }

    #[test]
    fn load_from_path_with_defaults_and_single_path() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("downloader.json");
        write_file(
            &cfg_path,
            r#"{
                "id": "test",
                "layout": "ffa",
                "rclone_path": "/bin/echo",
                "rclone_config_path": "/tmp/rclone.conf",
                "config_update_url": " https://example.com/config.json "
            }"#,
        );

        let cfg = DownloaderConfig::load_from_path(&cfg_path).expect("load config");
        assert_eq!(cfg.config_update_url, "https://example.com/config.json");
        let Backend::Ffa(ffa) = cfg.backend else { panic!("expected ffa layout") };
        assert_eq!(ffa.rclone_path, RclonePath::Single("/bin/echo".into()));
        assert_eq!(ffa.rclone_config_path, "/tmp/rclone.conf");
        assert!(!ffa.disable_randomize_remote);
        assert_eq!(ffa.root_dir, "Quest Games");
        assert_eq!(ffa.list_path, "FFA.txt");
    }

    #[test]
    fn rclone_path_map_resolves_platform_first_then_platform_only() {
        // Build a map that includes both exact platform-arch and platform-only keys
        let mut map = HashMap::new();
        map.insert(CURRENT_PLATFORM_ARCH_KEY.to_string(), String::from("/bin/exact"));
        map.insert(CURRENT_PLATFORM.to_string(), String::from("/bin/platform"));
        map.insert(String::from("other"), String::from("/bin/other"));

        let path = RclonePath::Map(map);
        let resolved = path.resolve_for_current_platform().expect("resolve current");
        assert_eq!(resolved, "/bin/exact");

        // Remove the exact key to ensure platform-only fallback
        let mut map2 = match path {
            RclonePath::Map(m) => m,
            _ => unreachable!(),
        };
        map2.remove(CURRENT_PLATFORM_ARCH_KEY);
        let path2 = RclonePath::Map(map2);
        let resolved2 = path2.resolve_for_current_platform().expect("resolve platform only");
        assert_eq!(resolved2, "/bin/platform");
    }

    #[test]
    fn rclone_path_map_errors_when_missing_keys() {
        let mut map = HashMap::new();
        map.insert(String::from("irrelevant"), String::from("/bin/foo"));
        let path = RclonePath::Map(map);
        let err = path.resolve_for_current_platform().unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("rclone_path missing key"));
    }

    #[test]
    fn layout_specific_fields_are_required() {
        for (config, error) in [
            (
                r#"{"id": "a", "layout": "new-repo", "config_update_url": "https://example.com"}"#,
                "base_url",
            ),
            (
                r#"{"id": "a", "layout": "ffa", "rclone_path": "/bin/echo", "config_update_url": "https://example.com"}"#,
                "rclone_config_path",
            ),
            (
                r#"{"id": "a", "layout": "new-repo", "base_url": "https://example.com"}"#,
                "config_update_url",
            ),
        ] {
            let err = DownloaderConfig::parse(config).expect_err(error);
            assert!(format!("{err:#}").contains(error), "{err:#}");
        }
    }

    #[test]
    fn newrepo_loads_without_rclone_fields() {
        let cfg = DownloaderConfig::parse(
            r#"{
                "id": "new-repo",
                "layout": "new-repo",
                "base_url": "https://example.com/repo/",
                "config_update_url": "https://example.com/config.json"
            }"#,
        )
        .expect("load config");
        assert_eq!(
            cfg.backend,
            Backend::NewRepo(NewRepoConfig { base_url: "https://example.com/repo".into() })
        );
    }

    #[test]
    fn ffa_rejects_mixed_url_and_local_rclone_files() {
        let mut map = HashMap::new();
        map.insert(CURRENT_PLATFORM.to_string(), "/bin/rclone".to_string());
        map.insert("other".to_string(), "https://example.com/rclone".to_string());
        let cfg = DownloaderConfig {
            backend: Backend::Ffa(FfaConfig {
                rclone_path: RclonePath::Map(map),
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(format!("{err:#}").contains("must both be local or both be URLs"));
    }

    #[test]
    fn donation_remote_name_and_path_are_set_together() {
        let ffa = |name: Option<&str>, path: Option<&str>| DownloaderConfig {
            backend: Backend::Ffa(FfaConfig {
                donation_remote_name: name.map(Into::into),
                donation_remote_path: path.map(Into::into),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(ffa(Some("remote"), None).validate().is_err());
        assert!(ffa(Some(" "), Some("path")).validate().is_err());
        assert!(ffa(None, None).validate().is_ok());
        let cfg = ffa(Some("remote"), Some("path"));
        cfg.validate().unwrap();
        assert_eq!(cfg.donation_target(), Some(("remote", "path")));
    }

    #[test]
    fn runtime_comparison_ignores_metadata() {
        let cfg = DownloaderConfig {
            backend: Backend::NewRepo(NewRepoConfig { base_url: "http://example.com".into() }),
            ..Default::default()
        };
        let mut changed = cfg.clone();
        changed.display_name = Some("Name".into());
        changed.description = Some("Description".into());
        changed.config_update_url = "http://example.com/new".into();
        assert!(cfg.same_runtime(&changed));
        changed.backend =
            Backend::NewRepo(NewRepoConfig { base_url: "http://other.example.com".into() });
        assert!(!cfg.same_runtime(&changed));
    }
}
