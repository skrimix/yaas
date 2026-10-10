//! Package staging: ZIP extraction for Windows, `ditto` extraction for macOS.

#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::{collections::BTreeSet, fs, io::Write, path::Path};

use anyhow::{Result, ensure};

/// Rejects symlinks in an existing destination path, including its parents.
pub(super) fn check_destination(path: &Path) -> Result<()> {
    for parent in path.ancestors() {
        match fs::symlink_metadata(parent) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "Destination contains a symlink: {}",
                    parent.display()
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "Destination contains a reparse point"
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Extracts the Windows package into `output` and checks its layout.
/// Only called on Windows outside of tests.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn extract_windows_zip(archive: &Path, output: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    let mut seen = BTreeSet::new();
    fs::create_dir_all(output)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            anyhow::bail!("Invalid package path: {}", entry.name());
        };
        ensure!(seen.insert(relative.to_string_lossy().to_lowercase()), "Duplicate archive path");
        // A package must never replace portable user data or update workspaces.
        ensure!(
            !relative.components().any(|part| {
                let name = part.as_os_str().to_string_lossy();
                name.eq_ignore_ascii_case("_portable_data") || name.starts_with(".yaas-update")
            }),
            "Package overlaps user data"
        );
        let path = output.join(&relative);
        check_destination(&path)?;
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    for name in ["yaas.exe", "hub.dll"] {
        ensure!(output.join(name).is_file(), "Missing Windows package file: {name}");
    }
    Ok(())
}

/// Extracts the macOS package into `output` with `ditto`, preserving symlinks, and
/// verifies the staged bundle before it may replace the installed one.
#[cfg(target_os = "macos")]
pub(super) fn extract_macos_zip(archive: &Path, output: &Path) -> Result<PathBuf> {
    use std::process::Command;
    fs::create_dir_all(output)?;
    ensure!(
        Command::new("/usr/bin/ditto")
            .arg("-x")
            .arg("-k")
            .arg(archive)
            .arg(output)
            .status()?
            .success(),
        "Cannot extract macOS package"
    );
    let bundle = output.join("YAAS.app");
    ensure!(
        bundle.join("Contents/Info.plist").is_file()
            && bundle.join("Contents/MacOS/YAAS").is_file(),
        "Unexpected macOS bundle layout"
    );
    ensure!(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle)
            .status()?
            .success(),
        "Downloaded app signature is invalid"
    );
    Ok(bundle)
}

/// The AppImage package must look like a type-2 AppImage before it may replace
/// the installed one.
#[cfg(target_os = "linux")]
pub(super) fn check_appimage(package: &Path) -> Result<()> {
    use std::io::Read;
    let mut bytes = [0; 11];
    fs::File::open(package)?.read_exact(&mut bytes)?;
    ensure!(
        bytes.starts_with(b"\x7fELF") && bytes.get(8..11) == Some(b"AI\x02"),
        "Downloaded file is not an AppImage"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    #[test]
    fn extraction_rejects_traversal_and_duplicate_files() {
        for names in [vec!["../escape"], vec!["a", "A"], vec!["_portable_data/settings.json"]] {
            let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let archive = root.path().join("package.zip");
            let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
            for name in names {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(b"data").unwrap();
            }
            zip.finish().unwrap();
            let error = extract_windows_zip(&archive, &root.path().join("output")).unwrap_err();
            assert!(!error.to_string().contains("Missing Windows package file"));
            assert!(!root.path().join("escape").exists());
        }
    }

    #[test]
    fn extraction_writes_regular_package() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("package.zip");
        let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
        for name in ["yaas.exe", "hub.dll", "data/flutter_assets/a.txt"] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"data").unwrap();
        }
        zip.finish().unwrap();
        let output = root.path().join("output");
        extract_windows_zip(&archive, &output).unwrap();
        assert_eq!(fs::read(output.join("yaas.exe")).unwrap(), b"data");
        assert_eq!(fs::read(output.join("data/flutter_assets/a.txt")).unwrap(), b"data");
    }
}
