//! Release metadata (`release.json`) and version ordering.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Identity {
    pub version: String,
    pub build_number: u16,
    pub channel: String,
    pub commit: String,
    pub run_number: u64,
    pub run_attempt: u64,
}

impl Identity {
    pub fn newer_than(&self, installed: &Self) -> Result<bool> {
        // Changed channel always shows an update
        if self.channel != installed.channel {
            return Ok(true);
        }
        if self.channel == "nightly" {
            return Ok(
                (self.run_number, self.run_attempt) > (installed.run_number, installed.run_attempt)
            );
        }
        Ok((version(&self.version)?, self.build_number)
            > (version(&installed.version)?, installed.build_number))
    }
}

fn version(value: &str) -> Result<[u16; 3]> {
    let parts = value
        .split('.')
        .map(str::parse)
        .collect::<Result<Vec<u16>, _>>()
        .with_context(|| format!("Invalid version: {value}"))?;
    parts.try_into().map_err(|_| anyhow::anyhow!("Invalid version: {value}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Asset {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Manifest {
    #[serde(flatten)]
    pub identity: Identity,
    pub assets: Vec<Asset>,
}

/// Name of the release asset that updates this platform.
pub(crate) fn package_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("YAAS-windows-x64.zip"),
        ("linux", "x86_64") => Some("YAAS-linux-x86_64.AppImage"),
        ("macos", "x86_64" | "aarch64") => Some("YAAS-macos.zip"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(channel: &str, version: &str, build: u16, run: u64, attempt: u64) -> Identity {
        Identity {
            version: version.into(),
            build_number: build,
            channel: channel.into(),
            commit: "a".repeat(40),
            run_number: run,
            run_attempt: attempt,
        }
    }

    #[test]
    fn ordering_and_channel_switches() {
        let stable = identity("stable", "1.9.0", 20, 50, 1);
        assert!(identity("stable", "1.10.0", 1, 1, 1).newer_than(&stable).unwrap());
        assert!(!identity("stable", "1.8.0", 100, 100, 1).newer_than(&stable).unwrap());
        assert!(identity("stable", "1.9.0", 21, 1, 1).newer_than(&stable).unwrap());
        assert!(!stable.newer_than(&stable).unwrap());
        let nightly = identity("nightly", "1.0.0", 1, 50, 2);
        assert!(nightly.newer_than(&stable).unwrap());
        assert!(stable.newer_than(&nightly).unwrap());
        assert!(nightly.newer_than(&identity("nightly", "9.0.0", 1, 50, 1)).unwrap());
        assert!(!nightly.newer_than(&identity("nightly", "0.1.0", 1, 51, 1)).unwrap());
    }

    #[test]
    fn rejects_malformed_versions() {
        for v in ["1.0", "1.0.0-beta", "65536.0.0", "1.0.-1", "1.0.0.0"] {
            assert!(version(v).is_err(), "{v}");
        }
        assert_eq!(version("1.10.0").unwrap(), [1, 10, 0]);
    }
}
