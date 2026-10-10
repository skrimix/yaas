//! Renders the small platform scripts that replace the installation after the
//! app exits. Rendering is pure text generation so every variant is testable
//! on any host.

use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};

/// What the script puts in place of the install target. Each variant is only
/// constructed on its own platform; rendering stays testable everywhere.
#[derive(Debug, Clone)]
pub(crate) enum Payload {
    /// Staged directory copied over the install directory (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    Directory(PathBuf),
    /// The downloaded AppImage (Linux); the same file as [`ScriptPlan::package`].
    /// `mode` holds the installed AppImage's permission bits.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    AppImage { mode: u32 },
    /// Staged `YAAS.app` bundle (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Bundle(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct ScriptPlan {
    /// Directory holding the staged payload and `update.log`; removed on success.
    pub staging: PathBuf,
    /// Downloaded package; removed on success.
    pub package: PathBuf,
    pub payload: Payload,
    /// Install directory (Windows), AppImage path (Linux), or bundle path (macOS).
    pub target: PathBuf,
    /// Application process that must exit before files are replaced.
    pub pid: u32,
    /// Relaunch with `--portable`.
    pub portable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptKind {
    Windows,
    Linux,
    MacOs,
}

impl ScriptKind {
    pub(crate) fn host() -> Self {
        if cfg!(windows) {
            ScriptKind::Windows
        } else if cfg!(target_os = "macos") {
            ScriptKind::MacOs
        } else {
            ScriptKind::Linux
        }
    }

    pub(crate) fn file_name(self, candidate_id: &str) -> String {
        let extension = match self {
            ScriptKind::Windows => "bat",
            ScriptKind::Linux | ScriptKind::MacOs => "sh",
        };
        format!("update-{candidate_id}.{extension}")
    }
}

/// Directory the script creates with `mkdir` while it replaces `target`, so only
/// one install runs per target. It lives inside the install directory on
/// Windows and next to the AppImage or app bundle elsewhere.
pub(crate) fn lock_path(kind: ScriptKind, target: &Path) -> PathBuf {
    match kind {
        ScriptKind::Windows => target.join(".yaas-update.lock"),
        ScriptKind::Linux | ScriptKind::MacOs => {
            let mut name = target.file_name().unwrap_or_default().to_os_string();
            name.push(".yaas-update.lock");
            target.with_file_name(name)
        }
    }
}

/// Batch files need literal `%` doubled and CRLF line endings.
fn bat_path(path: &Path) -> String {
    path.display().to_string().replace('%', "%%")
}

/// Single-quotes a value for POSIX shells.
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

pub(crate) fn render(kind: ScriptKind, plan: &ScriptPlan) -> Result<String> {
    match kind {
        ScriptKind::Windows => render_windows(plan),
        ScriptKind::Linux => render_linux(plan),
        ScriptKind::MacOs => render_macos(plan),
    }
}

fn render_windows(plan: &ScriptPlan) -> Result<String> {
    let Payload::Directory(source) = &plan.payload else {
        anyhow::bail!("Windows update requires a staged directory");
    };
    let portable = if plan.portable { " --portable" } else { "" };
    let lines = [
        "@echo off".to_string(),
        "setlocal".to_string(),
        format!("set \"PID={}\"", plan.pid),
        format!("set \"SRC={}\"", bat_path(source)),
        format!("set \"DST={}\"", bat_path(&plan.target)),
        format!("set \"PKG={}\"", bat_path(&plan.package)),
        format!("set \"STAGING={}\"", bat_path(&plan.staging)),
        format!("set \"LOCK={}\"", bat_path(&lock_path(ScriptKind::Windows, &plan.target))),
        "set \"LOG=%STAGING%\\update.log\"".to_string(),
        String::new(),
        "echo [update] Waiting for YAAS (PID %PID%) to exit... > \"%LOG%\"".to_string(),
        "set /a TRIES=0".to_string(),
        ":wait".to_string(),
        "tasklist /FI \"PID eq %PID%\" /FO CSV /NH 2>nul | findstr /C:\"\\\",\\\"%PID%\\\",\"\" \
         >nul"
            .to_string(),
        "if errorlevel 1 goto exited".to_string(),
        "set /a TRIES+=1".to_string(),
        "if %TRIES% GEQ 20 goto kill".to_string(),
        "ping 127.0.0.1 -n 2 >nul".to_string(),
        "goto wait".to_string(),
        ":kill".to_string(),
        "echo [update] YAAS did not exit; killing >> \"%LOG%\"".to_string(),
        "taskkill /F /PID %PID% >> \"%LOG%\" 2>&1".to_string(),
        "ping 127.0.0.1 -n 3 >nul".to_string(),
        ":exited".to_string(),
        "mkdir \"%LOCK%\" 2>nul || goto busy".to_string(),
        "echo [update] Installing update... >> \"%LOG%\"".to_string(),
        "robocopy \"%SRC%\" \"%DST%\" /E /R:5 /W:1 /NFL /NDL /NP >> \"%LOG%\" 2>&1".to_string(),
        "if errorlevel 8 goto failed".to_string(),
        "echo [update] Starting new version >> \"%LOG%\"".to_string(),
        format!("start \"\" \"%DST%\\yaas.exe\"{portable}"),
        "del /f /q \"%PKG%\" >nul 2>&1".to_string(),
        "rmdir \"%LOCK%\" >nul 2>&1".to_string(),
        "cd /d \"%TEMP%\"".to_string(),
        "rmdir /s /q \"%STAGING%\" >nul 2>&1".to_string(),
        "(goto) 2>nul & del /f /q \"%~f0\"".to_string(),
        ":failed".to_string(),
        "echo [update] Installation failed >> \"%LOG%\"".to_string(),
        "rmdir \"%LOCK%\" >nul 2>&1".to_string(),
        "start \"\" notepad.exe \"%LOG%\"".to_string(),
        "exit /b 1".to_string(),
        ":busy".to_string(),
        "echo [update] Another update is being installed; skipping >> \"%LOG%\"".to_string(),
        "exit /b 1".to_string(),
        String::new(),
    ];
    Ok(lines.join("\r\n"))
}

/// Shared wait-for-exit and locking block for the POSIX scripts.
fn sh_wait_and_lock() -> Vec<String> {
    [
        "echo \"[update] Waiting for YAAS (PID $PID) to exit...\"",
        "i=0",
        "while kill -0 \"$PID\" 2>/dev/null; do",
        "    i=$((i + 1))",
        "    if [ \"$i\" -ge 150 ]; then",
        "        echo \"[update] YAAS did not exit; killing\"",
        "        kill -9 \"$PID\" 2>/dev/null",
        "        sleep 1",
        "        break",
        "    fi",
        "    sleep 0.2",
        "done",
        "if ! mkdir \"$LOCK\" 2>/dev/null; then",
        "    echo \"[update] Another update is being installed; skipping\"",
        "    exit 1",
        "fi",
        "",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn sh_header(kind: ScriptKind, plan: &ScriptPlan) -> Vec<String> {
    vec![
        "#!/bin/sh".to_string(),
        "# Generated by YAAS self-update; deletes itself on success.".to_string(),
        format!("PID={}", plan.pid),
        format!("PKG={}", sh_quote(&plan.package)),
        format!("STAGING={}", sh_quote(&plan.staging)),
        format!("LOCK={}", sh_quote(&lock_path(kind, &plan.target))),
        "LOG=\"$STAGING/update.log\"".to_string(),
        "exec >>\"$LOG\" 2>&1".to_string(),
        String::new(),
    ]
}

/// Shared success/cleanup tail for the POSIX scripts. `on_failure` runs when the
/// install step failed (the old installation may be gone or partially replaced).
fn sh_cleanup(on_failure: Option<&str>) -> Vec<String> {
    let mut lines: Vec<String> = [
        "    rm -f \"$PKG\"",
        "    rm -rf \"$STAGING\"",
        "    rm -f \"$0\"",
        "else",
        "    echo \"[update] FAILED\"",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if let Some(failure) = on_failure {
        lines.push(format!("    {failure}"));
    }
    lines.extend(["fi".to_string(), "rmdir \"$LOCK\"".to_string(), String::new()]);
    lines
}

fn render_linux(plan: &ScriptPlan) -> Result<String> {
    let Payload::AppImage { mode } = plan.payload else {
        anyhow::bail!("Linux update requires the AppImage package");
    };
    ensure!(mode <= 0o777, "Invalid AppImage permissions");
    let portable = if plan.portable { " --portable" } else { "" };
    let mut lines = sh_header(ScriptKind::Linux, plan);
    lines.push(format!("TARGET={}", sh_quote(&plan.target)));
    lines.push(String::new());
    lines.extend(sh_wait_and_lock());
    lines.extend(
        [
            "echo \"[update] Installing update\"",
            "TMP=\"$TARGET.yaas-update\"",
            &format!(
                "if cp \"$PKG\" \"$TMP\" && chmod {mode:o} \"$TMP\" && mv -f \"$TMP\" \
                 \"$TARGET\"; then"
            ),
            "    echo \"[update] Starting new version\"",
            &format!("    nohup \"$TARGET\"{portable} </dev/null >/dev/null 2>&1 &"),
        ]
        .into_iter()
        .map(String::from),
    );
    // Remove a partial copy on failure.
    lines.extend(sh_cleanup(Some("rm -f \"$TMP\"")));
    Ok(lines.join("\n"))
}

fn render_macos(plan: &ScriptPlan) -> Result<String> {
    let Payload::Bundle(staged) = &plan.payload else {
        anyhow::bail!("macOS update requires a staged app bundle");
    };
    let mut lines = sh_header(ScriptKind::MacOs, plan);
    lines.push(format!("STAGED={}", sh_quote(staged)));
    lines.push(format!("TARGET={}", sh_quote(&plan.target)));
    lines.push(String::new());
    lines.extend(sh_wait_and_lock());
    lines.extend(
        [
            "echo \"[update] Installing update\"",
            "if rm -rf \"$TARGET\" && ditto \"$STAGED\" \"$TARGET\"; then",
            "    echo \"[update] Starting new version\"",
            "    open \"$TARGET\"",
        ]
        .into_iter()
        .map(String::from),
    );
    lines.extend(sh_cleanup(None));
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> ScriptPlan {
        ScriptPlan {
            staging: PathBuf::from("/home/user/.local/share/YAAS/updates/staging-id with spaces"),
            package: PathBuf::from("/home/user/.local/share/YAAS/updates/downloads/id/package"),
            payload: Payload::AppImage { mode: 0o750 },
            target: PathBuf::from("/home/user/Applications/YAAS App.AppImage"),
            pid: 4242,
            portable: true,
        }
    }

    #[test]
    fn windows_script_replaces_install_dir_and_preserves_portable_flag() {
        let mut plan = plan();
        plan.payload = Payload::Directory(PathBuf::from(
            r"C:\Users\user\AppData\Local\YAAS\updates\staging-id\new",
        ));
        plan.target = PathBuf::from(r"C:\Users\user\Apps\YAAS");
        plan.staging = PathBuf::from(r"C:\Users\user\AppData\Local\YAAS\updates\staging-id");
        plan.package =
            PathBuf::from(r"C:\Users\user\AppData\Local\YAAS\updates\downloads\id\package");
        let script = render(ScriptKind::Windows, &plan).unwrap();
        assert!(script.contains("set \"PID=4242\""));
        // The wait loop must match the exact PID column of tasklist's CSV output.
        assert!(script.contains(
            "tasklist /FI \"PID eq %PID%\" /FO CSV /NH 2>nul | findstr \
             /C:\"\\\",\\\"%PID%\\\",\"\" >nul"
        ));
        assert!(script.contains("robocopy \"%SRC%\" \"%DST%\" /E /R:5 /W:1"));
        assert!(script.contains("start \"\" \"%DST%\\yaas.exe\" --portable"));
        assert!(script.contains("taskkill /F /PID %PID%"));
        assert!(script.contains("del /f /q \"%~f0\""));
        // The lock lives inside the install directory (separator depends on the host).
        assert!(script.contains(r#"set "LOCK=C:\Users\user\Apps\YAAS"#));
        assert!(script.contains(r#".yaas-update.lock""#));
        assert!(script.contains("mkdir \"%LOCK%\" 2>nul || goto busy"));
        // Batch files must use CRLF consistently; cmd mis-parses lone LF in some cases.
        assert!(script.contains("\r\n"));
        assert_eq!(script.replace("\r\n", "").matches(['\r', '\n']).count(), 0);
        // No /PURGE: portable data and extra files survive.
        assert!(!script.contains("/PURGE") && !script.contains("/MIR"));
    }

    #[test]
    fn windows_script_escapes_percents_for_batch_parsing() {
        let mut plan = plan();
        plan.payload = Payload::Directory(PathBuf::from(r"C:\100% legit\new"));
        plan.target = PathBuf::from(r"C:\100% legit\YAAS");
        let script = render(ScriptKind::Windows, &plan).unwrap();
        assert!(script.contains(r#"set "DST=C:\100%% legit\YAAS""#));
    }

    #[test]
    fn linux_script_replaces_appimage_atomically_and_relaunches() {
        let script = render(ScriptKind::Linux, &plan()).unwrap();
        assert!(script.contains("PID=4242"));
        assert!(script.contains("PKG='/home/user/.local/share/YAAS/updates/downloads/id/package'"));
        assert!(script.contains("TARGET='/home/user/Applications/YAAS App.AppImage'"));
        assert!(script.contains("mv -f \"$TMP\" \"$TARGET\""));
        assert!(script.contains("nohup \"$TARGET\" --portable"));
        assert!(script.contains("chmod 750 \"$TMP\""));
        assert!(
            script.contains("LOCK='/home/user/Applications/YAAS App.AppImage.yaas-update.lock'")
        );
        assert!(script.contains("if ! mkdir \"$LOCK\""));
        assert!(script.contains("rmdir \"$LOCK\""));
        assert!(script.contains("kill -9 \"$PID\""));
        assert!(script.contains("rm -f \"$0\""));
        assert!(!script.contains("\r"));
    }

    #[test]
    fn macos_script_replaces_bundle_with_ditto() {
        let mut plan = plan();
        plan.payload = Payload::Bundle(PathBuf::from(
            "/Users/user/Library/Application Support/YAAS/updates/staging-id/YAAS.app",
        ));
        plan.target = PathBuf::from("/Applications/YAAS.app");
        plan.portable = false;
        let script = render(ScriptKind::MacOs, &plan).unwrap();
        assert!(script.contains("rm -rf \"$TARGET\" && ditto \"$STAGED\" \"$TARGET\""));
        assert!(script.contains("open \"$TARGET\""));
        assert!(script.contains("LOCK='/Applications/YAAS.app.yaas-update.lock'"));
        assert!(!script.contains("--portable"));
    }

    #[test]
    fn mismatched_payload_is_rejected() {
        assert!(render(ScriptKind::Windows, &plan()).is_err());
        let mut plan = plan();
        plan.payload = Payload::Directory(PathBuf::from("/x"));
        assert!(render(ScriptKind::Linux, &plan).is_err());
        assert!(render(ScriptKind::MacOs, &plan).is_err());
    }

    #[test]
    fn sh_quote_handles_single_quotes() {
        assert_eq!(sh_quote(Path::new("/a/b'c")), "'/a/b'\\''c'");
    }
}
