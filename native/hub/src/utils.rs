use std::{
    error::Error,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
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
    // TODO: use glob
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let mut rd = match fs::read_dir(&path).await {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        while let Some(entry) = rd.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_file() {
                return Ok(true);
            } else if file_type.is_dir() {
                stack.push(entry.path());
            }
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
    // TODO: use glob
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        if entry.file_type().await?.is_file()
            && entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("7z"))
        {
            let path = entry.path();
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
