use app_update::{Identity, install::InstallRequest, processes::ProcessIdentity};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

// The test executable stands in for YAAS and bundled tools on every desktop OS.
#[test]
#[ignore]
fn fixture_app() {
    let root = std::env::current_dir().unwrap();
    let Ok(role) = fs::read_to_string(root.join("role")) else {
        return;
    };
    if role == "parent" {
        fs::write(root.join(format!("ready-{}", std::process::id())), "ready").unwrap();
        while !root.join("exit-parent").exists() {
            thread::sleep(Duration::from_millis(20));
        }
    } else {
        fs::write(root.join("relaunched"), "yes").unwrap();
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < until, "Timed out waiting for fixture");
        thread::sleep(Duration::from_millis(20));
    }
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const ARGUMENTS: [&str; 4] = ["--ignored", "--exact", "fixture_app", "--nocapture"];

struct Fixture {
    _root: tempfile::TempDir,
    root: PathBuf,
    request_path: PathBuf,
    request: InstallRequest,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::Builder::new()
            .prefix("yaas update spaces ")
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap();
        let root = temporary.path().to_path_buf();
        let target = if cfg!(target_os = "linux") {
            root.join("yaas.AppImage")
        } else if cfg!(windows) {
            root.join("installed")
        } else {
            root.join("YAAS.app")
        };
        let runtime_executable = if cfg!(windows) {
            target.join("yaas.exe")
        } else if cfg!(target_os = "macos") {
            target.join("Contents/MacOS/YAAS")
        } else {
            target.clone()
        };
        fs::create_dir_all(runtime_executable.parent().unwrap()).unwrap();
        fs::copy(std::env::current_exe().unwrap(), &runtime_executable).unwrap();
        let request = InstallRequest {
            package: root.join("package"),
            target,
            runtime_executable,
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
            bundled_executables: vec![],
            appdir: None,
            arguments: ARGUMENTS.into_iter().map(Into::into).collect(),
            working_directory: root.clone(),
        };
        let request_path = root.join("request.json");
        let result = Self {
            _root: temporary,
            root,
            request_path,
            request,
        };
        result.package();
        fs::write(result.root.join("role"), "parent").unwrap();
        result.save();
        result
    }

    fn package(&self) {
        if cfg!(target_os = "linux") {
            let mut binary = fs::read(std::env::current_exe().unwrap()).unwrap();
            // AppImage's marker lives in ELF padding and doesn't change fixture execution.
            binary[8..11].copy_from_slice(b"AI\x02");
            fs::write(&self.request.package, binary).unwrap();
        } else {
            use zip::{ZipWriter, write::SimpleFileOptions};
            let staging = self.root.join("staging");
            let bundle = staging.join("YAAS.app");
            let files = if cfg!(windows) {
                vec!["yaas.exe", "hub.dll", "yaas-updater.exe", "data/new-asset"]
            } else {
                vec![
                    "YAAS.app/Contents/MacOS/YAAS",
                    "YAAS.app/Contents/MacOS/yaas-updater",
                ]
            };
            for file in files {
                let path = staging.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::copy(std::env::current_exe().unwrap(), path).unwrap();
            }
            if cfg!(target_os = "macos") {
                fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
                fs::write(
                    bundle.join("Contents/Resources/yaas-build.json"),
                    serde_json::to_vec(&self.request.expected).unwrap(),
                )
                .unwrap();
                fs::write(bundle.join("Contents/Info.plist"), r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>YAAS</string><key>CFBundleIdentifier</key><string>io.github.skrimix.yaas.fixture</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#).unwrap();
                assert!(
                    Command::new("/usr/bin/codesign")
                        .args(["--force", "--deep", "--sign", "-"])
                        .arg(&bundle)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            fn add(zip: &mut ZipWriter<fs::File>, root: &Path, dir: &Path) {
                for entry in fs::read_dir(dir).unwrap() {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    if path.is_dir() {
                        add(zip, root, &path);
                    } else {
                        zip.start_file(
                            path.strip_prefix(root)
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .replace('\\', "/"),
                            SimpleFileOptions::default().unix_permissions(0o755),
                        )
                        .unwrap();
                        zip.write_all(&fs::read(&path).unwrap()).unwrap();
                    }
                }
            }
            let mut zip = ZipWriter::new(fs::File::create(&self.request.package).unwrap());
            add(&mut zip, &staging, &staging);
            zip.finish().unwrap();
        }
    }

    fn save(&self) {
        self.request.save(&self.request_path).unwrap();
    }

    fn spawn_app(&self, executable: &Path) -> Running {
        let child = Command::new(executable)
            .args(ARGUMENTS)
            .current_dir(&self.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let running = Running(child);
        wait_until(|| self.root.join(format!("ready-{pid}")).exists());
        running
    }

    fn parent(&mut self) -> Running {
        let child = self.spawn_app(&self.request.runtime_executable);
        let pid = Pid::from_u32(child.0.id());
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        self.request.parent = ProcessIdentity {
            pid: child.0.id(),
            start_time: system.process(pid).unwrap().start_time(),
        };
        self.save();
        child
    }

    fn helper(&self) -> Running {
        let log = fs::File::create(self.root.join("updater.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_yaas-updater"))
            .arg(&self.request_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        Running(child)
    }

    fn wait_helper(&self, helper: &mut Running, success: bool) {
        wait_until(|| helper.0.try_wait().unwrap().is_some());
        assert_eq!(
            helper.0.wait().unwrap().success(),
            success,
            "{}",
            fs::read_to_string(self.root.join("updater.log")).unwrap()
        );
    }
}

// Keep real helpers sequential: the updater lock deliberately spans installations.
#[test]
fn helper_lifecycle() {
    let mut fixture = Fixture::new();
    let other = Fixture::new();
    let mut parent = fixture.parent();
    let original = fs::read(&fixture.request.runtime_executable).unwrap();
    let mut helper = fixture.helper();
    wait_until(|| {
        fs::read_to_string(fixture.root.join("updater.log"))
            .unwrap()
            .contains("Waiting")
    });
    assert_eq!(
        fs::read(&fixture.request.runtime_executable).unwrap(),
        original
    );
    assert!(parent.0.try_wait().unwrap().is_none());

    // Another installation cannot update while this helper owns the user lock.
    let mut duplicate = other.helper();
    other.wait_helper(&mut duplicate, false);
    assert!(
        fs::read_to_string(other.root.join("updater.log"))
            .unwrap()
            .contains("Another YAAS updater")
    );
    assert!(other.request_path.exists());

    fs::write(fixture.root.join("role"), "updated").unwrap();
    fs::write(fixture.root.join("exit-parent"), "exit").unwrap();
    parent.0.wait().unwrap();
    fixture.wait_helper(&mut helper, true);
    wait_until(|| fixture.root.join("relaunched").exists());
    assert!(!fixture.request_path.exists());
    assert!(!fixture.request.package.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(fixture.request.executable())
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }

    // Force-stop a stuck YAAS, another instance, and bundled ADB, while keeping external ADB alive.
    let mut fixture = Fixture::new();
    let mut parent = fixture.parent();
    let mut second_app = fixture.spawn_app(&fixture.request.runtime_executable);
    let bundled = fixture
        .root
        .join(if cfg!(windows) { "adb.exe" } else { "adb" });
    fs::copy(std::env::current_exe().unwrap(), &bundled).unwrap();
    let mut adb = fixture.spawn_app(&bundled);
    fixture.request.bundled_executables.push(bundled);
    fixture.save();
    let external = Fixture::new();
    let external_adb = external
        .root
        .join(if cfg!(windows) { "adb.exe" } else { "adb" });
    fs::copy(std::env::current_exe().unwrap(), &external_adb).unwrap();
    let mut external_process = external.spawn_app(&external_adb);
    fs::write(fixture.root.join("role"), "updated").unwrap();
    let mut helper = fixture.helper();
    fixture.wait_helper(&mut helper, true);
    assert!(!parent.0.wait().unwrap().success());
    assert!(!second_app.0.wait().unwrap().success());
    assert!(!adb.0.wait().unwrap().success());
    assert!(external_process.0.try_wait().unwrap().is_none());
    wait_until(|| fixture.root.join("relaunched").exists());

    // The parent can exit before the helper starts.
    let mut fixture = Fixture::new();
    let mut parent = fixture.parent();
    fs::write(fixture.root.join("exit-parent"), "exit").unwrap();
    parent.0.wait().unwrap();
    fs::write(fixture.root.join("role"), "updated").unwrap();
    let mut helper = fixture.helper();
    fixture.wait_helper(&mut helper, true);
    wait_until(|| fixture.root.join("relaunched").exists());

    // A malformed package leaves the installed files and failed request in place.
    let fixture = Fixture::new();
    let old = fs::read(&fixture.request.runtime_executable).unwrap();
    fs::write(&fixture.request.package, "broken package").unwrap();
    let mut helper = fixture.helper();
    fixture.wait_helper(&mut helper, false);
    assert_eq!(fs::read(&fixture.request.runtime_executable).unwrap(), old);
    assert!(fixture.request_path.exists());
    assert!(fixture.request.package.exists());
    // A launch failure leaves the newly installed files in place, with no rollback.
    let mut fixture = Fixture::new();
    fixture.request.working_directory = fixture.root.join("missing-directory");
    fixture.save();
    let mut helper = fixture.helper();
    fixture.wait_helper(&mut helper, false);
    assert!(
        fs::read_to_string(fixture.root.join("updater.log"))
            .unwrap()
            .contains("Cannot launch YAAS")
    );
    assert!(fixture.request.executable().is_file());
    assert!(fixture.request_path.exists());
    if cfg!(windows) {
        assert!(fixture.request.target.join("data/new-asset").exists());
    } else if cfg!(target_os = "macos") {
        assert!(
            fixture
                .request
                .target
                .join("Contents/Resources/yaas-build.json")
                .exists()
        );
    } else {
        assert_eq!(
            fs::read(fixture.request.executable()).unwrap(),
            fs::read(&fixture.request.package).unwrap()
        );
    }

    #[cfg(target_os = "linux")]
    appimage_mounts();
}

#[cfg(target_os = "linux")]
fn appimage_mounts() {
    let mut fixture = Fixture::new();
    let first = fixture.root.join("mount-one");
    let second = fixture.root.join("mount-two");
    let unrelated = fixture.root.join("mount-other-image");
    for mount in [&first, &second, &unrelated] {
        fs::create_dir_all(mount.join("usr/bin")).unwrap();
        for name in ["yaas", "usr/bin/adb"] {
            fs::copy(std::env::current_exe().unwrap(), mount.join(name)).unwrap();
        }
    }
    fixture.request.runtime_executable = first.join("yaas");
    fixture.request.bundled_executables = vec![first.join("usr/bin/adb")];
    fixture.request.appdir = Some(first.clone());
    fixture.save();
    let spawn = |mount: &Path, image: &Path, binary: &str| {
        let child = Command::new(mount.join(binary))
            .args(ARGUMENTS)
            .env("APPIMAGE", image)
            .env("APPDIR", mount)
            .current_dir(&fixture.root)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let child = Running(child);
        wait_until(|| fixture.root.join(format!("ready-{pid}")).exists());
        child
    };
    let mut parent = spawn(&first, &fixture.request.target, "yaas");
    let mut another = spawn(&second, &fixture.request.target, "yaas");
    let mut adb = spawn(&second, &fixture.request.target, "usr/bin/adb");
    let other_image = fixture.root.join("other.AppImage");
    let mut external = spawn(&unrelated, &other_image, "yaas");
    let mut external_adb = spawn(&unrelated, &other_image, "usr/bin/adb");
    fs::write(fixture.root.join("role"), "updated").unwrap();
    let mut helper = fixture.helper();
    fixture.wait_helper(&mut helper, true);
    assert!(!parent.0.wait().unwrap().success());
    assert!(!another.0.wait().unwrap().success());
    assert!(!adb.0.wait().unwrap().success());
    assert!(external.0.try_wait().unwrap().is_none());
    assert!(external_adb.0.try_wait().unwrap().is_none());
    wait_until(|| fixture.root.join("relaunched").exists());
}
