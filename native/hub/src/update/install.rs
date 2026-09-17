use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use app_update::{
    Identity,
    install::{InstallRequest, clean_environment},
    processes::ProcessIdentity,
};

use super::release::Candidate;

pub(crate) struct Prepared {
    directory: PathBuf,
}

impl Prepared {
    pub(crate) fn launch(self) -> Result<()> {
        let mut command = Command::new(self.directory.join(helper_name()));
        let log = fs::File::create(self.directory.join("updater.log"))?;
        command
            .arg(self.directory.join("request.json"))
            .current_dir(&self.directory)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        clean_environment(&mut command);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command.spawn().context("Cannot start update helper")?;
        Ok(())
    }

    pub async fn cancel(self) {
        let _ = tokio::fs::remove_dir_all(self.directory).await;
    }
}

fn helper_name() -> &'static str {
    if cfg!(windows) { "yaas-updater.exe" } else { "yaas-updater" }
}

pub(super) fn installed_identity() -> Identity {
    Identity {
        version: env!("YAAS_APP_VERSION").into(),
        build_number: env!("YAAS_BUILD_NUMBER").parse().unwrap(),
        channel: env!("YAAS_RELEASE_CHANNEL").into(),
        commit: crate::built_info::GIT_COMMIT_HASH.unwrap_or_default().into(),
        run_number: env!("YAAS_RUN_NUMBER").parse().unwrap_or_default(),
        run_attempt: env!("YAAS_RUN_ATTEMPT").parse().unwrap_or_default(),
    }
}

pub(super) fn installation() -> Result<(PathBuf, PathBuf)> {
    ensure!(
        env!("YAAS_RELEASE_CHANNEL") != "development" && crate::built_info::PROFILE == "release",
        "Self-update installation is unavailable in development builds"
    );
    let exe = std::env::current_exe()?.canonicalize()?;
    if cfg!(target_os = "linux") {
        let appimage = PathBuf::from(
            std::env::var_os("APPIMAGE")
                .context("Only AppImage installations support self-update on Linux")?,
        );
        ensure!(
            appimage.is_absolute() && appimage.is_file(),
            "Cannot locate the installed AppImage"
        );
        app_update::package::check_destination(&appimage)?;
        let helper = crate::utils::resolve_binary_path(None, "yaas-updater")?;
        Ok((appimage, helper))
    } else if cfg!(target_os = "windows") {
        let directory = exe.parent().context("Missing application directory")?.to_path_buf();
        let helper = directory.join("yaas-updater.exe");
        ensure!(helper.is_file(), "The installed update helper is missing");
        Ok((directory, helper))
    } else if cfg!(target_os = "macos") {
        ensure!(
            !exe.components().any(|c| c.as_os_str() == "AppTranslocation"),
            "Move YAAS to a writable Applications folder before updating"
        );
        let bundle = exe
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("Cannot locate application bundle")?
            .to_path_buf();
        ensure!(
            bundle.extension().is_some_and(|x| x == "app")
                && exe == bundle.join("Contents/MacOS/YAAS"),
            "Unsupported macOS bundle layout"
        );
        let helper = bundle.join("Contents/MacOS/yaas-updater");
        ensure!(helper.is_file(), "The installed update helper is missing");
        Ok((bundle, helper))
    } else {
        anyhow::bail!("Self-update is unsupported on this platform")
    }
}

pub(super) fn unavailable_reason() -> Option<String> {
    installation()
        .and_then(|(target, _)| {
            app_update::package::check_destination(&target)?;
            let directory = if cfg!(windows) {
                target.as_path()
            } else {
                target.parent().context("Missing installation directory")?
            };
            tempfile::NamedTempFile::new_in(directory)
                .context("Installation directory is not writable")?;
            Ok(())
        })
        .err()
        .map(|e| format!("{e:#}"))
}

fn prepare_files(download: &Path, candidate: &Candidate) -> Result<Prepared> {
    app_update::verify_file(&download.join("package"), &candidate.asset).map_err(|e| {
        super::release::failure(
            crate::models::signals::update::AppUpdateErrorKind::Integrity,
            e.to_string(),
        )
    })?;
    let (target, helper) = installation()?;
    if let Some(reason) = unavailable_reason() {
        anyhow::bail!(reason)
    }
    // Also exclude startup cleanup while the request directory is being written.
    let _lock = app_update::install::lock()?;
    let requests = app_update::install::user_update_dir()?.join("requests");
    fs::create_dir_all(&requests)?;
    let directory = tempfile::Builder::new().prefix("update-").tempdir_in(requests)?;
    fs::copy(helper, directory.path().join(helper_name()))?;
    let runtime_executable = std::env::current_exe()?.canonicalize()?;
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
    let mut bundled_executables = Vec::new();
    for name in ["adb", "7za", "7zz", "7zzs", "rclone"] {
        if let Ok(path) = crate::utils::resolve_binary_path(None, name)
            && let Ok(path) = path.canonicalize()
            && (path.starts_with(&target)
                || appdir.as_ref().is_some_and(|dir| path.starts_with(dir)))
        {
            bundled_executables.push(path);
        }
    }
    let request = InstallRequest {
        package: download.join("package").canonicalize()?,
        working_directory: restart_directory(&target)?,
        target,
        expected: candidate.identity.clone(),
        parent: ProcessIdentity::current()?,
        runtime_executable,
        bundled_executables,
        appdir,
        arguments: std::env::args_os().skip(1).collect(),
    };
    request.save(&directory.path().join("request.json"))?;
    Ok(Prepared { directory: directory.keep() })
}

pub(super) async fn prepare(download: PathBuf, candidate: Candidate) -> Result<Prepared> {
    tokio::task::spawn_blocking(move || prepare_files(&download, &candidate)).await?
}

fn restart_directory(target: &Path) -> Result<PathBuf> {
    if let Some(appdir) = std::env::var_os("APPDIR") {
        if let Some(original) = std::env::var_os("OWD") {
            let original = PathBuf::from(original);
            if original.is_absolute() && original.is_dir() && !original.starts_with(&appdir) {
                return Ok(original);
            }
        }
        let current = std::env::current_dir()?;
        if current.starts_with(appdir) {
            return Ok(target.parent().context("Missing installation directory")?.to_path_buf());
        }
    }
    std::env::current_dir().context("Cannot resolve working directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_removes_preparation_but_keeps_download() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("request");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("request.json"), "request").unwrap();
        fs::write(directory.join(helper_name()), "helper").unwrap();
        let package = root.path().join("package");
        fs::write(&package, "verified download").unwrap();
        Prepared { directory: directory.clone() }.cancel().await;
        assert!(!directory.exists());
        assert_eq!(fs::read_to_string(package).unwrap(), "verified download");
    }

    #[test]
    fn failed_helper_spawn_retains_request_and_log() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("request.json"), "request").unwrap();
        let result = Prepared { directory: root.path().into() }.launch();
        assert!(result.is_err());
        assert!(root.path().join("request.json").exists());
        assert!(root.path().join("updater.log").exists());
    }
}
