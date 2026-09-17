//! Installs a verified package and starts the application.

use crate::{
    Identity,
    package::{check_destination, extract_zip},
    processes::ProcessIdentity,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Everything the updater needs after the app exits.
#[derive(Debug, Serialize, Deserialize)]
pub struct InstallRequest {
    pub package: PathBuf,
    pub target: PathBuf,
    pub expected: Identity,
    pub parent: ProcessIdentity,
    pub runtime_executable: PathBuf,
    pub bundled_executables: Vec<PathBuf>,
    pub appdir: Option<PathBuf>,
    pub arguments: Vec<OsString>,
    pub working_directory: PathBuf,
}

pub fn user_update_dir() -> Result<PathBuf> {
    let name = if cfg!(target_os = "macos") {
        "io.github.skrimix.yaas"
    } else {
        "YAAS"
    };
    Ok(dirs::data_dir()
        .context("Cannot locate user data directory")?
        .join(name)
        .join("updates"))
}

/// Holds the single updater slot for this user, including portable installations.
pub fn lock() -> Result<fs::File> {
    let root = user_update_dir()?;
    fs::create_dir_all(&root)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("updater.lock"))?;
    file.try_lock().context("Another YAAS updater is running")?;
    Ok(file)
}

impl InstallRequest {
    pub fn read(path: &Path) -> Result<Self> {
        serde_json::from_slice(&fs::read(path)?).context("Cannot read update request")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        fs::write(path, serde_json::to_vec_pretty(self)?).context("Cannot write update request")
    }

    pub fn executable(&self) -> PathBuf {
        if cfg!(windows) {
            self.target.join("yaas.exe")
        } else if cfg!(target_os = "macos") {
            self.target.join("Contents/MacOS/YAAS")
        } else {
            self.target.clone()
        }
    }

    pub fn install(&self) -> Result<()> {
        check_destination(&self.target)?;
        let parent = if cfg!(windows) {
            self.target.as_path()
        } else {
            self.target
                .parent()
                .context("Missing installation directory")?
        };
        let workspace = tempfile::Builder::new()
            .prefix(".yaas-update-")
            .tempdir_in(parent)?;
        let staged = workspace.path().join("new");
        if cfg!(target_os = "linux") {
            fs::copy(&self.package, &staged)?;
            let mut bytes = [0; 64];
            fs::File::open(&staged)?.read_exact(&mut bytes)?;
            ensure!(
                bytes.starts_with(b"\x7fELF") && bytes.get(8..11) == Some(b"AI\x02"),
                "Downloaded file is not an AppImage"
            );
            fs::set_permissions(&staged, fs::metadata(&self.target)?.permissions())?;
            fs::rename(staged, &self.target).context("Cannot replace AppImage")?;
        } else {
            extract_zip(&self.package, &staged, cfg!(target_os = "macos"))?;
            if cfg!(windows) {
                overwrite_directory(&staged, &self.target)?;
            } else {
                let bundle = staged.join("YAAS.app");
                let identity: Identity = serde_json::from_slice(&fs::read(
                    bundle.join("Contents/Resources/yaas-build.json"),
                )?)?;
                ensure!(
                    identity == self.expected,
                    "Downloaded bundle identity mismatch"
                );
                #[cfg(target_os = "macos")]
                ensure!(
                    Command::new("/usr/bin/codesign")
                        .args(["--verify", "--deep", "--strict"])
                        .arg(&bundle)
                        .status()?
                        .success(),
                    "Downloaded app signature is invalid"
                );
                fs::remove_dir_all(&self.target).context("Cannot remove old app bundle")?;
                fs::rename(bundle, &self.target).context("Cannot install app bundle")?;
            }
        }
        Ok(())
    }

    pub fn launch(&self) -> Result<()> {
        let mut command = Command::new(self.executable());
        command
            .args(&self.arguments)
            .current_dir(&self.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        clean_environment(&mut command);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command.spawn().context("Cannot launch YAAS")?;
        Ok(())
    }
}

fn overwrite_directory(source: &Path, destination: &Path) -> Result<()> {
    check_destination(destination)?;
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        check_destination(&target)?;
        if entry.file_type()?.is_dir() {
            overwrite_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("Cannot replace {}", target.display()))?;
        }
    }
    Ok(())
}

/// Removes leftover updater copies from successful runs. Failed requests are retained.
pub fn cleanup() -> Result<()> {
    let Ok(_lock) = lock() else { return Ok(()) };
    let requests = user_update_dir()?.join("requests");
    if !requests.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(requests)? {
        let path = entry?.path();
        if path.is_dir() && !path.join("request.json").exists() {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}
pub fn clean_environment(command: &mut Command) {
    if std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some() {
        for key in [
            "APPIMAGE",
            "APPDIR",
            "ARGV0",
            "OWD",
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
        ] {
            command.env_remove(key);
        }
        if let Some(path) = std::env::var_os("PATH") {
            let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
            let paths = std::env::split_paths(&path)
                .filter(|p| !appdir.as_ref().is_some_and(|a| p.starts_with(a)));
            if let Ok(path) = std::env::join_paths(paths) {
                command.env("PATH", path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_preserves_files_absent_from_package_and_portable_data() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("new");
        let target = root.path().join("installed");
        fs::create_dir_all(source.join("data")).unwrap();
        fs::create_dir_all(target.join("_portable_data")).unwrap();
        fs::write(source.join("yaas.exe"), "new").unwrap();
        fs::write(source.join("data/asset"), "asset").unwrap();
        fs::write(target.join("yaas.exe"), "old").unwrap();
        fs::write(target.join("obsolete.dll"), "keep").unwrap();
        fs::write(target.join("_portable_data/settings.json"), "settings").unwrap();
        overwrite_directory(&source, &target).unwrap();
        assert_eq!(fs::read_to_string(target.join("yaas.exe")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(target.join("data/asset")).unwrap(),
            "asset"
        );
        assert_eq!(
            fs::read_to_string(target.join("obsolete.dll")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(target.join("_portable_data/settings.json")).unwrap(),
            "settings"
        );
    }

    #[cfg(unix)]
    #[test]
    fn overwrite_refuses_destination_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("new");
        let target = root.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, "keep").unwrap();
        fs::write(source.join("file"), "new").unwrap();
        std::os::unix::fs::symlink(&outside, target.join("file")).unwrap();
        assert!(overwrite_directory(&source, &target).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "keep");
    }
}
