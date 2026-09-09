use app_update::{
    Identity,
    transaction::{Phase, Transaction, replacement},
};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// Spawn this test executable as a stand-in for YAAS on every desktop platform.
#[test]
#[ignore]
fn fixture_app() {
    let root = std::env::current_dir().unwrap();
    let Ok(role) = fs::read_to_string(root.join("role")) else {
        return;
    };
    if role == "parent" {
        fs::write(root.join("parent-ready"), "ready").unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        while !root.join("exit-parent").exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        let tx = Transaction::read(&root).unwrap();
        if role == "updated" {
            app_update::transaction::acknowledge(&root, &tx.expected).unwrap();
        }
        fs::write(root.join("relaunched"), "yes").unwrap();
    }
}

fn wait_for(path: &Path) {
    let until = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < until,
            "Timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_helper(commit: bool, bad_executable: bool) {
    let root = tempfile::Builder::new()
        .prefix("yaas update spaces ")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let extension = if cfg!(windows) { ".exe" } else { "" };
    let app = root.path().join(format!("yaas{extension}"));
    let new = workspace.join(format!("new{extension}"));
    fs::copy(std::env::current_exe().unwrap(), &app).unwrap();
    if bad_executable {
        fs::write(&new, "not an executable").unwrap();
    } else {
        fs::copy(std::env::current_exe().unwrap(), &new).unwrap();
    }
    fs::write(root.path().join("role"), "parent").unwrap();
    let arguments = ["--ignored", "--exact", "fixture_app", "--nocapture"];
    let mut parent = Command::new(&app)
        .args(arguments)
        .current_dir(root.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&root.path().join("parent-ready"));
    let tx = Transaction {
        id: "integration".into(),
        expected: Identity {
            version: "2.0.0".into(),
            build_number: 1,
            channel: "stable".into(),
            commit: "a".repeat(40),
            run_number: 2,
            run_attempt: 1,
        },
        parent_pid: parent.id(),
        installation_lock: root.path().join("installation.lock"),
        workspace: workspace.clone(),
        executable: app.clone(),
        arguments: arguments.into_iter().map(Into::into).collect(),
        working_directory: root.path().into(),
        replacements: vec![replacement(
            app.clone(),
            Some(new),
            workspace.join("backup"),
        )],
        phase: Phase::Prepared,
        error: None,
    };
    tx.save(root.path()).unwrap();
    let mut helper = Command::new(env!("CARGO_BIN_EXE_yaas-updater"))
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(helper.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "READY");
    if commit {
        helper.stdin.take().unwrap().write_all(b"COMMIT\n").unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            Transaction::read(root.path()).unwrap().phase,
            Phase::Prepared
        );
        fs::write(
            root.path().join("role"),
            if bad_executable {
                "restored"
            } else {
                "updated"
            },
        )
        .unwrap();
    } else {
        drop(helper.stdin.take());
    }
    fs::write(root.path().join("exit-parent"), "exit").unwrap();
    parent.wait().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if helper.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= until {
            helper.kill().unwrap();
            panic!("Helper did not finish");
        }
        thread::sleep(Duration::from_millis(20));
    }
    if commit {
        wait_for(&root.path().join("relaunched"));
        assert_eq!(
            Transaction::read(root.path()).unwrap().phase,
            if bad_executable {
                Phase::RolledBack
            } else {
                Phase::Launched
            }
        );
        if !bad_executable {
            assert!(!workspace.exists());
        }
        assert_eq!(
            fs::read(&app).unwrap(),
            fs::read(std::env::current_exe().unwrap()).unwrap()
        );
    } else {
        assert_eq!(
            Transaction::read(root.path()).unwrap().phase,
            Phase::Prepared
        );
    }
}

#[test]
fn helper_waits_for_exit_relaunches_and_cleans_accepted_backup() {
    run_helper(true, false);
}
#[test]
fn helper_cancellation_does_not_replace_files() {
    run_helper(false, false);
}
#[test]
fn helper_rolls_back_failed_launch_and_restarts_previous_app() {
    run_helper(true, true);
}
