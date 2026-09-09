//! Package layout checks and ZIP extraction for staged updates.

use crate::Identity;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const INVENTORY: &str = "yaas-package.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub schema_version: u32,
    pub identity: Identity,
    pub files: Vec<String>,
}

pub fn relative_path(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains(['\\', ':']) && !name.starts_with('/'),
        "Invalid package path: {name}"
    );
    let path = PathBuf::from(name);
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "Invalid package path: {name}"
    );
    ensure!(
        name.split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.ends_with(['.', ' '])),
        "Invalid package path: {name}"
    );
    for part in name.split('/') {
        let base = part.split('.').next().unwrap().to_ascii_uppercase();
        ensure!(
            !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && base.as_bytes()[3].is_ascii_digit()),
            "Reserved package path: {name}"
        );
    }
    Ok(path)
}

impl Inventory {
    pub fn read(root: &Path) -> Result<Self> {
        let inventory: Self = serde_json::from_slice(&fs::read(root.join(INVENTORY))?)
            .context("Invalid package inventory")?;
        ensure!(
            inventory.schema_version == 1,
            "Unsupported package inventory"
        );
        inventory.identity.validate()?;
        let mut seen = BTreeSet::new();
        for name in &inventory.files {
            relative_path(name)?;
            ensure!(seen.insert(name.to_lowercase()), "Duplicate package path");
            ensure!(
                !name
                    .split('/')
                    .any(|p| p.eq_ignore_ascii_case("_portable_data")
                        || p.starts_with(".yaas-update")),
                "Package overlaps user data"
            );
        }
        for required in [INVENTORY, "yaas.exe", "hub.dll", "yaas-updater.exe"] {
            ensure!(
                inventory.files.iter().any(|p| p == required),
                "Missing package file: {required}"
            );
        }
        Ok(inventory)
    }
}

/// Rejects symlinks in an existing destination path, including its parents.
pub fn check_destination(path: &Path) -> Result<()> {
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

/// Extracts a release ZIP, preserving macOS modes and contained relative symlinks.
pub fn extract_zip(archive: &Path, output: &Path, macos: bool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    let mut directories = Vec::new();
    fs::create_dir_all(output)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().trim_end_matches('/').to_string();
        let relative = relative_path(&name)?;
        ensure!(seen.insert(name.to_lowercase()), "Duplicate archive path");
        if macos {
            ensure!(
                relative.starts_with("YAAS.app"),
                "Unexpected macOS ZIP layout"
            );
        }
        let path = output.join(&relative);
        check_destination(&path)?;
        let mode = entry.unix_mode().unwrap_or(0o644);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            directories.push((path, mode));
            continue;
        }
        fs::create_dir_all(path.parent().unwrap())?;
        if mode & 0o170000 == 0o120000 {
            ensure!(macos, "Symlink in Windows package");
            let mut target = String::new();
            entry.by_ref().take(4097).read_to_string(&mut target)?;
            ensure!(
                target.len() <= 4096 && !target.is_empty() && !Path::new(&target).is_absolute(),
                "Invalid archive symlink"
            );
            let mut depth = relative.parent().unwrap().components().count();
            for component in Path::new(&target).components() {
                match component {
                    Component::Normal(_) => depth += 1,
                    Component::CurDir => {}
                    Component::ParentDir => {
                        ensure!(depth > 1, "Symlink escapes app bundle");
                        depth -= 1;
                    }
                    _ => anyhow::bail!("Invalid archive symlink"),
                }
            }
            links.push((path, target));
        } else {
            ensure!(
                mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
                "Unsupported archive entry"
            );
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            std::io::copy(&mut entry, &mut file)?;
            file.flush()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777))?;
            }
        }
    }
    // Create links after regular files so extraction never writes through a link.
    for (path, target) in links {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path)?;
        #[cfg(not(unix))]
        {
            let _ = (path, target);
            anyhow::bail!("macOS extraction requires Unix");
        }
    }
    #[cfg(unix)]
    for (directory, mode) in directories.into_iter().rev() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(mode & 0o777))?;
    }
    if macos {
        let root = output.join("YAAS.app");
        for name in [
            "Contents/Info.plist",
            "Contents/MacOS/YAAS",
            "Contents/MacOS/yaas-updater",
        ] {
            ensure!(
                root.join(name).is_file(),
                "Missing macOS bundle file: {name}"
            );
        }
        // Resolve every link after extraction, including chains, and keep it inside the bundle.
        validate_links(&root, &root.canonicalize()?)?;
    } else {
        let inventory = Inventory::read(output)?;
        let actual = regular_files(output, output)?;
        ensure!(
            actual == inventory.files.into_iter().collect(),
            "Package inventory does not match ZIP contents"
        );
    }
    Ok(())
}

fn validate_links(root: &Path, boundary: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            ensure!(
                entry.path().canonicalize()?.starts_with(boundary),
                "Symlink escapes app bundle"
            );
        } else if entry.file_type()?.is_dir() {
            validate_links(&entry.path(), boundary)?;
        }
    }
    Ok(())
}

fn regular_files(root: &Path, directory: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            files.extend(regular_files(root, &entry.path())?);
        } else {
            ensure!(entry.file_type()?.is_file(), "Unexpected package entry");
            files.insert(
                entry
                    .path()
                    .strip_prefix(root)?
                    .to_str()
                    .context("Non-UTF8 package path")?
                    .replace('\\', "/"),
            );
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_unsafe_paths() {
        for name in [
            "../yaas",
            "/yaas",
            "C:/yaas",
            "data/../x",
            "a\\b",
            "a//b",
            "a/./b",
            "x.",
        ] {
            assert!(relative_path(name).is_err(), "{name}");
        }
        assert!(relative_path("data/flutter_assets/a.txt").is_ok());
    }
}

#[cfg(test)]
mod zip_tests {
    use super::*;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn extraction_rejects_traversal_and_duplicate_files() {
        for names in [vec!["../escape"], vec!["a", "A"]] {
            let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let archive = root.path().join("package.zip");
            let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
            for name in names {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(b"data").unwrap();
            }
            zip.finish().unwrap();
            assert!(extract_zip(&archive, &root.path().join("output"), false).is_err());
            assert!(!root.path().join("escape").exists());
        }
    }
    #[cfg(unix)]
    #[test]
    fn macos_extraction_preserves_modes_and_framework_links() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let archive = root.path().join("package.zip");
        let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
        for name in [
            "Contents/Info.plist",
            "Contents/MacOS/YAAS",
            "Contents/MacOS/yaas-updater",
            "Contents/Frameworks/Example.framework/Versions/A/Example",
        ] {
            zip.start_file(
                format!("YAAS.app/{name}"),
                SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
            zip.write_all(b"binary").unwrap();
        }
        zip.add_symlink(
            "YAAS.app/Contents/Frameworks/Example.framework/Versions/Current",
            "A",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.add_symlink(
            "YAAS.app/Contents/Frameworks/Example.framework/Example",
            "Versions/Current/Example",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.finish().unwrap();
        let output = root.path().join("output");
        extract_zip(&archive, &output, true).unwrap();
        assert_eq!(
            fs::metadata(output.join("YAAS.app/Contents/MacOS/YAAS"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        let link = output.join("YAAS.app/Contents/Frameworks/Example.framework/Example");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(link).unwrap(), b"binary");
    }
    #[cfg(unix)]
    #[test]
    fn macos_extraction_rejects_escaping_symlinks() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let archive = root.path().join("package.zip");
        let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.add_symlink(
            "YAAS.app/Contents/escape",
            "../../outside",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.finish().unwrap();
        assert!(extract_zip(&archive, &root.path().join("output"), true).is_err());
    }
}
