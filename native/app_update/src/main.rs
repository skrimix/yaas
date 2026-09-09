//! Runs a prepared update after the application exits.

use anyhow::{Context, Result, ensure};
use app_update::transaction::{Phase, Transaction, remove};
use std::{
    fs,
    io::{BufRead, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("YAAS update failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("Expected transaction directory")?,
    );
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("helper.lock"))?;
    lock.try_lock()
        .context("Update helper is already running")?;
    let mut transaction = Transaction::read(&directory)?;
    let installation_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&transaction.installation_lock)?;
    installation_lock
        .try_lock()
        .context("Another update is using this installation")?;
    let parent = Parent::open(transaction.parent_pid)?;
    transaction.preflight()?;
    println!("READY");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim() != "COMMIT" {
        return Ok(());
    }
    let until = Instant::now() + Duration::from_secs(60);
    while !parent.exited()? {
        if Instant::now() >= until {
            transaction.error = Some("YAAS did not exit; installation was cancelled".into());
            transaction.save(&directory)?;
            anyhow::bail!("YAAS did not exit");
        }
        thread::sleep(Duration::from_millis(100));
    }
    #[cfg(windows)]
    if let Some(parent) = transaction.executable.parent() {
        if let Err(error) = app_update::adb::stop_bundled_server(&parent.join("adb.exe")) {
            eprintln!("Could not stop bundled ADB: {error:#}");
        }
    }
    let result = transaction
        .apply(&directory)
        .and_then(|_| transaction.launch());
    if let Err(error) = result {
        transaction.error = Some(format!("{error:#}"));
        transaction.rollback(&directory)?;
        if let Err(launch_error) = transaction.launch() {
            transaction.error = Some(format!(
                "{error:#}; previous app could not start: {launch_error:#}"
            ));
            transaction.save(&directory)?;
        }
        return Err(error);
    }
    // The new app may already be running. A journal write failure must not
    // replace files underneath it; Launching is sufficient for acknowledgement.
    transaction.phase = Phase::Launched;
    if let Err(error) = transaction.save(&directory) {
        eprintln!("Could not record launch: {error:#}");
    }
    // A timeout retains the backup; it never rolls back a running application.
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        if directory.join("accepted.json").exists() {
            remove(&transaction.workspace)?;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

#[cfg(unix)]
struct Parent(u32);
#[cfg(unix)]
impl Parent {
    fn open(pid: u32) -> Result<Self> {
        ensure!(pid > 1 && pid <= i32::MAX as u32, "Invalid parent PID");
        Ok(Self(pid))
    }
    fn exited(&self) -> Result<bool> {
        // A reused PID can delay replacement, but cannot cause replacement before exit.
        if unsafe { libc::kill(self.0 as i32, 0) } == 0 {
            return Ok(false);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(true)
        } else {
            Err(error.into())
        }
    }
}

#[cfg(windows)]
struct Parent(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Parent {
    fn open(pid: u32) -> Result<Self> {
        let handle =
            unsafe { windows_sys::Win32::System::Threading::OpenProcess(0x00100000, 0, pid) };
        ensure!(
            !handle.is_null(),
            "Cannot wait for parent process: {}",
            std::io::Error::last_os_error()
        );
        Ok(Self(handle))
    }
    fn exited(&self) -> Result<bool> {
        let status =
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0) };
        match status {
            0 => Ok(true),
            258 => Ok(false),
            _ => anyhow::bail!("Cannot wait for YAAS process"),
        }
    }
}
#[cfg(windows)]
impl Drop for Parent {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
