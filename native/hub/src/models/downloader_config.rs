use std::{collections::HashMap, fmt, fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::error;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DownloaderConfig {
    /// ID of the config. Used for cache separation.
    pub id: String,
    pub rclone_path: RclonePath,
    #[serde(default)]
    pub rclone_config_path: Option<String>,
    #[serde(default)]
    pub remote_name_filter_regex: Option<String>,
    #[serde(default)]
    pub disable_randomize_remote: bool,
    /// Optional remote used for app sharing uploads.
    #[serde(default)]
    pub share_remote_name: Option<String>,
    /// Optional path within the share remote where uploaded archives are placed.
    #[serde(default)]
    pub share_remote_path: Option<String>,
    /// Repository layout selector. "ffa" or "vrp-public"
    pub layout: RepoLayoutKind,
    #[serde(default = "default_root_dir")]
    pub root_dir: String,
    #[serde(default = "default_list_path")]
    pub list_path: String,
    #[serde(default)]
    pub vrp_public_url: String,
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
        Ok(cfg)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RepoLayoutKind {
    #[serde(rename = "ffa")]
    Ffa,
    #[serde(rename = "vrp-public")]
    VrpPublic,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum RclonePath {
    Single(String),
    Map(HashMap<String, String>),
}

#[cfg(target_os = "windows")]
pub(crate) const CURRENT_PLATFORM: &str = "windows";
#[cfg(target_os = "linux")]
pub(crate) const CURRENT_PLATFORM: &str = "linux";
#[cfg(target_os = "macos")]
pub(crate) const CURRENT_PLATFORM: &str = "macos";

#[cfg(target_arch = "x86_64")]
pub(crate) const CURRENT_ARCH: &str = "x64";
#[cfg(target_arch = "aarch64")]
pub(crate) const CURRENT_ARCH: &str = "arm64";
#[cfg(target_arch = "x86")]
pub(crate) const CURRENT_ARCH: &str = "x86";
#[cfg(target_arch = "arm")]
pub(crate) const CURRENT_ARCH: &str = "arm";

pub(crate) fn current_platform_arch_key() -> String {
    format!("{}-{}", CURRENT_PLATFORM, CURRENT_ARCH)
}

impl RclonePath {
    pub(crate) fn resolve_for_current_platform(&self) -> Result<String> {
        match self {
            RclonePath::Single(s) => Ok(s.clone()),
            RclonePath::Map(map) => {
                let combined = current_platform_arch_key();
                if let Some(v) = map.get(&combined) {
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
            RclonePath::Single(ref s) => assert_eq!(s, "/bin/echo"),
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
        let combined = current_platform_arch_key();

        map.insert(combined.clone(), String::from("/bin/exact"));
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
        map2.remove(&combined);
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
}
