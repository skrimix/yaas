//! Stages a verified update and hands off to a platform script (see `script.rs`).

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};

use super::{
    manifest::Identity,
    script::{self, ScriptKind, ScriptPlan},
};

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

/// Locates the install target: the install directory (Windows), the AppImage
/// path (Linux), or the app bundle path (macOS).
fn installation() -> Result<PathBuf> {
    ensure!(
        env!("YAAS_RELEASE_CHANNEL") != "development" && crate::built_info::PROFILE == "release",
        "Self-update installation is unavailable in development builds"
    );
    let exe = dunce::canonicalize(std::env::current_exe()?)?;
    if cfg!(target_os = "linux") {
        let appimage = PathBuf::from(
            std::env::var_os("APPIMAGE")
                .context("Only AppImage installations support self-update on Linux")?,
        );
        ensure!(
            appimage.is_absolute() && appimage.is_file(),
            "Cannot locate the installed AppImage"
        );
        Ok(appimage)
    } else if cfg!(target_os = "windows") {
        Ok(exe.parent().context("Missing application directory")?.to_path_buf())
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
        Ok(bundle)
    } else {
        anyhow::bail!("Self-update is unsupported on this platform")
    }
}

/// Directory that holds `target`; the script's working directory and the
/// relaunched app's working directory.
fn install_dir(target: &Path) -> Result<&Path> {
    if cfg!(windows) {
        Ok(target)
    } else {
        target.parent().context("Missing installation directory")
    }
}

/// Locates a writable installation.
fn writable_installation() -> Result<PathBuf> {
    let target = installation()?;
    tempfile::NamedTempFile::new_in(install_dir(&target)?)
        .context("Installation directory is not writable")?;
    Ok(target)
}

pub(super) fn unavailable_reason() -> Option<String> {
    writable_installation().err().map(|e| format!("{e:#}"))
}

/// Path of the staged payload in an update workspace; it exists once staging succeeded.
pub(super) fn staged_path(workspace: &Path) -> PathBuf {
    if cfg!(windows) {
        workspace.join("new")
    } else if cfg!(target_os = "macos") {
        workspace.join("new/YAAS.app")
    } else {
        workspace.join("package")
    }
}

/// Prepares the verified `workspace/package` for installation, so extraction and
/// signature problems surface while the app is still running.
#[cfg(target_os = "linux")]
pub(super) fn stage(workspace: &Path) -> Result<()> {
    super::package::check_appimage(&workspace.join("package"))
}

/// Prepares the verified `workspace/package` for installation, so extraction and
/// signature problems surface while the app is still running.
#[cfg(not(target_os = "linux"))]
pub(super) fn stage(workspace: &Path) -> Result<()> {
    let partial = workspace.join("new.partial");
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    #[cfg(windows)]
    super::package::extract_windows_zip(&workspace.join("package"), &partial)?;
    #[cfg(target_os = "macos")]
    super::package::extract_macos_zip(&workspace.join("package"), &partial)?;
    fs::rename(&partial, workspace.join("new"))?;
    Ok(())
}

/// Writes the install script into `root` and starts it, detached. The app must
/// exit right after; the script waits for that before replacing files.
pub(super) fn launch(root: &Path, workspace: &Path, portable: bool) -> Result<()> {
    let target = writable_installation()?;
    let staged = staged_path(workspace);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)?.permissions().mode() & 0o777;
        fs::set_permissions(&staged, fs::Permissions::from_mode(mode))?;
    }
    let kind = ScriptKind::host();
    let plan = ScriptPlan {
        log: root.join(super::LOG_NAME),
        workspace: workspace.to_path_buf(),
        staged,
        target: target.clone(),
        pid: std::process::id(),
        portable,
    };
    let script = root.join(kind.file_name());
    fs::write(&script, script::render(kind, &plan)).context("Cannot write update script")?;
    script_command(&script, install_dir(&target)?).spawn().context("Cannot start update script")?;
    Ok(())
}

/// Builds the detached command that runs the update script.
fn script_command(script: &Path, restart_dir: &Path) -> Command {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd");
        use std::os::windows::process::CommandExt;
        command.raw_arg(format!("/d /s /c \"\"{}\"\"", script.display()));
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        command.arg(script);
        // Survive even if the app exits via a terminal signal.
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        command
    };
    command
        .current_dir(restart_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    clean_environment(&mut command);
    command
}

/// Removes AppImage runtime env vars for clean script run and relaunch.
fn clean_environment(command: &mut Command) {
    if std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some() {
        for key in ["APPIMAGE", "APPDIR", "ARGV0", "OWD", "LD_LIBRARY_PATH", "LD_PRELOAD"] {
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
    use std::time::{Duration, Instant};

    use super::*;

    /// Starts a short-lived process standing in for the running app.
    fn exiting_app() -> u32 {
        let mut app = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "3", "127.0.0.1"]).stdout(Stdio::null());
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("1");
            command
        }
        .spawn()
        .unwrap();
        let pid = app.id();
        // Reap the process so the script's wait loop observes the exit.
        std::thread::spawn(move || {
            let _ = app.wait();
        });
        pid
    }

    fn wait_for(condition: impl Fn() -> bool, timeout: Duration) {
        let until = Instant::now() + timeout;
        while !condition() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn missing_script_exits_non_zero() {
        let root = tempfile::tempdir().unwrap();
        let mut child =
            script_command(&root.path().join("missing.sh"), root.path()).spawn().unwrap();
        // /bin/sh and cmd start fine; the missing script must not succeed.
        assert!(!child.wait().unwrap().success());
    }

    #[test]
    fn failed_staging_leaves_no_payload() {
        let workspace = tempfile::tempdir().unwrap();
        fs::write(workspace.path().join("package"), "not a valid package").unwrap();
        assert!(stage(workspace.path()).is_err());
        if !cfg!(target_os = "linux") {
            assert!(!staged_path(workspace.path()).exists());
        }
    }

    /// Runs a generated Linux script against a fixture installation.
    #[cfg(unix)]
    #[test]
    fn linux_script_replaces_appimage_relaunches_and_cleans_up() {
        use std::{io::Write as _, os::unix::fs::PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install dir");
        let updates = root.path().join("updates");
        let workspace = updates.join("0123");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        let target = install.join("YAAS.AppImage");
        fs::write(&target, "old version").unwrap();
        let marker = root.path().join("relaunched");
        // The staged package: a fake AppImage (a script) that leaves a marker when run.
        let staged = workspace.join("package");
        let mut file = fs::File::create(&staged).unwrap();
        writeln!(file, "#!/bin/sh\necho relaunched > \"{}\"", marker.display()).unwrap();
        drop(file);
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o750)).unwrap();

        let plan = ScriptPlan {
            log: updates.join("update.log"),
            workspace: workspace.clone(),
            staged,
            target: target.clone(),
            pid: exiting_app(),
            portable: true,
        };
        let script = updates.join("update.sh");
        fs::write(&script, script::render(ScriptKind::Linux, &plan)).unwrap();
        let status = script_command(&script, &install).spawn().unwrap().wait().unwrap();
        assert!(status.success());

        // The relaunched "app" runs asynchronously; wait for its marker.
        wait_for(|| marker.exists(), Duration::from_secs(10));
        assert!(marker.exists(), "new app was not relaunched");
        assert!(fs::read_to_string(&target).unwrap().starts_with("#!/bin/sh"));
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o750);
        assert!(!workspace.exists(), "workspace not cleaned up");
        assert!(!script.exists(), "script did not delete itself");
        assert!(!install.join("YAAS.AppImage.yaas-update").exists());
        assert!(fs::read_to_string(updates.join("update.log")).unwrap().contains("Starting"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_script_replaces_files_and_preserves_extras() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("installed dir");
        fs::create_dir_all(install.join("_portable_data")).unwrap();
        fs::write(install.join("yaas.exe"), "old").unwrap();
        fs::write(install.join("hub.dll"), "old").unwrap();
        fs::write(install.join("obsolete.dll"), "keep").unwrap();
        fs::write(install.join("_portable_data/settings.json"), "settings").unwrap();
        let updates = root.path().join("updates");
        let workspace = updates.join("0123");
        let staged = workspace.join("new");
        fs::create_dir_all(&staged).unwrap();
        // The new yaas.exe must be a real executable for the relaunch step.
        fs::copy(r"C:\Windows\System32\whoami.exe", staged.join("yaas.exe")).unwrap();
        fs::write(staged.join("hub.dll"), "new").unwrap();
        fs::write(workspace.join("package"), "zip").unwrap();

        let plan = ScriptPlan {
            log: updates.join("update.log"),
            workspace: workspace.clone(),
            staged,
            target: install.clone(),
            pid: exiting_app(),
            portable: true,
        };
        let script = updates.join("update.bat");
        fs::write(&script, script::render(ScriptKind::Windows, &plan)).unwrap();
        let mut child = script_command(&script, &install).spawn().unwrap();

        // The script self-deletes on success; poll for completion.
        wait_for(|| !script.exists(), Duration::from_secs(90));
        let _ = child.kill();
        assert!(!script.exists(), "script did not delete itself");
        assert!(!workspace.exists(), "workspace not cleaned up");
        assert!(updates.join("update.log").exists());
        assert!(fs::metadata(install.join("yaas.exe")).unwrap().len() > 1000);
        assert_eq!(fs::read(install.join("hub.dll")).unwrap(), b"new");
        // Files absent from the package are preserved, including portable data.
        assert_eq!(fs::read_to_string(install.join("obsolete.dll")).unwrap(), "keep");
        assert_eq!(
            fs::read_to_string(install.join("_portable_data/settings.json")).unwrap(),
            "settings"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_staging_verifies_signed_bundle_and_script_replaces_it() {
        let root = tempfile::tempdir().unwrap();
        let updates = root.path().join("updates");
        let workspace = updates.join("0123");
        fs::create_dir_all(&workspace).unwrap();

        // Build a minimal signed YAAS.app as the downloaded "new" version.
        let source = root.path().join("source/YAAS.app");
        let macos = source.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::write(
            source.join("Contents/Info.plist"),
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<plist version="1.0"><dict>"#,
                r#"<key>CFBundleExecutable</key><string>YAAS</string>"#,
                r#"<key>CFBundlePackageType</key><string>APPL</string>"#,
                r#"</dict></plist>"#,
            ),
        )
        .unwrap();
        fs::write(macos.join("YAAS"), "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(macos.join("YAAS"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        // A framework symlink, to verify ditto preserves links.
        let framework = source.join("Contents/Frameworks/F.framework");
        fs::create_dir_all(framework.join("Versions/A")).unwrap();
        fs::write(framework.join("Versions/A/F"), "lib").unwrap();
        std::os::unix::fs::symlink("A", framework.join("Versions/Current")).unwrap();
        std::os::unix::fs::symlink("Versions/Current/F", framework.join("F")).unwrap();
        assert!(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--deep", "-s", "-"])
                .arg(&source)
                .status()
                .unwrap()
                .success(),
            "ad-hoc signing failed"
        );
        assert!(
            Command::new("/usr/bin/ditto")
                .args(["-c", "-k"])
                .arg(&source)
                .arg(workspace.join("package"))
                .status()
                .unwrap()
                .success(),
            "zip creation failed"
        );

        // Staging (runs right after the download in the real flow): extraction + verification.
        stage(&workspace).unwrap();
        let staged = staged_path(&workspace);
        assert!(
            fs::symlink_metadata(staged.join("Contents/Frameworks/F.framework/F"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "ditto must preserve framework symlinks"
        );

        // Existing installation with a stale file that must disappear.
        let target = root.path().join("Applications/YAAS.app");
        fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
        fs::write(target.join("Contents/MacOS/YAAS"), "old").unwrap();
        fs::write(target.join("stale-file"), "stale").unwrap();

        let plan = ScriptPlan {
            log: updates.join("update.log"),
            workspace: workspace.clone(),
            staged,
            target: target.clone(),
            pid: exiting_app(),
            portable: false,
        };
        let script = updates.join("update.sh");
        fs::write(&script, script::render(ScriptKind::MacOs, &plan)).unwrap();
        let status = script_command(&script, root.path()).spawn().unwrap().wait().unwrap();
        assert!(status.success());

        assert!(fs::read_to_string(target.join("Contents/MacOS/YAAS")).unwrap().contains("exit 0"));
        assert!(!target.join("stale-file").exists(), "old bundle must be fully replaced");
        assert!(target.join("Contents/Frameworks/F.framework/F").exists());
        assert!(!root.path().join("Applications/YAAS.app.yaas-update").exists());
        assert!(!workspace.exists(), "workspace not cleaned up");
        assert!(!script.exists(), "script did not delete itself");
    }
}
