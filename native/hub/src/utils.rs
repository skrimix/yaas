use std::{
    env,
    error::Error,
    fs as stdfs,
    fs::{File, rename},
    io,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use sysproxy::Sysproxy;
use tokio::fs;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, instrument, trace, warn};

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

/// Resolve an executable path by consulting (in order):
/// - a custom path (file or directory)
/// - the directory of the current executable (bundled next to the app)
/// - `PATH` (via `which`)
///
/// The `base_name` must be provided without an extension (e.g. "adb", "rclone").
#[instrument(level = "debug", ret, skip(custom_path))]
pub fn resolve_binary_path(custom_path: Option<&str>, base_name: &str) -> Result<PathBuf> {
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
        warn!(raw, "Custom path did not resolve; looking for binary by name");
    }

    // 2) Next to current executable (bundled)
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
        && let Some(found) = try_in_dir(dir, base_name)
    {
        return Ok(found);
    }

    // 3) PATH search (plain program name)
    if let Ok(found) = which::which(base_name) {
        return Ok(found);
    }

    // Nothing worked
    Err(anyhow::anyhow!(
        "{} binary not found (checked custom path, PATH, and app directory)",
        base_name
    ))
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
    // Depth-first traversal using async fs; stop at first file
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
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        // Ignore errors collecting metadata; just skip entries
        if entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false)
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

/// Like `decompress_all_7z_in_dir` but cancellable via `CancellationToken`.
/// If the token becomes cancelled, the currently-extracting entry is aborted
/// and the function returns an error.
#[instrument(skip(dir, cancel), err)]
pub async fn decompress_all_7z_in_dir_cancellable(
    dir: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    let mut rd = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        if entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false)
            && entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("7z"))
        {
            if cancel.is_cancelled() {
                debug!("Cancellation requested before starting 7z extraction");
                return Err(anyhow::Error::from(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "extraction cancelled",
                )));
            }

            let path = entry.path();
            let dest_dir = dir.to_path_buf();
            let token = cancel.clone();

            tokio::task::spawn_blocking(move || -> Result<()> {
                sevenz_rust2::decompress_file_with_extract_fn(
                    &path,
                    dest_dir,
                    |entry, reader, final_path| {
                        if token.is_cancelled() {
                            return Err(sevenz_rust2::Error::from(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "extraction cancelled",
                            )));
                        }

                        if entry.is_directory() {
                            stdfs::create_dir_all(final_path)?;
                            return Ok(true);
                        }

                        if let Some(parent) = final_path.parent() {
                            stdfs::create_dir_all(parent)?;
                        }

                        let tmp_path = final_path.with_extension("part");
                        let out = File::create(&tmp_path).map_err(sevenz_rust2::Error::from)?;
                        let mut writer = BufWriter::new(out);

                        let mut buf = [0u8; 128 * 1024];
                        loop {
                            if token.is_cancelled() {
                                drop(writer);
                                let _ = stdfs::remove_file(&tmp_path);
                                return Err(sevenz_rust2::Error::from(io::Error::new(
                                    io::ErrorKind::Interrupted,
                                    "extraction cancelled",
                                )));
                            }
                            let n = match reader.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => n,
                                Err(e) => return Err(e.into()),
                            };
                            writer.write_all(&buf[..n])?;
                        }
                        writer.flush()?;

                        rename(&tmp_path, final_path)?;
                        Ok(true)
                    },
                )
                .context("Error decompressing 7z archive")?;
                Ok(())
            })
            .await??;
        }
    }

    Ok(())
}
