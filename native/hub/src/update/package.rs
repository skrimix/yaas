//! Package checks and extraction for staging.

use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};

/// Extracts the Windows package into `output` and checks its layout.
/// Only called on Windows outside of tests.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn extract_windows_zip(archive: &Path, output: &Path) -> Result<()> {
    zip::ZipArchive::new(fs::File::open(archive)?)?
        .extract(output)
        .context("Cannot extract Windows package")?;
    for name in ["yaas.exe", "hub.dll"] {
        ensure!(output.join(name).is_file(), "Missing Windows package file: {name}");
    }
    Ok(())
}

/// Extracts the macOS package into `output` with `ditto`, preserving symlinks, and
/// verifies the extracted `YAAS.app` before it may replace the installed one.
#[cfg(target_os = "macos")]
pub(super) fn extract_macos_zip(archive: &Path, output: &Path) -> Result<()> {
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
    Ok(())
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
    use std::io::Write;

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    fn zip(path: &Path, names: &[&str]) {
        let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
        for name in names {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"data").unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extraction_rejects_traversal_and_incomplete_packages() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("package.zip");
        zip(&archive, &["../escape", "yaas.exe", "hub.dll"]);
        assert!(extract_windows_zip(&archive, &root.path().join("output")).is_err());
        assert!(!root.path().join("escape").exists());

        zip(&archive, &["yaas.exe"]);
        let error = extract_windows_zip(&archive, &root.path().join("output2")).unwrap_err();
        assert!(error.to_string().contains("hub.dll"));
    }

    #[test]
    fn extraction_writes_regular_package() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("package.zip");
        zip(&archive, &["yaas.exe", "hub.dll", "data/flutter_assets/a.txt"]);
        let output = root.path().join("output");
        extract_windows_zip(&archive, &output).unwrap();
        assert_eq!(fs::read(output.join("yaas.exe")).unwrap(), b"data");
        assert_eq!(fs::read(output.join("data/flutter_assets/a.txt")).unwrap(), b"data");
    }
}
