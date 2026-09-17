//! Finds and stops processes belonging to the installation being updated.

use crate::install::InstallRequest;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use sysinfo::{
    Pid, Process, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_time: u64,
}

impl ProcessIdentity {
    pub fn current() -> Result<Self> {
        let pid = Pid::from_u32(std::process::id());
        let mut system = System::new();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, refresh_kind());
        Ok(Self::of(
            system
                .process(pid)
                .context("Cannot identify YAAS process")?,
        ))
    }

    fn of(process: &Process) -> Self {
        Self {
            pid: process.pid().as_u32(),
            start_time: process.start_time(),
        }
    }
}

fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_exe(UpdateKind::Always)
        .with_environ(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet)
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right || matches!((left.canonicalize(), right.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

struct TargetPaths {
    apps: Vec<PathBuf>,
    helpers: Vec<PathBuf>,
}

impl TargetPaths {
    fn new(request: &InstallRequest) -> Self {
        Self {
            apps: vec![request.runtime_executable.clone(), request.executable()],
            helpers: request.bundled_executables.clone(),
        }
    }

    fn add_mount(&mut self, request: &InstallRequest, mount: &Path) {
        let Some(original) = &request.appdir else {
            return;
        };
        if let Ok(relative) = request.runtime_executable.strip_prefix(original) {
            let path = mount.join(relative);
            if !self.apps.contains(&path) {
                self.apps.push(path);
            }
        }
        for path in &request.bundled_executables {
            if let Ok(relative) = path.strip_prefix(original) {
                let path = mount.join(relative);
                if !self.helpers.contains(&path) {
                    self.helpers.push(path);
                }
            }
        }
    }

    fn discover_mounts(&mut self, request: &InstallRequest, process: &Process) {
        if request.appdir.is_none() {
            return;
        }
        let value = |key: &str| {
            process
                .environ()
                .iter()
                .filter_map(|entry| entry.to_str())
                .find_map(|entry| entry.strip_prefix(key))
        };
        if let (Some(image), Some(mount)) = (value("APPIMAGE="), value("APPDIR="))
            && same_path(Path::new(image), &request.target)
        {
            self.add_mount(request, Path::new(mount));
        }
    }

    fn is_app(&self, path: &Path) -> bool {
        self.apps.iter().any(|app| same_path(path, app))
    }

    fn contains(&self, path: &Path) -> bool {
        self.is_app(path) || self.helpers.iter().any(|helper| same_path(path, helper))
    }
}

pub fn stop_installation(request: &InstallRequest) -> Result<()> {
    stop_with_timeouts(request, Duration::from_secs(5), Duration::from_secs(5))
}

fn stop_with_timeouts(
    request: &InstallRequest,
    graceful: Duration,
    forced: Duration,
) -> Result<()> {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    let own_pid = Pid::from_u32(std::process::id());
    let user = system
        .process(own_pid)
        .and_then(Process::user_id)
        .context("Cannot identify updater user")?
        .clone();
    let mut paths = TargetPaths::new(request);
    let mut known = HashSet::from([request.parent]);
    let mut apps = HashSet::from([request.parent]);
    let graceful_until = Instant::now() + graceful;
    let mut forced_until = None;
    loop {
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
        for process in system
            .processes()
            .values()
            .filter(|p| p.user_id() == Some(&user))
        {
            paths.discover_mounts(request, process);
        }
        let mut remaining = Vec::new();
        for process in system.processes().values() {
            if process.pid() == own_pid
                || process.user_id() != Some(&user)
                || matches!(
                    process.status(),
                    ProcessStatus::Zombie | ProcessStatus::Dead
                )
            {
                continue;
            }
            let identity = ProcessIdentity::of(process);
            if let Some(exe) = process.exe() {
                if paths.is_app(exe) {
                    apps.insert(identity);
                }
                if paths.contains(exe) {
                    known.insert(identity);
                }
            }
            if known.contains(&identity) {
                remaining.push(identity);
            }
        }
        if remaining.is_empty() {
            return Ok(());
        }
        if forced_until.is_none()
            && (Instant::now() >= graceful_until || !remaining.iter().any(|id| apps.contains(id)))
        {
            forced_until = Some(Instant::now() + forced);
        }
        if let Some(until) = forced_until {
            ensure!(
                Instant::now() < until,
                "Installation processes did not stop: {remaining:?}"
            );
            for identity in remaining {
                let pid = Pid::from_u32(identity.pid);
                // Refresh immediately before killing so a reused PID isn't mistaken for our target.
                system.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    true,
                    refresh_kind(),
                );
                if let Some(process) = system.process(pid)
                    && ProcessIdentity::of(process) == identity
                    && (identity == request.parent
                        || process.exe().is_some_and(|exe| paths.contains(exe)))
                {
                    process.kill();
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;
    use std::{
        fs,
        process::{Command, Stdio},
    };

    #[test]
    #[ignore]
    fn sleeper() {
        if std::env::var_os("YAAS_UPDATER_TEST_SLEEP").is_none() {
            return;
        }
        fs::write("ready", "ready").unwrap();
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn refuses_to_continue_when_processes_survive_the_deadline() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let exe = root
            .path()
            .join(if cfg!(windows) { "app.exe" } else { "app" });
        fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let mut child = Command::new(&exe)
            .args(["--ignored", "--exact", "processes::tests::sleeper"])
            .env("YAAS_UPDATER_TEST_SLEEP", "1")
            .current_dir(root.path())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while !root.path().join("ready").exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        let request = InstallRequest {
            package: root.path().join("package"),
            target: root.path().join("installed"),
            expected: Identity {
                version: "2.0.0".into(),
                build_number: 1,
                channel: "stable".into(),
                commit: "a".repeat(40),
                run_number: 2,
                run_attempt: 1,
            },
            parent: ProcessIdentity {
                pid: u32::MAX,
                start_time: 0,
            },
            runtime_executable: exe,
            bundled_executables: vec![],
            appdir: None,
            arguments: vec![],
            working_directory: root.path().into(),
        };
        let result = stop_with_timeouts(&request, Duration::ZERO, Duration::ZERO);
        let alive = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(alive);
        assert!(result.unwrap_err().to_string().contains("did not stop"));
    }
}
