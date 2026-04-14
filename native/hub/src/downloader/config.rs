use std::{collections::HashMap, fmt, fs, path::Path};

use anyhow::{Context, Result, ensure};
use const_format::concatcp;
use serde::Deserialize;
use tracing::error;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DownloaderConfig {
    /// ID of the config. Used for cache separation.
    pub id: String,
    // TODO: should this be a URI?
    #[serde(default)]
    pub rclone_path: Option<RclonePath>,
    #[serde(default)]
    pub rclone_config_path: Option<String>,
    #[serde(default)]
    pub remote_name_filter_regex: Option<String>,
    #[serde(default)]
    pub disable_randomize_remote: bool,
    /// Optional remote used for app donation uploads.
    // TODO: check that this and the next field are both present or both absent during parsing
    #[serde(default)]
    pub donation_remote_name: Option<String>,
    /// Optional path within the donation remote where uploaded archives are placed.
    #[serde(default)]
    pub donation_remote_path: Option<String>,
    /// Optional path to a newline-separated donation blacklist.
    ///
    /// For FFA layout this is a path on the configured rclone remote.
    #[serde(default)]
    pub donation_blacklist_path: Option<String>,
    /// Repository layout selector.
    pub layout: RepoLayoutKind,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_root_dir")]
    pub root_dir: String,
    #[serde(default = "default_list_path")]
    pub list_path: String,
    /// Optional URL used to update this downloader configuration.
    #[serde(default)]
    pub config_update_url: Option<String>,
}

fn default_root_dir() -> String {
    "Quest Games".to_string()
}

fn default_list_path() -> String {
    "FFA.txt".to_string()
}

impl DownloaderConfig {
    pub(crate) fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let cfg: DownloaderConfig = serde_json::from_str(&content)
            .context("Failed to parse downloader.json")
            .inspect_err(|e| {
                error!("Failed to parse downloader.json: {:#}", e);
            })?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(!self.id.trim().is_empty(), "downloader.id must not be empty");

        match self.layout {
            RepoLayoutKind::Ffa => {
                ensure!(
                    self.rclone_path.is_some(),
                    "rclone_path is required for the ffa repository layout"
                );
                ensure!(
                    self.rclone_config_path
                        .as_deref()
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false),
                    "rclone_config_path is required for the ffa repository layout"
                );
            }
            RepoLayoutKind::NewRepo => {
                let base_url = self.base_url.as_deref().map(str::trim).unwrap_or_default();
                ensure!(
                    !base_url.is_empty(),
                    "base_url is required for the new-repo repository layout"
                );
                let parsed = reqwest::Url::parse(base_url)
                    .with_context(|| format!("Invalid new-repo base_url: {base_url}"))?;
                ensure!(
                    parsed.scheme() == "http" || parsed.scheme() == "https",
                    "new-repo base_url must use http or https"
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
impl Default for DownloaderConfig {
    fn default() -> Self {
        Self {
            id: "test".to_string(),
            rclone_path: Some(RclonePath::Single("/bin/echo".to_string())),
            rclone_config_path: None,
            remote_name_filter_regex: None,
            disable_randomize_remote: false,
            donation_remote_name: None,
            donation_remote_path: None,
            donation_blacklist_path: None,
            layout: RepoLayoutKind::Ffa,
            base_url: None,
            root_dir: default_root_dir(),
            list_path: default_list_path(),
            config_update_url: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RepoLayoutKind {
    #[serde(rename = "ffa")]
    Ffa,
    #[serde(rename = "new-repo")]
    NewRepo,
}

#[derive(Debug, Clone, Deserialize)]
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
                "rclone_config_path": "/tmp/rclone.conf"
            }"#,
        );

        let cfg = DownloaderConfig::load_from_path(&cfg_path).expect("load config");
        match cfg.rclone_path {
            Some(RclonePath::Single(ref s)) => assert_eq!(s, "/bin/echo"),
            _ => panic!("expected Single path"),
        }
        assert_eq!(cfg.rclone_config_path.as_deref(), Some("/tmp/rclone.conf"));
        // default_randomize_remote = false when omitted
        assert!(!cfg.disable_randomize_remote);
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
    fn new_repo_requires_base_url() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("downloader.json");
        write_file(
            &cfg_path,
            r#"{
                "id": "new-repo",
                "layout": "new-repo"
            }"#,
        );

        let err = DownloaderConfig::load_from_path(&cfg_path).expect_err("missing base_url");
        assert!(format!("{err:#}").contains("base_url is required"));
    }

    #[test]
    fn new_repo_loads_without_rclone_fields() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("downloader.json");
        write_file(
            &cfg_path,
            r#"{
                "id": "new-repo",
                "layout": "new-repo",
                "base_url": "https://example.com/repo"
            }"#,
        );

        let cfg = DownloaderConfig::load_from_path(&cfg_path).expect("load config");
        assert_eq!(cfg.layout, RepoLayoutKind::NewRepo);
        assert!(cfg.rclone_path.is_none());
        assert_eq!(cfg.base_url.as_deref(), Some("https://example.com/repo"));
    }
}
