use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::signals::download::CloudApp as SignalCloudApp;

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
    pub size: u32,
}

fn deserialize_size_mb_to_bytes<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let size_str = String::deserialize(deserializer)?;
    let size_mb = f64::from_str(&size_str).map_err(serde::de::Error::custom)?;
    Ok((size_mb * 1000.0 * 1000.0) as u32)
}

impl CloudApp {
    pub fn into_proto(&self) -> SignalCloudApp {
        SignalCloudApp {
            app_name: self.app_name.clone(),
            full_name: self.full_name.clone(),
            package_name: self.package_name.clone(),
            version_code: self.version_code,
            last_updated: self.last_updated.clone(),
            size: self.size,
        }
    }
}
