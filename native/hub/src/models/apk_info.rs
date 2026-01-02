use std::path::Path;

use anyhow::{Context, Result, anyhow};
use apk_info::Apk;
use tracing::instrument;

#[derive(Debug, Clone)]
#[allow(unused)]
pub(crate) struct ApkInfo {
    pub application_label: Option<String>,
    pub package_name: String,
    pub version_code: Option<u32>,
    pub version_name: Option<String>,
}

/// Parse minimal info from an APK using `apk_info` crate.
#[instrument(ret, level = "debug", fields(apk_path = %apk_path.as_ref().display()))]
pub(crate) fn get_apk_info(apk_path: impl AsRef<Path>) -> Result<ApkInfo> {
    let apk_path = apk_path.as_ref();
    if !apk_path.exists() {
        return Err(anyhow!("APK file not found: {}", apk_path.display()));
    }

    let apk = Apk::new(apk_path).context("Failed to read APK file")?;

    let package_name = apk.get_package_name().ok_or_else(|| anyhow!("APK missing package name"))?;
    let version_code = apk
        .get_version_code()
        .map(|v| v.parse::<u32>().context("Failed to parse version code"))
        .transpose()?;
    let version_name = apk.get_version_name();
    let application_label = apk.get_application_label();

    Ok(ApkInfo { application_label, package_name, version_code, version_name })
}
