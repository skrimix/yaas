//! Stages a verified update and hands off to a platform script (see `script.rs`).

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use tracing::{info, warn};

use super::{
    manifest::{self, Identity},
    release::Candidate,
    script::{self, Payload, ScriptKind, ScriptPlan},
};
use crate::models::signals::update::AppUpdateErrorKind;

/// Age after which an install lock is assumed to be left by an interrupted script.
const STALE_LOCK_AGE: Duration = Duration::from_secs(30 * 60);

/// A staged update script, ready to run as soon as the app exits.
pub(crate) struct Prepared {
    staging: PathBuf,
    script: PathBuf,
    restart_dir: PathBuf,
    /// Directories whose running executables would lock or pin the installation.
    process_roots: Vec<PathBuf>,
}

impl Prepared {
    /// Stops other processes running from the installation (bundled ADB server,
    /// archive tools, other YAAS instances), then starts the platform script,
    /// detached.
    pub(crate) async fn launch(self) -> Result<()> {
        let roots = self.process_roots.clone();
        tokio::task::spawn_blocking(move || stop_processes_in(&roots, std::process::id())).await?;
        script_command(&self.script, &self.restart_dir)
            .spawn()
            .context("Cannot start update script")?;
        Ok(())
    }

    pub async fn cancel(self) {
        let _ = tokio::fs::remove_dir_all(&self.staging).await;
        let _ = tokio::fs::remove_file(&self.script).await;
    }
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

/// Locates the install target: the install directory (Windows), the AppImage
/// path (Linux), or the app bundle path (macOS).
pub(super) fn installation() -> Result<PathBuf> {
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
        super::package::check_destination(&appimage)?;
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

pub(super) fn unavailable_reason() -> Option<String> {
    installation()
        .and_then(|target| {
            super::package::check_destination(&target)?;
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

pub(super) async fn prepare(
    root: PathBuf,
    download: PathBuf,
    candidate: Candidate,
    portable: bool,
) -> Result<Prepared> {
    tokio::task::spawn_blocking(move || prepare_files(&root, &download, &candidate, portable))
        .await?
}

fn prepare_files(
    root: &Path,
    download: &Path,
    candidate: &Candidate,
    portable: bool,
) -> Result<Prepared> {
    let package = download.join("package");
    manifest::verify_file(&package, &candidate.asset)
        .map_err(|e| super::release::failure(AppUpdateErrorKind::Integrity, e.to_string()))?;
    let target = installation()?;
    if let Some(reason) = unavailable_reason() {
        anyhow::bail!(reason)
    }
    check_install_lock(&script::lock_path(ScriptKind::host(), &target), STALE_LOCK_AGE)?;
    prepare_in(root, &candidate.info.candidate_id, &package, target, portable)
}

/// Stages `package` and writes the update script. Nothing is left behind on failure.
fn prepare_in(
    root: &Path,
    candidate_id: &str,
    package: &Path,
    target: PathBuf,
    portable: bool,
) -> Result<Prepared> {
    let kind = ScriptKind::host();
    let staging = root.join(format!("staging-{candidate_id}"));
    let script = root.join(kind.file_name(candidate_id));
    if staging.exists() {
        fs::remove_dir_all(&staging).context("Cannot clear previous staging directory")?;
    }
    fs::create_dir_all(&staging)?;
    let result = (|| -> Result<Prepared> {
        let staged = stage(package, &staging, &target)?;
        let plan = ScriptPlan {
            staging: staging.clone(),
            package: dunce::canonicalize(package)?,
            payload: staged.payload,
            target,
            pid: std::process::id(),
            portable,
        };
        fs::write(&script, script::render(kind, &plan)?).context("Cannot write update script")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        }
        Ok(Prepared {
            staging: staging.clone(),
            script: script.clone(),
            restart_dir: staged.restart_dir,
            process_roots: staged.process_roots,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&script);
    }
    result
}

/// Platform-specific result of staging a package.
struct Staged {
    payload: Payload,
    /// Working directory for the script and the relaunched app.
    restart_dir: PathBuf,
    /// See [`Prepared::process_roots`].
    process_roots: Vec<PathBuf>,
}

#[cfg(target_os = "windows")]
fn stage(package: &Path, staging: &Path, target: &Path) -> Result<Staged> {
    let extracted = staging.join("new");
    super::package::extract_windows_zip(package, &extracted)?;
    Ok(Staged {
        payload: Payload::Directory(extracted),
        restart_dir: target.to_path_buf(),
        process_roots: vec![target.to_path_buf()],
    })
}

#[cfg(target_os = "linux")]
fn stage(package: &Path, _staging: &Path, target: &Path) -> Result<Staged> {
    use std::os::unix::fs::PermissionsExt;

    super::package::check_appimage(package)?;
    Ok(Staged {
        payload: Payload::AppImage { mode: fs::metadata(target)?.permissions().mode() & 0o777 },
        restart_dir: target.parent().context("Missing installation directory")?.to_path_buf(),
        process_roots: std::env::var_os("APPDIR").map(PathBuf::from).into_iter().collect(),
    })
}

#[cfg(target_os = "macos")]
fn stage(package: &Path, staging: &Path, target: &Path) -> Result<Staged> {
    let bundle = super::package::extract_macos_zip(package, staging)?;
    Ok(Staged {
        payload: Payload::Bundle(bundle),
        restart_dir: target.parent().context("Missing installation directory")?.to_path_buf(),
        process_roots: vec![target.to_path_buf()],
    })
}

/// Fails while another update script holds the install lock. Locks older than
/// `stale_after` were left by an interrupted script and are removed.
fn check_install_lock(lock: &Path, stale_after: Duration) -> Result<()> {
    let Ok(metadata) = fs::metadata(lock) else { return Ok(()) };
    let age = metadata.modified().ok().and_then(|m| m.elapsed().ok()).unwrap_or(Duration::MAX);
    ensure!(age >= stale_after, "Another YAAS update is being installed");
    fs::remove_dir(lock).context("Cannot remove stale update lock")
}

/// Kills processes other than `own_pid` whose executable lives inside one of
/// `roots` and waits briefly for them to exit.
fn stop_processes_in(roots: &[PathBuf], own_pid: u32) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.into());
    let roots: Vec<PathBuf> = roots.iter().map(|root| canonical(root)).collect();
    let refresh = ProcessRefreshKind::nothing().with_exe(UpdateKind::Always);
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    let mut pids = Vec::new();
    for process in system.processes().values() {
        if process.pid() == Pid::from_u32(own_pid) {
            continue;
        }
        let Some(exe) = process.exe() else { continue };
        let exe = canonical(exe);
        if roots.iter().any(|root| exe.starts_with(root)) {
            info!(pid = process.pid().as_u32(), path = %exe.display(), "Stopping installation process for update");
            process.kill();
            pids.push(process.pid());
        }
    }
    if pids.is_empty() {
        return;
    }
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, refresh);
        let running = pids.iter().any(|pid| {
            system
                .process(*pid)
                .is_some_and(|p| !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead))
        });
        if !running {
            return;
        }
        if Instant::now() >= until {
            warn!("Installation processes did not exit in time");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
    use super::*;

    #[tokio::test]
    async fn cancellation_removes_staging_and_script() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging-id");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("update.log"), "log").unwrap();
        let script = root.path().join("update-id.sh");
        fs::write(&script, "echo hi").unwrap();
        let package = root.path().join("package");
        fs::write(&package, "verified download").unwrap();
        Prepared {
            staging: staging.clone(),
            script: script.clone(),
            restart_dir: root.path().into(),
            process_roots: vec![],
        }
        .cancel()
        .await;
        assert!(!staging.exists());
        assert!(!script.exists());
        assert_eq!(fs::read_to_string(package).unwrap(), "verified download");
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
    fn failed_staging_leaves_nothing_behind() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        fs::write(&package, "not a valid package").unwrap();
        let target = root.path().join("installed");
        fs::write(&target, "old version").unwrap();

        assert!(prepare_in(root.path(), "id", &package, target, false).is_err());
        assert!(!root.path().join("staging-id").exists(), "staging left behind");
        assert!(!root.path().join(ScriptKind::host().file_name("id")).exists());
        assert!(package.exists(), "verified download must be kept");
    }

    #[test]
    fn install_lock_blocks_until_stale() {
        let root = tempfile::tempdir().unwrap();
        let lock = root.path().join("YAAS.AppImage.yaas-update.lock");
        check_install_lock(&lock, STALE_LOCK_AGE).unwrap();
        fs::create_dir(&lock).unwrap();
        let error = check_install_lock(&lock, STALE_LOCK_AGE).unwrap_err();
        assert!(error.to_string().contains("being installed"));
        assert!(lock.exists());
        check_install_lock(&lock, Duration::ZERO).unwrap();
        assert!(!lock.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stops_only_other_processes_inside_roots() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install");
        let external = root.path().join("external");
        let sleep = which::which("sleep").unwrap();
        let spawn = |dir: &Path, name: &str| {
            fs::create_dir_all(dir).unwrap();
            let exe = dir.join(name);
            fs::copy(&sleep, &exe).unwrap();
            Command::new(exe).arg("30").spawn().unwrap()
        };
        let mut helper = spawn(&install, "7za");
        let mut own = spawn(&install, "yaas");
        let mut outside = spawn(&external, "adb");

        stop_processes_in(&[install], own.id());

        assert!(helper.try_wait().unwrap().is_some(), "installation helper still running");
        assert!(own.try_wait().unwrap().is_none(), "own process was killed");
        assert!(outside.try_wait().unwrap().is_none(), "external process was killed");
        for child in [&mut own, &mut outside] {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    #[cfg(unix)]
    #[test]
    fn linux_script_skips_install_while_locked() {
        use super::script::{Payload, ScriptKind, ScriptPlan, lock_path, render};

        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("updates/staging-id");
        fs::create_dir_all(&staging).unwrap();
        let target = root.path().join("YAAS.AppImage");
        fs::write(&target, "old version").unwrap();
        let package = root.path().join("package");
        fs::write(&package, "new version").unwrap();
        let lock = lock_path(ScriptKind::Linux, &target);
        fs::create_dir(&lock).unwrap();
        let mut exited = Command::new("true").spawn().unwrap();
        let exited_pid = exited.id();
        exited.wait().unwrap();

        let plan = ScriptPlan {
            staging: staging.clone(),
            package: package.clone(),
            payload: Payload::AppImage { mode: 0o755 },
            target: target.clone(),
            pid: exited_pid,
            portable: false,
        };
        let script = root.path().join("updates/update-id.sh");
        fs::write(&script, render(ScriptKind::Linux, &plan).unwrap()).unwrap();
        let status = script_command(&script, root.path()).spawn().unwrap().wait().unwrap();

        assert!(!status.success());
        assert_eq!(fs::read_to_string(&target).unwrap(), "old version");
        assert!(lock.exists(), "another script's lock was removed");
        assert!(package.exists());
        assert!(fs::read_to_string(staging.join("update.log")).unwrap().contains("skipping"));
    }

    /// Runs a generated script for the host platform against a fixture
    /// installation. A short-lived process stands in for the running app.
    #[cfg(unix)]
    #[test]
    fn linux_script_replaces_appimage_relaunches_and_cleans_up() {
        use std::{io::Write as _, time::Instant};

        use super::script::{Payload, ScriptKind, ScriptPlan, render};

        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install dir");
        let staging = root.path().join("updates/staging-id");
        let downloads = root.path().join("updates/downloads/id");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        let target = install.join("YAAS.AppImage");
        fs::write(&target, "old version").unwrap();
        let marker = root.path().join("relaunched");
        // The downloaded package: a fake AppImage (a script) that leaves a marker when run.
        let package = downloads.join("package");
        let mut file = fs::File::create(&package).unwrap();
        writeln!(file, "#!/bin/sh\necho relaunched > \"{}\"", marker.display()).unwrap();
        drop(file);

        let mut sleeper = std::process::Command::new("sleep").arg("1").spawn().unwrap();
        let pid = sleeper.id();
        // Reap the sleeper so the script's wait loop observes the exit.
        std::thread::spawn(move || {
            let _ = sleeper.wait();
        });

        let plan = ScriptPlan {
            staging: staging.clone(),
            package: package.clone(),
            payload: Payload::AppImage { mode: 0o750 },
            target: target.clone(),
            pid,
            portable: true,
        };
        let script = root.path().join("updates/update-id.sh");
        fs::write(&script, render(ScriptKind::Linux, &plan).unwrap()).unwrap();
        let status = script_command(&script, &install).spawn().unwrap().wait().unwrap();
        assert!(status.success());

        // The relaunched "app" runs asynchronously; wait for its marker.
        let until = Instant::now() + std::time::Duration::from_secs(10);
        while !marker.exists() && Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(marker.exists(), "new app was not relaunched");
        assert!(fs::read_to_string(&target).unwrap().starts_with("#!/bin/sh"));
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o750);
        }
        assert!(!install.join("YAAS.AppImage.yaas-update.lock").exists(), "lock not released");
        assert!(!package.exists(), "package not cleaned up");
        assert!(!staging.exists(), "staging not cleaned up");
        assert!(!script.exists(), "script did not delete itself");
    }

    #[cfg(windows)]
    #[test]
    fn windows_script_replaces_files_and_preserves_extras() {
        use std::time::{Duration, Instant};

        use super::script::{Payload, ScriptKind, ScriptPlan, render};

        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("installed dir");
        fs::create_dir_all(install.join("_portable_data")).unwrap();
        fs::write(install.join("yaas.exe"), "old").unwrap();
        fs::write(install.join("hub.dll"), "old").unwrap();
        fs::write(install.join("obsolete.dll"), "keep").unwrap();
        fs::write(install.join("_portable_data/settings.json"), "settings").unwrap();
        let staging = root.path().join("updates/staging-id");
        let new_dir = staging.join("new");
        fs::create_dir_all(&new_dir).unwrap();
        // The new yaas.exe must be a real executable for the relaunch step.
        fs::copy(r"C:\Windows\System32\whoami.exe", new_dir.join("yaas.exe")).unwrap();
        fs::write(new_dir.join("hub.dll"), "new").unwrap();
        let downloads = root.path().join("updates/downloads/id");
        fs::create_dir_all(&downloads).unwrap();
        let package = downloads.join("package");
        fs::write(&package, "zip").unwrap();

        let mut sleeper = std::process::Command::new("ping")
            .args(["-n", "3", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = sleeper.id();
        std::thread::spawn(move || {
            let _ = sleeper.wait();
        });

        let plan = ScriptPlan {
            staging: staging.clone(),
            package: package.clone(),
            payload: Payload::Directory(new_dir),
            target: install.clone(),
            pid,
            portable: true,
        };
        let script = root.path().join("updates/update-id.bat");
        fs::write(&script, render(ScriptKind::Windows, &plan).unwrap()).unwrap();
        let mut child = script_command(&script, &install).spawn().unwrap();

        // The script self-deletes on success; poll for completion.
        let until = Instant::now() + Duration::from_secs(90);
        while script.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = child.kill();
        assert!(!script.exists(), "script did not delete itself");
        assert!(!staging.exists(), "staging not cleaned up");
        assert!(!package.exists(), "package not cleaned up");
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
        use std::time::{Duration, Instant};

        use super::script::{Payload, ScriptKind, ScriptPlan, render};

        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("updates/staging-id");
        let downloads = root.path().join("updates/downloads/id");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&downloads).unwrap();

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
        let package = downloads.join("package");
        assert!(
            Command::new("/usr/bin/ditto")
                .args(["-c", "-k"])
                .arg(&source)
                .arg(&package)
                .status()
                .unwrap()
                .success(),
            "zip creation failed"
        );

        // Staging (runs pre-exit in the real flow): extraction + verification.
        let staged = super::package::extract_macos_zip(&package, &staging).unwrap();
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

        let mut sleeper = std::process::Command::new("sleep").arg("1").spawn().unwrap();
        let pid = sleeper.id();
        std::thread::spawn(move || {
            let _ = sleeper.wait();
        });
        let plan = ScriptPlan {
            staging: staging.clone(),
            package: package.clone(),
            payload: Payload::Bundle(staged),
            target: target.clone(),
            pid,
            portable: false,
        };
        let script = root.path().join("updates/update-id.sh");
        fs::write(&script, render(ScriptKind::MacOs, &plan).unwrap()).unwrap();
        let status = script_command(&script, root.path()).spawn().unwrap().wait().unwrap();
        assert!(status.success());

        let until = Instant::now() + Duration::from_secs(10);
        while staging.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(fs::read_to_string(target.join("Contents/MacOS/YAAS")).unwrap().contains("exit 0"));
        assert!(!target.join("stale-file").exists(), "old bundle must be fully replaced");
        assert!(target.join("Contents/Frameworks/F.framework/F").exists());
        assert!(!staging.exists(), "staging not cleaned up");
        assert!(!script.exists(), "script did not delete itself");
    }
}
