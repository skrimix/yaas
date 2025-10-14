use std::{
    env,
    error::Error,
    fs as stdfs,
    fs::{File, rename},
    io,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
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
                debug!(proxy = &result, "Got system proxy");
                return Some(result);
            }
        }
        Err(e) => {
            error!(error = &e as &dyn Error, "Failed to get system proxy");
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
        warn!(raw, "Custom path did not resolve, looking for binary by name");
    }

    // 2) Next to current executable (bundled)
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
        && let Some(found) = try_in_dir(dir, base_name)
    {
        return Ok(found);
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

/// Removes a specific child directory if present. Errors are ignored.
pub async fn remove_child_dir_if_exists(parent: &Path, child: &str) {
    let target = parent.join(child);
    if target.exists() {
        let _ = fs::remove_dir_all(target).await;
    }
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
///
/// Stops at the earliest safe point if `cancel` is triggered, cleaning up any
/// partial output file and returning an `Interrupted` error.
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
                    entry_writer_extract_fn(Some(&token)),
                )
                .context("Error decompressing 7z archive")?;
                Ok(())
            })
            .await??;
        }
    }

    Ok(())
}

/// A reader that concatenates multiple files (e.g., `*.7z.001`, `*.7z.002`, ...)
/// into a single Read+Seek stream.
struct MultiVolumeReader {
    parts: Vec<BufReader<File>>,
    sizes: Vec<u64>,
    total: u64,
    cursor: u64,
}

impl MultiVolumeReader {
    /// Open all part files in order.
    fn open(paths: impl IntoIterator<Item = PathBuf>) -> io::Result<Self> {
        let mut parts = Vec::new();
        let mut sizes = Vec::new();
        for p in paths {
            let f = File::open(&p)?;
            let size = f.metadata()?.len();
            parts.push(BufReader::new(f));
            sizes.push(size);
        }
        let total = sizes.iter().sum();
        Ok(Self { parts, sizes, total, cursor: 0 })
    }

    /// Translate a global offset into (part_index, local_offset) and position
    /// the corresponding underlying file at that local offset.
    fn seek_within(&mut self, mut off: u64) -> io::Result<(usize, u64)> {
        for (i, &sz) in self.sizes.iter().enumerate() {
            if off < sz {
                // Use BufReader's Seek to keep its internal buffer consistent.
                self.parts[i].seek(SeekFrom::Start(off))?;
                return Ok((i, off));
            }
            off -= sz;
        }
        Ok((self.parts.len(), 0)) // EOF position
    }
}

impl Read for MultiVolumeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cursor >= self.total || buf.is_empty() {
            return Ok(0);
        }
        let mut written = 0usize;
        while written < buf.len() && self.cursor < self.total {
            let (idx, off) = self.seek_within(self.cursor)?;
            if idx >= self.parts.len() {
                break;
            }
            let remaining_in_part = self.sizes[idx] - off;
            let to_read = (buf.len() - written).min(remaining_in_part as usize);
            let n = self.parts[idx].read(&mut buf[written..written + to_read])?;
            if n == 0 {
                break;
            }
            written += n;
            self.cursor += n as u64;
        }
        Ok(written)
    }
}

impl Seek for MultiVolumeReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::End(n) => {
                let t = self.total as i128 + n as i128;
                if t < 0 {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
                }
                t as u64
            }
            SeekFrom::Current(n) => {
                let t = self.cursor as i128 + n as i128;
                if t < 0 {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
                }
                t as u64
            }
        };
        self.cursor = new.min(self.total);
        Ok(self.cursor)
    }
}

/// Discover multipart archive parts based on the first segment path.
///
/// Supports both `name.7z.001` and `name.001` styles by preserving the prefix
/// before the last extension and appending `.NNN` while files exist.
fn gather_multipart_parts(first: &Path) -> Result<Vec<PathBuf>> {
    ensure!(first.is_file(), "First part must be an existing file");
    let name = first.file_name().and_then(|s| s.to_str()).context("Invalid file name")?;

    let (prefix, _suffix) = name.rsplit_once('.').context("Expected numeric suffix like .001")?;

    let dir = first.parent().unwrap_or_else(|| Path::new("."));
    let mut n = 1u32;
    let mut out = Vec::new();
    loop {
        let candidate = dir.join(format!("{prefix}.{n:03}"));
        if candidate.is_file() {
            out.push(candidate);
            n += 1;
        } else {
            break;
        }
    }
    if out.is_empty() { Err(anyhow::anyhow!("No multipart segments found")) } else { Ok(out) }
}

/// Extract a multi-volume 7z archive given the path to the first segment
/// (e.g., `file.7z.001`) into `dest_dir`.
#[instrument(skip(first_part, dest_dir), err, level = "debug")]
pub async fn decompress_multipart_7z(first_part: &Path, dest_dir: &Path) -> Result<()> {
    let first = first_part.to_path_buf();
    let out_dir = dest_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let parts = gather_multipart_parts(&first)
            .with_context(|| format!("Failed to gather parts for {}", first.display()))?;
        let reader = MultiVolumeReader::open(parts)
            .with_context(|| format!("Failed to open multipart reader for {}", first.display()))?;
        sevenz_rust2::decompress_with_extract_fn(reader, out_dir, entry_writer_extract_fn(None))
            .context("Error decompressing multipart 7z archive")?;
        Ok(())
    })
    .await??;
    Ok(())
}

/// Extract a multi-volume 7z archive with cancellation support.
///
/// Stops at the earliest safe point if `cancel` is triggered, cleaning up any
/// partial output file and returning an `Interrupted` error.
#[instrument(skip(first_part, dest_dir, cancel), err, level = "debug")]
pub async fn decompress_multipart_7z_cancellable(
    first_part: &Path,
    dest_dir: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    let first = first_part.to_path_buf();
    let out_dir = dest_dir.to_path_buf();
    let token = cancel.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        if token.is_cancelled() {
            return Err(anyhow::Error::from(io::Error::new(
                io::ErrorKind::Interrupted,
                "extraction cancelled",
            )));
        }

        let parts = gather_multipart_parts(&first)
            .with_context(|| format!("gather parts for {}", first.display()))?;
        let reader = MultiVolumeReader::open(parts)
            .with_context(|| format!("open multipart reader for {}", first.display()))?;

        sevenz_rust2::decompress_with_extract_fn(
            reader,
            out_dir,
            entry_writer_extract_fn(Some(&token)),
        )
        .context("error decompressing multipart 7z archive")?;
        Ok(())
    })
    .await??;
    Ok(())
}

/// Extract a multi-volume encrypted 7z archive with cancellation support.
///
/// Stops at the earliest safe point if `cancel` is triggered, cleaning up any
/// partial output file and returning an `Interrupted` error.
#[instrument(skip(first_part, dest_dir, password, cancel), err, level = "debug")]
pub async fn decompress_multipart_7z_cancellable_with_password(
    first_part: &Path,
    dest_dir: &Path,
    password: sevenz_rust2::Password,
    cancel: CancellationToken,
) -> Result<()> {
    let first = first_part.to_path_buf();
    let out_dir = dest_dir.to_path_buf();
    let token = cancel.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        if token.is_cancelled() {
            return Err(anyhow::Error::from(io::Error::new(
                io::ErrorKind::Interrupted,
                "extraction cancelled",
            )));
        }

        let parts = gather_multipart_parts(&first)
            .with_context(|| format!("gather parts for {}", first.display()))?;
        let reader = MultiVolumeReader::open(parts)
            .with_context(|| format!("open multipart reader for {}", first.display()))?;

        sevenz_rust2::decompress_with_extract_fn_and_password(
            reader,
            out_dir,
            password,
            entry_writer_extract_fn(Some(&token)),
        )
        .context("error decompressing encrypted multipart 7z archive")?;
        Ok(())
    })
    .await??;
    Ok(())
}

/// Build an extract-fn closure that writes entries to disk using a temporary
/// `*.part` file, optionally observing a `CancellationToken`.
fn entry_writer_extract_fn<'a>(
    token: Option<&'a CancellationToken>,
) -> impl FnMut(
    &sevenz_rust2::ArchiveEntry,
    &mut dyn Read,
    &PathBuf,
) -> Result<bool, sevenz_rust2::Error>
+ 'a {
    move |entry, reader, final_path| {
        if let Some(tok) = token
            && tok.is_cancelled()
        {
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
            if let Some(tok) = token
                && tok.is_cancelled()
            {
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
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use tempfile::tempdir;

    use super::*;

    fn write_file(path: &Path, data: &[u8]) {
        std::fs::write(path, data).expect("write test file");
    }

    #[test]
    fn gather_parts_styles_and_order() {
        let td = tempdir().unwrap();
        let dir = td.path();

        let f1 = dir.join("foo.7z.001");
        let f2 = dir.join("foo.7z.002");
        write_file(&f1, b"A");
        write_file(&f2, b"B");

        let parts = gather_multipart_parts(&f1).expect("should gather foo parts");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].file_name().unwrap().to_string_lossy(), "foo.7z.001");
        assert_eq!(parts[1].file_name().unwrap().to_string_lossy(), "foo.7z.002");

        let b1 = dir.join("bar.001");
        let b2 = dir.join("bar.002");
        write_file(&b1, b"X");
        write_file(&b2, b"Y");
        let parts_b = gather_multipart_parts(&b1).expect("should gather bar parts");
        assert_eq!(parts_b.len(), 2);
        assert_eq!(parts_b[0].file_name().unwrap().to_string_lossy(), "bar.001");
        assert_eq!(parts_b[1].file_name().unwrap().to_string_lossy(), "bar.002");
    }

    #[test]
    fn gather_parts_not_found() {
        let td = tempdir().unwrap();
        let dir = td.path();
        let missing = dir.join("missing.001");
        let err = gather_multipart_parts(&missing).unwrap_err();
        assert!(err.to_string().contains("exist"));
    }

    #[test]
    fn multivolume_read_and_seek() {
        // Prepare two parts with deterministic data 0..10
        let td = tempdir().unwrap();
        let dir = td.path();
        let p1 = dir.join("data.7z.001");
        let p2 = dir.join("data.7z.002");

        // bytes 0..5 and 5..10
        let part1: Vec<u8> = (0u8..5u8).collect();
        let part2: Vec<u8> = (5u8..10u8).collect();
        write_file(&p1, &part1);
        write_file(&p2, &part2);

        let mut reader = MultiVolumeReader::open(vec![p1, p2]).expect("open multivolume");

        // Read 3 bytes -> 0,1,2
        let mut buf = [0u8; 3];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 3);
        assert_eq!(&buf, &[0, 1, 2]);

        // Read 4 bytes -> 3,4,5,6 (crosses boundary at 5)
        let mut buf2 = [0u8; 4];
        let n2 = reader.read(&mut buf2).unwrap();
        assert_eq!(n2, 4);
        assert_eq!(&buf2, &[3, 4, 5, 6]);

        // Read the rest -> 7,8,9
        let mut rest = [0u8; 16];
        let n3 = reader.read(&mut rest).unwrap();
        assert_eq!(n3, 3);
        assert_eq!(&rest[..3], &[7, 8, 9]);

        // EOF -> 0
        let mut z = [0u8; 1];
        assert_eq!(reader.read(&mut z).unwrap(), 0);

        // Seek to 7 and read 2 -> 7,8
        reader.seek(SeekFrom::Start(7)).unwrap();
        let mut s = [0u8; 2];
        let n4 = reader.read(&mut s).unwrap();
        assert_eq!(n4, 2);
        assert_eq!(&s, &[7, 8]);

        // Seek to start and read 2 -> 0,1
        reader.seek(SeekFrom::Start(0)).unwrap();
        let mut t = [0u8; 2];
        reader.read_exact(&mut t).unwrap();
        assert_eq!(&t, &[0, 1]);

        // Seek from end -1 and read 1 -> 9
        reader.seek(SeekFrom::End(-1)).unwrap();
        let mut last = [0u8; 1];
        reader.read_exact(&mut last).unwrap();
        assert_eq!(last[0], 9);
    }
}
