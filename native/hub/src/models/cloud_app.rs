use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CloudApp {
    #[serde(alias = "Game Name")]
    pub app_name: String,
    #[serde(alias = "Release Name")]
    pub full_name: String,
    #[serde(alias = "Package Name")]
    pub package_name: String,
    #[serde(alias = "Version Code")]
    pub version_code: u32,
    #[serde(alias = "Last Updated")]
    pub last_updated: String,
    #[serde(alias = "Size (MB)", deserialize_with = "deserialize_size_mb_to_bytes")]
    pub size: u64,
}

fn deserialize_size_mb_to_bytes<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let size_str = String::deserialize(deserializer)?;
    let size_mb = f64::from_str(&size_str).map_err(serde::de::Error::custom)?;
    Ok((size_mb * 1000.0 * 1000.0) as u64)
}
