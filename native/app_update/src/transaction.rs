//! File replacement with a journal and a retained backup.

use crate::{
    Identity,
    package::{Inventory, check_destination},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replacement {
    pub destination: PathBuf,
    pub source: Option<PathBuf>,
    pub backup: PathBuf,
    pub had_original: bool,
    pub started: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub expected: Identity,
    pub parent_pid: u32,
    pub installation_lock: PathBuf,
    pub workspace: PathBuf,
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub working_directory: PathBuf,
    pub replacements: Vec<Replacement>,
    pub phase: Phase,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Prepared,
    Applying,
    Launching,
    Launched,
    RolledBack,
    RecoveryRequired,
}

pub fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("Missing parent")?)?;
    serde_json::to_writer_pretty(&mut temporary, value)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

impl Transaction {
    pub fn read(directory: &Path) -> Result<Self> {
        serde_json::from_slice(&fs::read(directory.join("transaction.json"))?)
            .context("Cannot read update transaction")
    }
    pub fn save(&self, directory: &Path) -> Result<()> {
        save_json(&directory.join("transaction.json"), self)
    }

    pub fn preflight(&self) -> Result<()> {
        ensure!(self.phase == Phase::Prepared, "Transaction is not prepared");
        ensure!(
            !self.replacements.is_empty(),
            "Empty installation transaction"
        );
        self.expected.validate()?;
        for replacement in &self.replacements {
            check_destination(&replacement.destination)?;
            ensure!(!replacement.backup.exists(), "Backup already exists");
            ensure!(
                replacement.destination.try_exists()? == replacement.had_original,
                "Installation changed during preparation"
            );
            if let Some(source) = &replacement.source {
                ensure!(source.exists(), "Staged update is missing");
            }
            let mut parent = replacement
                .destination
                .parent()
                .context("Missing installation directory")?;
            while !parent.exists() {
                parent = parent.parent().context("Missing installation directory")?;
            }
            tempfile::NamedTempFile::new_in(parent)
                .context("Installation directory is not writable")?;
            if replacement.had_original && replacement.destination.is_file() {
                ensure!(
                    !fs::metadata(&replacement.destination)?
                        .permissions()
                        .readonly(),
                    "An installed file is read-only"
                );
            }
        }
        Ok(())
    }

    pub fn apply(&mut self, directory: &Path) -> Result<()> {
        self.preflight()?;
        self.phase = Phase::Applying;
        self.save(directory)?;
        for index in 0..self.replacements.len() {
            self.replacements[index].started = true;
            self.save(directory)?;
            let replacement = &self.replacements[index];
            fs::create_dir_all(replacement.destination.parent().unwrap())?;
            fs::create_dir_all(replacement.backup.parent().unwrap())?;
            if replacement.had_original {
                rename(&replacement.destination, &replacement.backup)?;
            }
            if let Some(source) = &replacement.source {
                rename(source, &replacement.destination)?;
            }
        }
        self.phase = Phase::Launching;
        self.save(directory)
    }

    pub fn rollback(&mut self, directory: &Path) -> Result<()> {
        let mut errors = Vec::new();
        for replacement in self.replacements.iter().rev().filter(|r| r.started) {
            let restore = || -> Result<()> {
                if replacement.backup.exists() {
                    remove(&replacement.destination)?;
                    rename(&replacement.backup, &replacement.destination)?;
                } else if !replacement.had_original
                    && replacement.source.as_ref().is_some_and(|s| !s.exists())
                {
                    remove(&replacement.destination)?;
                }
                Ok(())
            };
            if let Err(error) = restore() {
                errors.push(format!("{}: {error:#}", replacement.destination.display()));
            }
        }
        self.phase = if errors.is_empty() {
            Phase::RolledBack
        } else {
            Phase::RecoveryRequired
        };
        if !errors.is_empty() {
            self.error = Some(format!(
                "{}; rollback failed: {}",
                self.error.as_deref().unwrap_or("Update failed"),
                errors.join("; ")
            ));
        }
        self.save(directory)?;
        ensure!(errors.is_empty(), "Rollback failed: {}", errors.join("; "));
        Ok(())
    }

    pub fn launch(&self) -> Result<std::process::Child> {
        let mut command = Command::new(&self.executable);
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
        command.spawn().context("Cannot launch YAAS")
    }
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

pub fn replacement(destination: PathBuf, source: Option<PathBuf>, backup: PathBuf) -> Replacement {
    Replacement {
        had_original: destination.exists(),
        destination,
        source,
        backup,
        started: false,
    }
}

pub fn windows_replacements(
    installed: &Path,
    staged: &Path,
    backup: &Path,
    expected: &Identity,
) -> Result<Vec<Replacement>> {
    let old = Inventory::read(installed)?;
    let new = Inventory::read(staged)?;
    ensure!(
        new.identity == *expected,
        "Downloaded package identity mismatch"
    );
    let old_files: BTreeSet<_> = old.files.iter().collect();
    let new_files: BTreeSet<_> = new.files.iter().collect();
    for new_name in &new_files {
        if let Some(old_name) = old_files
            .iter()
            .find(|old| old.eq_ignore_ascii_case(new_name))
        {
            ensure!(
                old_name == new_name,
                "Package changes path casing: {new_name}"
            );
        }
    }
    let mut result = Vec::new();
    for name in old_files.union(&new_files) {
        let destination = installed.join(name);
        check_destination(&destination)?;
        ensure!(
            !destination.exists() || old_files.contains(name),
            "Update conflicts with an unowned file: {name}"
        );
        if destination.exists() {
            ensure!(
                destination.is_file(),
                "Package file replaced by a directory: {name}"
            );
        }
        let source = new_files.contains(name).then(|| staged.join(name));
        if source.is_some() || destination.exists() {
            result.push(replacement(destination, source, backup.join(name)));
        }
    }
    Ok(result)
}

pub fn remove(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)?
        }
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

fn rename(source: &Path, destination: &Path) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(e)
                if cfg!(windows)
                    && Instant::now() < until
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
                    ) =>
            {
                thread::sleep(Duration::from_millis(100))
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "Cannot move {} to {}",
                        source.display(),
                        destination.display()
                    )
                });
            }
        }
    }
}

/// Marks only a successfully initialized build as accepted. The helper owns cleanup.
pub fn acknowledge(directory: &Path, installed: &Identity) -> Result<bool> {
    let transaction = Transaction::read(directory)?;
    if transaction.expected != *installed
        || !matches!(transaction.phase, Phase::Launching | Phase::Launched)
    {
        return Ok(false);
    }
    save_json(&directory.join("accepted.json"), &transaction.id)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(root: &Path) -> Transaction {
        let old = root.join("app");
        let new = root.join("new");
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        Transaction {
            id: "test".into(),
            expected: Identity {
                version: "1.0.0".into(),
                build_number: 1,
                channel: "stable".into(),
                commit: "a".repeat(40),
                run_number: 1,
                run_attempt: 1,
            },
            parent_pid: std::process::id(),
            installation_lock: root.join("installation.lock"),
            workspace: root.into(),
            executable: old.clone(),
            arguments: vec![],
            working_directory: root.into(),
            replacements: vec![replacement(old, Some(new), root.join("backup"))],
            phase: Phase::Prepared,
            error: None,
        }
    }
    #[test]
    fn replacement_and_rollback_preserve_original() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut tx = sample(root.path());
        tx.save(root.path()).unwrap();
        tx.apply(root.path()).unwrap();
        assert_eq!(fs::read(root.path().join("app")).unwrap(), b"new");
        assert_eq!(fs::read(root.path().join("backup")).unwrap(), b"old");
        tx.rollback(root.path()).unwrap();
        assert_eq!(fs::read(root.path().join("app")).unwrap(), b"old");
        tx.rollback(root.path()).unwrap();
    }
    #[test]
    fn interrupted_apply_restores_only_started_files() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut tx = sample(root.path());
        let absent = root.path().join("created");
        let source = root.path().join("source");
        fs::write(&source, b"created").unwrap();
        tx.replacements.push(replacement(
            absent.clone(),
            Some(source),
            root.path().join("backup2"),
        ));
        tx.apply(root.path()).unwrap();
        tx.rollback(root.path()).unwrap();
        assert!(!absent.exists());
    }
    #[test]
    fn startup_ack_requires_expected_identity() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut tx = sample(root.path());
        tx.save(root.path()).unwrap();
        assert!(!acknowledge(root.path(), &tx.expected).unwrap());
        tx.apply(root.path()).unwrap();
        let mut wrong = tx.expected.clone();
        wrong.run_attempt += 1;
        assert!(!acknowledge(root.path(), &wrong).unwrap());
        assert!(acknowledge(root.path(), &tx.expected).unwrap());
        assert!(root.path().join("backup").exists());
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    use crate::package::{INVENTORY, Inventory};

    fn identity(version: &str) -> Identity {
        Identity {
            version: version.into(),
            build_number: 1,
            channel: "stable".into(),
            commit: "a".repeat(40),
            run_number: 1,
            run_attempt: 1,
        }
    }
    fn package(root: &Path, version: &str, extra: &str) {
        fs::create_dir_all(root).unwrap();
        let files = [INVENTORY, "yaas.exe", "hub.dll", "yaas-updater.exe", extra]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        for file in &files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, version).unwrap();
        }
        save_json(
            &root.join(INVENTORY),
            &Inventory {
                schema_version: 1,
                identity: identity(version),
                files,
            },
        )
        .unwrap();
    }
    #[test]
    fn windows_inventory_preserves_user_files_and_removes_obsolete_files() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let installed = root.path().join("installed");
        let staged = root.path().join("staged");
        package(&installed, "1.0.0", "data/obsolete");
        package(&staged, "2.0.0", "data/new");
        fs::create_dir(installed.join("_portable_data")).unwrap();
        fs::write(installed.join("_portable_data/settings.json"), "settings").unwrap();
        fs::write(installed.join("notes.txt"), "notes").unwrap();
        let replacements = windows_replacements(
            &installed,
            &staged,
            &root.path().join("backup"),
            &identity("2.0.0"),
        )
        .unwrap();
        let mut tx = Transaction {
            id: "test".into(),
            expected: identity("2.0.0"),
            parent_pid: std::process::id(),
            installation_lock: root.path().join("lock"),
            workspace: root.path().into(),
            executable: installed.join("yaas.exe"),
            arguments: vec!["--portable".into()],
            working_directory: installed.clone(),
            replacements,
            phase: Phase::Prepared,
            error: None,
        };
        tx.apply(root.path()).unwrap();
        assert!(!installed.join("data/obsolete").exists());
        assert!(installed.join("data/new").exists());
        assert_eq!(
            fs::read_to_string(installed.join("_portable_data/settings.json")).unwrap(),
            "settings"
        );
        assert_eq!(
            fs::read_to_string(installed.join("notes.txt")).unwrap(),
            "notes"
        );
        tx.rollback(root.path()).unwrap();
        assert!(installed.join("data/obsolete").exists());
        assert!(!installed.join("data/new").exists());
        assert_eq!(
            Inventory::read(&installed).unwrap().identity,
            identity("1.0.0")
        );
    }
    #[test]
    fn rejects_unowned_collision_and_wrong_identity() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let installed = root.path().join("installed");
        let staged = root.path().join("staged");
        package(&installed, "1.0.0", "old");
        package(&staged, "2.0.0", "new");
        fs::write(installed.join("new"), "user file").unwrap();
        assert!(
            windows_replacements(
                &installed,
                &staged,
                &root.path().join("backup"),
                &identity("2.0.0")
            )
            .is_err()
        );
        fs::remove_file(installed.join("new")).unwrap();
        assert!(
            windows_replacements(
                &installed,
                &staged,
                &root.path().join("backup"),
                &identity("3.0.0")
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_installation_files() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let installed = root.path().join("installed");
        let staged = root.path().join("staged");
        package(&installed, "1.0.0", "old");
        package(&staged, "2.0.0", "new");
        fs::remove_file(installed.join("hub.dll")).unwrap();
        std::os::unix::fs::symlink(root.path().join("outside"), installed.join("hub.dll")).unwrap();
        assert!(
            windows_replacements(
                &installed,
                &staged,
                &root.path().join("backup"),
                &identity("2.0.0")
            )
            .is_err()
        );
    }
}
