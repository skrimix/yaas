use std::str::FromStr;

use lazy_regex::regex;
use rinf::SignalPiece;
use serde::{Deserialize, Deserializer, Serialize};
/// Custom helper used only during deserialization from CSV/remote list.
/// This allows us to keep one public struct while still supporting
/// header aliases and custom parsing logic.
/// This is needed because rinf doesn't let us use serde "alias" and "deserialize_with" attributes.
#[derive(Deserialize)]
struct CloudAppCsvHelper {
    #[serde(alias = "Game Name")]
    app_name: String,
    #[serde(alias = "Release Name")]
    full_name: String,
    #[serde(alias = "Package Name")]
    package_name: String,
    #[serde(alias = "Version Code")]
    version_code: u32,
    #[serde(alias = "Last Updated")]
    last_updated: String,
    #[serde(alias = "Size (MB)")]
    size: String,
}

fn parse_size_mb_to_bytes(size_mb_str: &str) -> Result<u64, String> {
    let size_mb = f64::from_str(size_mb_str)
        .map_err(|e| format!("invalid size (MB) '{size_mb_str}': {e}"))?;
    Ok((size_mb * 1000.0 * 1000.0) as u64)
}

/// Strips known rename markers from a package name to derive the original.
fn normalize_package_name(name: &str) -> String {
    let re = regex!(r"(^mr\.)|(^mrf\.)|(\.jjb)");
    re.replace_all(name, "").into_owned()
}

/// A cloud app from the local device.
#[derive(Serialize, Debug, Clone, SignalPiece)]
pub struct CloudApp {
    pub app_name: String,
    pub full_name: String,
    pub package_name: String,
    /// Package name normalized to original by removing known renames
    pub original_package_name: String,
    pub version_code: u32,
    pub last_updated: String,
    pub size: u64,
}

impl<'de> Deserialize<'de> for CloudApp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Delegate to helper with serde field attributes, then convert
        let helper = CloudAppCsvHelper::deserialize(deserializer)?;
        let size = parse_size_mb_to_bytes(&helper.size).map_err(serde::de::Error::custom)?;
        let original = normalize_package_name(&helper.package_name);
        Ok(CloudApp {
            app_name: helper.app_name,
            full_name: helper.full_name,
            package_name: helper.package_name,
            original_package_name: original,
            version_code: helper.version_code,
            last_updated: helper.last_updated,
            size,
        })
    }
}

#[derive(serde::Deserialize, Debug)]
pub struct AppApiResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub quality_rating_aggregate: Option<f32>,
    #[serde(default)]
    pub rating_count: Option<u32>,
}
