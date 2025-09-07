use std::{
    error::Error,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use glob::glob;
use sysproxy::Sysproxy;
use tokio::fs;
use tracing::{debug, error, instrument};

#[instrument(ret, level = "debug")]
pub fn get_sys_proxy() -> Option<String> {
    let proxy = Sysproxy::get_system_proxy();
    match proxy {
        Ok(proxy) => {
            if proxy.enable {
                let result = format!("http://{}:{}", proxy.host, proxy.port);
                debug!(proxy = &result, "got system proxy");
                return Some(result);
            }
        }
        Err(e) => {
            error!(error = &e as &dyn Error, "failed to get system proxy");
        }
    }
    None
}

/// Finds the first immediate subdirectory in `dir`
pub async fn first_subdirectory(dir: &Path) -> Result<Option<PathBuf>> {
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// Checks recursively if a directory contains any files
pub async fn dir_has_any_files(dir: &Path) -> Result<bool> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(false);
    }
    let pattern = dir.join("**/*").to_string_lossy().to_string();
    for path in (glob(&pattern).context("Invalid glob pattern for dir_has_any_files")?).flatten() {
        if path.is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Removes a specific child directory if present
pub async fn remove_child_dir_if_exists(parent: &Path, child: &str) -> Result<()> {
    let target = parent.join(child);
    if target.exists() {
        let _ = fs::remove_dir_all(target).await;
    }
    Ok(())
}

/// Decompresses all .7z archives found directly under `dir` into `dir`.
#[instrument(skip(dir), err)]
pub async fn decompress_all_7z_in_dir(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let pattern = dir.join("*.7z").to_string_lossy().to_string();
    for path in
        (glob(&pattern).context("Invalid glob pattern for decompress_all_7z_in_dir")?).flatten()
    {
        if path.is_file() {
            debug!(path = %path.display(), "Decompressing 7z archive");
            let dir_clone = dir.to_path_buf();
            tokio::task::spawn_blocking(move || {
                sevenz_rust2::decompress_file(&path, dir_clone)
                    .context("Error decompressing 7z archive")
            })
            .await??;
        }
    }
    Ok(())
}
