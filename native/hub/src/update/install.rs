use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, ensure};
use app_update::{
    Identity,
    package::{Inventory, extract_zip},
    transaction::{Phase, Transaction, clean_environment, replacement, windows_replacements},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Child,
    time::{Duration, timeout},
};

use super::release::Candidate;

pub(super) struct Prepared {
    pub directory: PathBuf,
    pub transaction: Transaction,
    pub helper: Child,
    pub committed: bool,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.helper.start_kill();
        }
    }
}

impl Prepared {
    pub async fn commit(&mut self) -> Result<()> {
        ensure!(self.helper.try_wait()?.is_none(), "Update helper stopped before shutdown");
        let mut input =
            self.helper.stdin.take().context("Update helper is not waiting for shutdown")?;
        input.write_all(b"COMMIT\n").await?;
        input.shutdown().await?;
        self.committed = true;
        Ok(())
    }

    pub async fn cancel(mut self) {
        let _ = self.helper.kill().await;
        let _ = self.helper.wait().await;
        let _ = fs::remove_dir_all(&self.transaction.workspace);
        if self.transaction.error.is_none() {
            let _ = fs::remove_dir_all(&self.directory);
        }
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
        let inventory = Inventory::read(&directory).context(
            "This installation has no package inventory; install a current release manually first",
        )?;
        ensure!(
            inventory.identity == installed_identity(),
            "The package inventory does not match this build; reinstall YAAS manually"
        );
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

fn prepare_files(
    root: &Path,
    download: &Path,
    candidate: &Candidate,
) -> Result<(PathBuf, Transaction)> {
    app_update::verify_file(&download.join("package"), &candidate.asset).map_err(|e| {
        super::release::failure(
            crate::models::signals::update::AppUpdateErrorKind::Integrity,
            e.to_string(),
        )
    })?;
    let (target, helper) = installation()?;
    let directory = root.join("transactions").join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&directory)?;
    let installation_lock = if cfg!(windows) {
        target.join(".yaas-update.lock")
    } else {
        target.with_file_name(format!(
            ".{}.yaas-update.lock",
            target.file_name().unwrap().to_string_lossy()
        ))
    };
    let working_directory = restart_directory(&target)?;
    let workspace_parent = if cfg!(windows) { target.as_path() } else { target.parent().unwrap() };
    let workspace = tempfile::Builder::new()
        .prefix(".yaas-update-")
        .tempdir_in(workspace_parent)
        .context("Cannot stage update beside installation")?;
    let staged = workspace.path().join("new");
    let backup = workspace.path().join("backup");
    let result = (|| -> Result<Transaction> {
        let replacements;
        let executable;
        if cfg!(target_os = "linux") {
            fs::copy(download.join("package"), &staged)?;
            fs::set_permissions(&staged, fs::metadata(&target)?.permissions())?;
            let mut bytes = [0; 64];
            fs::File::open(&staged)?.read_exact(&mut bytes)?;
            ensure!(
                bytes.starts_with(b"\x7fELF") && bytes.get(8..11) == Some(b"AI\x02"),
                "Downloaded file is not an AppImage"
            );
            executable = target.clone();
            replacements = vec![replacement(target, Some(staged), backup)];
        } else {
            extract_zip(&download.join("package"), &staged, cfg!(target_os = "macos"))?;
            if cfg!(target_os = "windows") {
                replacements =
                    windows_replacements(&target, &staged, &backup, &candidate.identity)?;
                executable = target.join("yaas.exe");
            } else {
                let bundle = staged.join("YAAS.app");
                let identity: Identity = serde_json::from_slice(&fs::read(
                    bundle.join("Contents/Resources/yaas-build.json"),
                )?)?;
                ensure!(identity == candidate.identity, "Downloaded bundle identity mismatch");
                #[cfg(target_os = "macos")]
                ensure!(
                    std::process::Command::new("/usr/bin/codesign")
                        .args(["--verify", "--deep", "--strict"])
                        .arg(&bundle)
                        .status()?
                        .success(),
                    "Downloaded app signature is invalid"
                );
                executable = target.join("Contents/MacOS/YAAS");
                replacements = vec![replacement(target, Some(bundle), backup)];
            }
        }
        let helper_name = if cfg!(windows) { "yaas-updater.exe" } else { "yaas-updater" };
        fs::copy(helper, directory.join(helper_name))?;
        let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
        let transaction = Transaction {
            id: candidate.info.candidate_id.clone(),
            expected: candidate.identity.clone(),
            parent_pid: std::process::id(),
            installation_lock,
            workspace: workspace.path().to_path_buf(),
            executable,
            arguments,
            working_directory,
            replacements,
            phase: Phase::Prepared,
            error: None,
        };
        transaction.preflight()?;
        transaction.save(&directory)?;
        Ok(transaction)
    })();
    match result {
        Ok(transaction) => {
            let _ = workspace.keep();
            Ok((directory, transaction))
        }
        Err(error) => {
            let _ = fs::remove_dir_all(directory);
            Err(error)
        }
    }
}

pub(super) async fn prepare(
    root: PathBuf,
    download: PathBuf,
    candidate: Candidate,
) -> Result<Prepared> {
    let (directory, mut transaction) =
        tokio::task::spawn_blocking(move || prepare_files(&root, &download, &candidate)).await??;
    let result = async {
        let name = if cfg!(windows) { "yaas-updater.exe" } else { "yaas-updater" };
        let mut command = tokio::process::Command::new(directory.join(name));
        command
            .arg(&directory)
            .current_dir(&directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(fs::File::create(directory.join("helper.log"))?))
            .kill_on_drop(false);
        clean_environment(command.as_std_mut());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut helper = command.spawn().context("Cannot start update helper")?;
        let mut output = BufReader::new(helper.stdout.take().unwrap());
        let mut line = String::new();
        let ready = timeout(Duration::from_secs(10), output.read_line(&mut line)).await;
        if !matches!(ready, Ok(Ok(_))) || line.trim() != "READY" {
            let _ = helper.kill().await;
            let _ = helper.wait().await;
            anyhow::bail!("Update helper failed preflight; see helper.log");
        }
        Ok(helper)
    }
    .await;
    match result {
        Ok(helper) => Ok(Prepared { directory, transaction, helper, committed: false }),
        Err(error) => {
            let _ = fs::remove_dir_all(&transaction.workspace);
            transaction.error = Some(format!("{error:#}"));
            let _ = transaction.save(&directory);
            Err(error)
                .with_context(|| format!("Helper log: {}", directory.join("helper.log").display()))
        }
    }
}

#[derive(Default)]
pub(super) struct Recovery {
    pub message: Option<String>,
    pub block: Option<String>,
}

pub(super) fn recover_startup(root: &Path) -> Result<Recovery> {
    let transactions = root.join("transactions");
    if !transactions.exists() {
        return Ok(Recovery::default());
    }
    let mut block = None;
    let mut messages = Vec::new();
    for entry in fs::read_dir(transactions)? {
        let directory = entry?.path();
        if !directory.is_dir() {
            continue;
        }
        let Ok(mut tx) = Transaction::read(&directory) else {
            continue;
        };
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("helper.lock"))?;
        if matches!(tx.phase, Phase::Launching | Phase::Launched)
            && tx.expected == installed_identity()
        {
            app_update::transaction::acknowledge(&directory, &installed_identity())?;
            let _ = fs::remove_dir_all(root.join("downloads").join(&tx.id));
            if lock.try_lock().is_ok() {
                app_update::transaction::remove(&tx.workspace)?;
                drop(lock);
                fs::remove_dir_all(&directory)?;
            }
            continue;
        }
        if lock.try_lock().is_err() {
            continue;
        }
        match tx.phase {
            Phase::Prepared => {
                app_update::transaction::remove(&tx.workspace)?;
            }
            Phase::Applying => {
                // The old app may have been started manually after a power loss. Do not
                // replace its loaded files; keep the journal for explicit recovery.
                tx.phase = Phase::RecoveryRequired;
                tx.error = Some(
                    "An update was interrupted. Close YAAS and restore the backup recorded in \
                     transaction.json before retrying."
                        .into(),
                );
                tx.save(&directory)?;
            }
            _ => {}
        }
        if tx.phase == Phase::RecoveryRequired {
            block =
                Some(format!("An interrupted update needs recovery; see {}", directory.display()));
        }
        if let Some(error) = tx.error {
            messages.push(format!("{error} ({})", directory.display()));
        } else if matches!(tx.phase, Phase::Launching | Phase::Launched) {
            messages.push(format!(
                "An update has not confirmed startup; backup retained at {}",
                tx.workspace.display()
            ));
        }
    }
    Ok(Recovery { message: (!messages.is_empty()).then(|| messages.join("\n")), block })
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

    fn transaction(root: &Path, phase: Phase) -> (PathBuf, Transaction) {
        let directory = root.join("transactions/test");
        fs::create_dir_all(&directory).unwrap();
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("backup"), "old app").unwrap();
        let tx = Transaction {
            id: "test".into(),
            expected: installed_identity(),
            parent_pid: std::process::id(),
            installation_lock: root.join("install.lock"),
            workspace,
            executable: root.join("app"),
            arguments: vec![],
            working_directory: root.into(),
            replacements: vec![],
            phase,
            error: None,
        };
        tx.save(&directory).unwrap();
        (directory, tx)
    }

    #[test]
    fn interrupted_replacement_preserves_backup_and_blocks_installation() {
        let root = tempfile::tempdir().unwrap();
        let (directory, tx) = transaction(root.path(), Phase::Applying);
        let recovery = recover_startup(root.path()).unwrap();
        assert!(recovery.block.is_some());
        assert!(recovery.message.is_some());
        assert_eq!(Transaction::read(&directory).unwrap().phase, Phase::RecoveryRequired);
        assert_eq!(fs::read_to_string(tx.workspace.join("backup")).unwrap(), "old app");
    }

    #[test]
    fn accepted_build_cleans_completed_transaction_and_download() {
        let root = tempfile::tempdir().unwrap();
        let (directory, tx) = transaction(root.path(), Phase::Launched);
        fs::create_dir_all(root.path().join("downloads/test")).unwrap();
        let recovery = recover_startup(root.path()).unwrap();
        assert!(recovery.block.is_none());
        assert!(!directory.exists());
        assert!(!tx.workspace.exists());
        assert!(!root.path().join("downloads/test").exists());
    }

    #[test]
    fn running_helper_owns_cleanup_after_acknowledgement() {
        let root = tempfile::tempdir().unwrap();
        let (directory, tx) = transaction(root.path(), Phase::Launching);
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("helper.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        recover_startup(root.path()).unwrap();
        assert!(directory.join("accepted.json").exists());
        assert!(tx.workspace.join("backup").exists());
    }
}
