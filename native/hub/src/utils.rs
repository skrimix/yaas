use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
};

use anyhow::Result;
use sysproxy::Sysproxy;
use tokio::fs;
use tracing::{debug, instrument, trace, warn};

#[instrument(level = "debug")]
pub(crate) fn get_sys_proxy() -> Option<String> {
    let proxy = Sysproxy::get_system_proxy();
    match proxy {
        Ok(proxy) => {
            if proxy.enable {
                let result = format!("http://{}:{}", proxy.host, proxy.port);
                debug!(proxy = &result, "Got system proxy");
                return Some(result);
            }
        }
        Err(e) => {
            warn!(error = &e as &dyn Error, "Failed to get system proxy");
        }
    }
    None
}

/// Resolve an executable path by consulting (in order):
/// - a custom path (file or directory)
/// - the directory of the current executable (bundled next to the app)
/// - `PATH` (via `which`)
///
/// The `base_name` must be provided without an extension (e.g. "adb", "rclone").
#[instrument(level = "debug", ret, skip(custom_path))]
pub(crate) fn resolve_binary_path(custom_path: Option<&str>, base_name: &str) -> Result<PathBuf> {
    // Build candidate file names with platform-specific extensions
    #[cfg(target_os = "windows")]
    const CANDIDATES: [&str; 2] = [".exe", ""];
    #[cfg(not(target_os = "windows"))]
    const CANDIDATES: [&str; 1] = [""];

    // Given a directory, try to locate the binary inside it
    fn try_in_dir(dir: &Path, base: &str) -> Option<PathBuf> {
        for ext in CANDIDATES {
            let candidate =
                if ext.is_empty() { dir.join(base) } else { dir.join(format!("{base}{ext}")) };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    // 1) Try the user-provided path, if any
    if let Some(raw) = custom_path.filter(|s| !s.trim().is_empty()) {
        trace!(raw, "Trying to resolve from custom path");
        let as_path = PathBuf::from(raw);
        if as_path.is_file() {
            return Ok(as_path);
        }
        if as_path.is_dir()
            && let Some(found) = try_in_dir(&as_path, base_name)
        {
            return Ok(found);
        }
        // First check next to our own executable
        if let Ok(exe) = env::current_exe()
            && let Some(dir) = exe.parent()
            && let Some(found) = try_in_dir(dir, base_name)
        {
            return Ok(found);
        }
        // Not bundled, try PATH
        if let Ok(found) = which::which(raw) {
            return Ok(found);
        }
        warn!(raw, "Custom path did not resolve, looking for binary by name");
    }

    // 2) Next to current executable (bundled)
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
        && let Some(found) = try_in_dir(dir, base_name)
    {
        return Ok(found);
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(appimage) = env::var("APPIMAGE")
            && !appimage.is_empty()
        {
            let app_dir = Path::new(&appimage).parent();
            if let Some(dir) = app_dir
                && let Some(found) = try_in_dir(dir, base_name)
            {
                return Ok(found);
            }
        }
    }

    // 3) PATH search
    if let Ok(found) = which::which(base_name) {
        return Ok(found);
    }

    Err(anyhow::anyhow!(
        "{} binary not found (checked custom path, PATH, and app directory)",
        base_name
    ))
}

/// Finds the first immediate subdirectory in `dir`
pub(crate) async fn first_subdirectory(dir: &Path) -> Result<Option<PathBuf>> {
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
pub(crate) async fn dir_has_any_files(dir: &Path) -> Result<bool> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(false);
    }
    // Depth-first traversal using async fs, stop at first file
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let mut rd = match fs::read_dir(&path).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        while let Some(entry) = rd.next_entry().await? {
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_file() {
                return Ok(true);
            } else if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(false)
}

/// Removes a specific child directory if present. Errors are ignored.
pub(crate) async fn remove_child_dir_if_exists(parent: &Path, child: &str) {
    let target = parent.join(child);
    if target.exists() {
        let _ = fs::remove_dir_all(target).await;
    }
}
