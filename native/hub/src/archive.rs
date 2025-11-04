use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::LazyLock,
};

use anyhow::{Context, Result, ensure};
use tokio::{fs, process::Command as TokioCommand};
use tokio_util::sync::CancellationToken;
use tracing::{debug, instrument};

use crate::utils::resolve_binary_path;

// TODO: run resolve every time if not found
static SEVENZ_PATH: LazyLock<Option<PathBuf>> = LazyLock::new(|| resolve_7z_path().ok());

/// Resolve 7-Zip binary path for the current platform.
#[instrument(level = "debug", ret, err)]
fn resolve_7z_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[&str] = &["7za", "7z", "7zz"];
    #[cfg(target_os = "linux")]
    const CANDIDATES: &[&str] = &["7zzs", "7zz", "7za", "7z"];
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[&str] = &["7zz", "7za", "7z"];

    for name in CANDIDATES {
        if let Ok(path) = resolve_binary_path(None, name) {
            debug!(path = %path.display(), "Resolved 7-Zip binary path");
            return Ok(path);
        }
    }
    Err(anyhow::anyhow!("7-Zip binary not found (tried {:?})", CANDIDATES))
}

/// Run 7-Zip with provided args, optionally cancellable.
async fn run_7z<I, S>(args: I, cancel: Option<&CancellationToken>) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let bin = SEVENZ_PATH.clone().context("No 7-Zip binary found")?;

    let mut cmd = TokioCommand::new(&bin);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());

    let mut child = cmd.spawn().context("Failed to spawn 7-Zip process")?;
    if let Some(tok) = cancel {
        tokio::select! {
            status = child.wait() => {
                let status = status.context("Failed to wait for 7-Zip process")?;
                ensure!(status.success(), "7-Zip exited with status: {}", status);
            }
            _ = tok.cancelled() => {
                let _ = child.kill().await;
                return Err(anyhow::anyhow!(io::Error::new(io::ErrorKind::Interrupted, "extraction cancelled")));
            }
        }
    } else {
        let status = child.wait().await.context("Failed to wait for 7-Zip process")?;
        ensure!(status.success(), "7-Zip exited with status: {}", status);
    }
    Ok(())
}

/// Extract an archive into `dest_dir`.
///
/// - `password`: if provided, passes `-p<password>` to 7-Zip.
/// - `wanted`: if provided and non-empty, only extracts the listed entries.
/// - `archive` can be a regular archive file or the first segment of a
///   multi-volume archive (e.g. `file.7z.001`). 7-Zip will detect parts.
#[instrument(skip(archive, dest_dir, password, wanted, cancel), err, level = "debug")]
pub async fn decompress_archive(
    archive: &Path,
    dest_dir: &Path,
    password: Option<&str>,
    wanted: Option<&[&str]>,
    cancel: Option<CancellationToken>,
) -> Result<()> {
    let mut args: Vec<OsString> = vec![OsString::from("x"), OsString::from("-y")];

    if let Some(pass) = password {
        args.push(OsString::from(format!("-p{}", pass)));
    }

    let mut out_arg = OsString::from("-o");
    out_arg.push(dest_dir.as_os_str());
    args.push(out_arg);
    args.push(archive.as_os_str().to_os_string());

    if let Some(list) = wanted.filter(|w| !w.is_empty()) {
        for item in list {
            args.push(OsString::from(*item));
        }
    }

    run_7z(args, cancel.as_ref()).await
}

/// Decompresses all `.7z` archives found directly under `dir` into `dir`.
#[instrument(skip(dir, cancel), err)]
pub async fn decompress_all_7z_in_dir(dir: &Path, cancel: Option<CancellationToken>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        if entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false)
            && entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("7z"))
        {
            if cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
                debug!("Cancellation requested before starting 7z extraction");
                return Err(anyhow::Error::from(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "extraction cancelled",
                )));
            }
            let path = entry.path();
            debug!(path = %path.display(), "Decompressing 7z archive");
            decompress_archive(&path, dir, None, None, cancel.clone()).await?;
        }
    }
    Ok(())
}

/// Run 7-Zip and capture stdout.
async fn run_7z_capture<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let bin = SEVENZ_PATH.clone().context("No 7-Zip binary found")?;

    let output = TokioCommand::new(&bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .context("Failed to run 7-Zip")?;

    ensure!(output.status.success(), "7-Zip exited with status: {}", output.status);
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// List file paths contained in an archive using 7-Zip.
/// Returns only file entries (directories are filtered out).
pub async fn list_archive_file_paths(archive: &Path) -> Result<Vec<String>> {
    // Use technical list for easier parsing
    let out = run_7z_capture([
        OsString::from("l"),
        OsString::from("-slt"),
        archive.as_os_str().to_os_string(),
    ])
    .await?;
    Ok(parse_7z_slt_listing(&out))
}

/// Parse stdout of `7z l -slt` and return file paths (directories filtered out).
fn parse_7z_slt_listing(out: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut cur_path: Option<String> = None;
    let mut cur_is_folder: Option<bool> = None;

    for line in out.lines() {
        if let Some(p) = line.strip_prefix("Path = ") {
            // Flush previous record if any
            if let (Some(path), Some(is_folder)) = (&cur_path, cur_is_folder)
                && !is_folder
            {
                result.push(path.clone());
            }
            cur_path = Some(p.trim().to_string());
            cur_is_folder = None;
            continue;
        }
        if let Some(f) = line.strip_prefix("Folder = ") {
            let v = f.trim();
            // 7-Zip -slt prints Folder as one of: "+" (dir), "-" (file), or sometimes "Yes"/"No".
            let is_folder = match v {
                "+" => true,
                "-" => false,
                _ if v.eq_ignore_ascii_case("yes") || v.eq_ignore_ascii_case("true") => true,
                _ if v.eq_ignore_ascii_case("no") || v.eq_ignore_ascii_case("false") => false,
                _ => false,
            };
            cur_is_folder = Some(is_folder);
            continue;
        }
    }
    // Flush last entry
    if let (Some(path), Some(is_folder)) = (cur_path, cur_is_folder)
        && !is_folder
    {
        result.push(path);
    }

    result
}

/// Extract a single entry from an archive into `dest_dir`, flattening paths (7z `e`).
pub async fn extract_single_from_archive(
    archive: &Path,
    dest_dir: &Path,
    entry: &str,
) -> Result<()> {
    let mut out_arg = OsString::from("-o");
    out_arg.push(dest_dir.as_os_str());
    run_7z(
        [
            OsString::from("e"),
            OsString::from("-y"),
            out_arg,
            archive.as_os_str().to_os_string(),
            OsString::from(entry),
        ],
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::parse_7z_slt_listing;

    #[test]
    fn parse_7z_listing() {
        let sample = r#"7-Zip 25.01 (x64) : Copyright (c) 1999-2025 Igor Pavlov : 2025-08-03
 64-bit locale=en_US.UTF-8 Threads:16 OPEN_MAX:1024, ASM

Scanning the drive for archives:
1 file, 25328783 bytes (25 MiB)

Listing archive: rclone-v1.71.1-linux-amd64.zip

--
Path = rclone-v1.71.1-linux-amd64.zip
Type = zip
Physical Size = 25328783

----------
Path = rclone-v1.71.1-linux-amd64
Path = rclone-v1.71.1-linux-amd64
Folder = +
Size = 0
Packed Size = 0
Modified = 2025-09-24 20:54:58
Created = 
Accessed = 
Attributes = D drwxr-xr-x
Encrypted = -
Comment = 
CRC = 
Method = Store
Characteristics = UT:MA:1 ux
Host OS = Unix
Version = 10
Volume Index = 0
Offset = 0

Path = rclone-v1.71.1-linux-amd64/rclone.1
Folder = -
Size = 2853244
Packed Size = 704724
Modified = 2025-09-24 20:33:23
Created = 
Accessed = 
Attributes =  -rw-r--r--
Encrypted = -
Comment = 
CRC = AC5AED3F
Method = Deflate:Maximum
Characteristics = UT:MA:1 ux
Host OS = Unix
Version = 20
Volume Index = 0
Offset = 85

Path = rclone-v1.71.1-linux-amd64/README.txt
Folder = -
Size = 2588508
Packed Size = 653703
Modified = 2025-09-24 20:33:23
Created = 
Accessed = 
Attributes =  -rw-r--r--
Encrypted = -
Comment = 
CRC = E50CE049
Method = Deflate:Maximum
Characteristics = UT:MA:1 ux
Host OS = Unix
Version = 20
Volume Index = 0
Offset = 704902

Path = rclone-v1.71.1-linux-amd64/git-log.txt
Folder = -
Size = 11069
Packed Size = 4550
Modified = 2025-09-24 20:50:42
Created = 
Accessed = 
Attributes =  -rw-r--r--
Encrypted = -
Comment = 
CRC = 9735D5A6
Method = Deflate:Maximum
Characteristics = UT:MA:1 ux
Host OS = Unix
Version = 20
Volume Index = 0
Offset = 1358700

Path = rclone-v1.71.1-linux-amd64/README.html
Folder = -
Size = 3549871
Packed Size = 811067
Modified = 2025-09-24 20:33:23
Created = 
Accessed = 
Attributes =  -rw-r--r--
Encrypted = -
Comment = 
CRC = 900333F3
Method = Deflate:Maximum
Characteristics = UT:MA:1 ux
Host OS = Unix
Version = 20
Volume Index = 0
Offset = 1363346

Path = rclone-v1.71.1-linux-amd64/rclone
Folder = -
Size = 69161144
Packed Size = 23153533
Modified = 2025-09-24 20:54:58
Created = 
Accessed = 
Attributes =  -rwxr-x-x
Encrypted = -
Comment = 
CRC = 7A97B213
Method = Deflate:Maximum
Characteristics = UT:MA:1 ux
Host OS = Unix
Version = 20
Volume Index = 0
Offset = 2174509
"#;

        let files = parse_7z_slt_listing(sample);
        // Directory should not be present
        assert!(!files.iter().any(|p| p == "rclone-v1.71.1-linux-amd64"));

        // Expected files should be present
        assert!(files.iter().any(|p| p.ends_with("/rclone")));
        assert!(files.iter().any(|p| p.ends_with("/rclone.1")));
        assert!(files.iter().any(|p| p.ends_with("/README.txt")));
        assert!(files.iter().any(|p| p.ends_with("/README.html")));
        assert!(files.iter().any(|p| p.ends_with("/git-log.txt")));

        assert_eq!(files.len(), 5);
    }
}
