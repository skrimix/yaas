//! System proxy detection.
//!
//! Produces a single proxy URL (`http://host:port` or `socks5://host:port`) that is
//! passed to reqwest and exported as `http_proxy`/`https_proxy` for rclone.
//! When several protocols are configured, HTTPS is preferred, then HTTP, then SOCKS.

use std::error::Error;

use tracing::{debug, instrument, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyKind {
    Http,
    Socks,
}

/// Returns the system proxy URL if one is configured and enabled.
#[instrument(level = "debug")]
pub(crate) fn get_sys_proxy() -> Option<String> {
    match platform::read_proxy() {
        Ok(Some(proxy)) => {
            debug!(proxy, "Got system proxy");
            Some(proxy)
        }
        Ok(None) => {
            debug!("No system proxy configured");
            None
        }
        Err(e) => {
            warn!(error = e.as_ref() as &dyn Error, "Failed to get system proxy");
            None
        }
    }
}

/// Normalizes a proxy address into a URL with an explicit scheme.
///
/// Accepts `host:port`, `scheme://host:port` and the legacy KDE `[scheme://]host port` form.
fn proxy_url(raw: &str, kind: ProxyKind) -> Option<String> {
    let raw = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if raw.is_empty() {
        return None;
    }

    let (scheme, rest) = match raw.split_once("://") {
        Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
        None => (None, raw),
    };

    let mut parts = rest.split_whitespace();
    let address = match (parts.next(), parts.next()) {
        (Some(host), Some(port)) => format!("{}:{port}", host.trim_end_matches('/')),
        (Some(host), None) => host.trim_end_matches('/').to_string(),
        _ => return None,
    };
    if address.is_empty() || address.starts_with(':') {
        return None;
    }

    let scheme = match (kind, scheme.as_deref()) {
        (ProxyKind::Http, Some(s @ ("http" | "https"))) => s,
        (ProxyKind::Http, _) => "http",
        (ProxyKind::Socks, Some(s @ ("socks4" | "socks4a" | "socks5" | "socks5h"))) => s,
        (ProxyKind::Socks, _) => "socks5",
    };
    Some(format!("{scheme}://{address}"))
}

/// Builds a proxy URL from separate host and port values. A port of 0 means "unset".
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn host_port_url(host: &str, port: &str, kind: ProxyKind) -> Option<String> {
    let host = host.trim().trim_matches('\'');
    let port = port.trim().parse::<u16>().ok().filter(|&p| p != 0);
    match port {
        Some(port) => proxy_url(&format!("{host}:{port}"), kind),
        None => proxy_url(host, kind),
    }
}

/// Parses the WinINet `ProxyServer` registry value.
///
/// The value is either a single `host:port` used for all protocols, or a list of
/// `protocol=host:port` entries separated by `;`.
#[cfg(any(target_os = "windows", test))]
fn parse_windows_proxy_server(value: &str) -> Option<String> {
    let entries: Vec<&str> =
        value.split([';', ' ', '\t']).map(str::trim).filter(|e| !e.is_empty()).collect();

    if let Some(single) = entries.iter().find(|e| !e.contains('=')) {
        return proxy_url(single, ProxyKind::Http);
    }

    let lookup = |protocol: &str| {
        entries.iter().find_map(|entry| {
            let (key, addr) = entry.split_once('=')?;
            key.trim().eq_ignore_ascii_case(protocol).then_some(addr)
        })
    };
    [("https", ProxyKind::Http), ("http", ProxyKind::Http), ("socks", ProxyKind::Socks)]
        .into_iter()
        .find_map(|(protocol, kind)| proxy_url(lookup(protocol)?, kind))
}

/// Parses the output of `scutil --proxy`.
#[cfg(any(target_os = "macos", test))]
fn parse_scutil_proxy(output: &str) -> Option<String> {
    use std::collections::HashMap;

    let values: HashMap<&str, &str> = output
        .lines()
        .filter_map(|line| line.split_once(" : "))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();

    [("HTTPS", ProxyKind::Http), ("HTTP", ProxyKind::Http), ("SOCKS", ProxyKind::Socks)]
        .into_iter()
        .find_map(|(prefix, kind)| {
            if values.get(format!("{prefix}Enable").as_str()) != Some(&"1") {
                return None;
            }
            let host = values.get(format!("{prefix}Proxy").as_str())?;
            let port = values.get(format!("{prefix}Port").as_str()).copied().unwrap_or("");
            host_port_url(host, port, kind)
        })
}

#[cfg(target_os = "windows")]
mod platform {
    use anyhow::{Context, Result};
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ},
    };

    const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    pub(super) fn read_proxy() -> Result<Option<String>> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(INTERNET_SETTINGS, KEY_READ)
            .context("Failed to open Internet Settings registry key")?;
        let enabled = key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) != 0;
        if !enabled {
            return Ok(None);
        }
        let server = key.get_value::<String, _>("ProxyServer").unwrap_or_default();
        Ok(super::parse_windows_proxy_server(&server))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    use anyhow::{Context, Result, ensure};

    pub(super) fn read_proxy() -> Result<Option<String>> {
        let output =
            Command::new("scutil").arg("--proxy").output().context("Failed to run scutil")?;
        ensure!(output.status.success(), "scutil exited with {}", output.status);
        Ok(super::parse_scutil_proxy(&String::from_utf8_lossy(&output.stdout)))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{env, io, process::Command};

    use anyhow::{Context, Result, ensure};
    use tracing::debug;

    use super::{ProxyKind, host_port_url, proxy_url};

    pub(super) fn read_proxy() -> Result<Option<String>> {
        let is_kde = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .any(|desktop| desktop.eq_ignore_ascii_case("KDE"));
        if is_kde { read_kde() } else { read_gsettings() }
    }

    /// Builds a command for a desktop helper tool, undoing AppImage library overrides.
    fn helper_command(program: &str) -> Command {
        let mut command = Command::new(program);
        if env::var_os("APPIMAGE").is_some() {
            command.env_remove("LD_LIBRARY_PATH");
        }
        command
    }

    fn run(mut command: Command) -> io::Result<Option<String>> {
        let output = command.output()?;
        Ok(output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().into()))
    }

    fn read_kde() -> Result<Option<String>> {
        let programs = match env::var("KDE_SESSION_VERSION").as_deref() {
            Ok("5") => ["kreadconfig5", "kreadconfig6"],
            _ => ["kreadconfig6", "kreadconfig5"],
        };

        let read = |key: &str| -> Result<String> {
            let mut last_error = None;
            for program in programs {
                let mut command = helper_command(program);
                command.args(["--file", "kioslaverc", "--group", "Proxy Settings", "--key", key]);
                match run(command) {
                    Ok(Some(value)) => return Ok(value),
                    Ok(None) => return Ok(String::new()),
                    Err(e) => last_error = Some(e),
                }
            }
            Err(last_error.expect("at least one program")).context("Failed to run kreadconfig")
        };

        // 1 = manual configuration; PAC, auto-detect and env-var modes are not resolved here
        if read("ProxyType")? != "1" {
            return Ok(None);
        }
        for (key, kind) in [
            ("httpsProxy", ProxyKind::Http),
            ("httpProxy", ProxyKind::Http),
            ("socksProxy", ProxyKind::Socks),
        ] {
            if let Some(url) = proxy_url(&read(key)?, kind) {
                return Ok(Some(url));
            }
        }
        Ok(None)
    }

    fn read_gsettings() -> Result<Option<String>> {
        const SCHEMA: &str = "org.gnome.system.proxy";

        let get = |schema: &str, key: &str| -> Result<String> {
            let mut command = helper_command("gsettings");
            command.args(["get", schema, key]);
            let value = run(command).context("Failed to run gsettings")?;
            ensure!(value.is_some(), "gsettings failed to read {schema} {key}");
            Ok(value.unwrap_or_default())
        };

        let mode = match get(SCHEMA, "mode") {
            Ok(mode) => mode,
            Err(e) => {
                debug!(error = format!("{e:#}"), "gsettings is unavailable, assuming no proxy");
                return Ok(None);
            }
        };
        if mode != "'manual'" {
            return Ok(None);
        }
        for (protocol, kind) in
            [("https", ProxyKind::Http), ("http", ProxyKind::Http), ("socks", ProxyKind::Socks)]
        {
            let schema = format!("{SCHEMA}.{protocol}");
            let host = get(&schema, "host")?;
            let port = get(&schema, "port")?;
            if let Some(url) = host_port_url(&host, &port, kind) {
                return Ok(Some(url));
            }
        }
        Ok(None)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    pub(super) fn read_proxy() -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_url_formats() {
        assert_eq!(
            proxy_url("127.0.0.1:8080", ProxyKind::Http).as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            proxy_url("http://127.0.0.1 8080", ProxyKind::Http).as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            proxy_url("https://proxy.corp:3128/", ProxyKind::Http).as_deref(),
            Some("https://proxy.corp:3128")
        );
        assert_eq!(
            proxy_url("socks://127.0.0.1 1080", ProxyKind::Socks).as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        assert_eq!(
            proxy_url("'proxy.corp'", ProxyKind::Http).as_deref(),
            Some("http://proxy.corp")
        );
        assert_eq!(proxy_url("", ProxyKind::Http), None);
        assert_eq!(proxy_url("  ''  ", ProxyKind::Socks), None);
        assert_eq!(proxy_url("http://", ProxyKind::Http), None);
    }

    #[test]
    fn host_port_handles_unset_port() {
        assert_eq!(
            host_port_url("'127.0.0.1'", "8080", ProxyKind::Http).as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(host_port_url("''", "0", ProxyKind::Http), None);
        assert_eq!(
            host_port_url("proxy", "0", ProxyKind::Socks).as_deref(),
            Some("socks5://proxy")
        );
    }

    #[test]
    fn windows_proxy_server() {
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:10809").as_deref(),
            Some("http://127.0.0.1:10809")
        );
        assert_eq!(
            parse_windows_proxy_server("socks=127.0.0.1:10808").as_deref(),
            Some("socks5://127.0.0.1:10808")
        );
        assert_eq!(
            parse_windows_proxy_server("http=proxy:80;https=secure:443;socks=127.0.0.1:1080")
                .as_deref(),
            Some("http://secure:443")
        );
        assert_eq!(
            parse_windows_proxy_server("ftp=ftp:21;HTTP=http://proxy:3128").as_deref(),
            Some("http://proxy:3128")
        );
        assert_eq!(parse_windows_proxy_server("ftp=ftp:21"), None);
        assert_eq!(parse_windows_proxy_server(""), None);
    }

    #[test]
    fn scutil_output() {
        let output = "<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
  }
  FTPPassive : 1
  HTTPEnable : 0
  HTTPPort : 8080
  HTTPProxy : 10.0.0.1
  SOCKSEnable : 1
  SOCKSPort : 1080
  SOCKSProxy : 127.0.0.1
}";
        assert_eq!(parse_scutil_proxy(output).as_deref(), Some("socks5://127.0.0.1:1080"));
        assert_eq!(parse_scutil_proxy("<dictionary> {\n  HTTPEnable : 0\n}"), None);
    }
}
