//! Installs an update after the application exits.

use anyhow::{Context, Result};
use app_update::{
    install::{self, InstallRequest},
    processes,
};
use std::{fs, path::PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("YAAS update failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("Expected update request file")?,
    );
    let _lock = install::lock()?;
    let request = InstallRequest::read(&path)?;
    eprintln!("Waiting for YAAS to exit");
    processes::stop_installation(&request)?;
    eprintln!("Installing update");
    request.install()?;
    request.launch()?;
    // No acknowledgement: successful spawning is the end of the update.
    fs::remove_file(path)?;
    if let Err(error) = fs::remove_file(&request.package) {
        eprintln!("Could not remove downloaded package: {error}");
    }
    eprintln!("Update installed and YAAS started");
    Ok(())
}
