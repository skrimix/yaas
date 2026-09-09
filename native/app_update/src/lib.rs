//! Release metadata and installation transactions shared with the update helper.

pub mod adb;
pub mod package;
pub mod transaction;

use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub version: String,
    pub build_number: u16,
    pub channel: String,
    pub commit: String,
    pub run_number: u64,
    pub run_attempt: u64,
}

impl Identity {
    pub fn validate(&self) -> Result<()> {
        version(&self.version)?;
        ensure!(self.build_number > 0, "Invalid build number");
        ensure!(
            matches!(self.channel.as_str(), "stable" | "nightly"),
            "Invalid channel"
        );
        ensure!(
            self.run_number > 0 && self.run_attempt > 0,
            "Missing CI identity"
        );
        ensure!(
            self.commit.len() == 40 && self.commit.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid commit"
        );
        Ok(())
    }

    pub fn newer_than(&self, installed: &Self) -> Result<bool> {
        self.validate()?;
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

pub fn version(value: &str) -> Result<[u16; 3]> {
    let parts = value
        .split('.')
        .map(|part| {
            ensure!(
                !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()),
                "Invalid version"
            );
            ensure!(part.len() == 1 || !part.starts_with('0'), "Invalid version");
            part.parse::<u16>().context("Invalid version")
        })
        .collect::<Result<Vec<_>>>()?;
    parts
        .try_into()
        .map_err(|_| anyhow::anyhow!("Expected MAJOR.MINOR.PATCH"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub os: String,
    pub architectures: Vec<String>,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    #[serde(flatten)]
    pub identity: Identity,
    pub tag: String,
    pub assets: Vec<Asset>,
}

impl Manifest {
    pub fn validate(&self, channel: &str, tag: &str) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported release metadata schema"
        );
        self.identity.validate()?;
        ensure!(
            self.identity.channel == channel && self.tag == tag,
            "Release identity mismatch"
        );
        let expected_tag = if channel == "stable" {
            format!("v{}", self.identity.version)
        } else {
            "nightly".into()
        };
        ensure!(self.tag == expected_tag, "Unexpected release tag");
        let mut names = std::collections::HashSet::new();
        for asset in &self.assets {
            ensure!(names.insert(&asset.name), "Duplicate release asset");
            ensure!(
                asset.size > 0
                    && asset.sha256.len() == 64
                    && asset.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
                "Invalid asset size or checksum"
            );
        }
        Ok(())
    }

    pub fn asset(&self, os: &str, arch: &str) -> Result<&Asset> {
        let name = match (os, arch) {
            ("windows", "x86_64") => "YAAS-windows-x64.zip",
            ("linux", "x86_64") => "YAAS-linux-x86_64.AppImage",
            ("macos", "x86_64" | "aarch64") => "YAAS-macos.zip",
            _ => anyhow::bail!("No package for {os}/{arch}"),
        };
        let matches = self
            .assets
            .iter()
            .filter(|a| a.os == os && a.architectures.iter().any(|a| a == arch))
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1 && matches[0].name == name,
            "Missing or ambiguous platform package"
        );
        Ok(matches[0])
    }
}

pub fn verify_file(path: &Path, asset: &Asset) -> Result<()> {
    let mut file = File::open(path).context("Cannot open update package")?;
    ensure!(
        file.metadata()?.len() == asset.size,
        "Package size mismatch"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    ensure!(
        const_hex::encode(hash.finalize()).eq_ignore_ascii_case(&asset.sha256),
        "Package checksum mismatch"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn identity(channel: &str, version: &str, build: u16, run: u64, attempt: u64) -> Identity {
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
        assert!(
            identity("stable", "1.10.0", 1, 1, 1)
                .newer_than(&stable)
                .unwrap()
        );
        assert!(
            !identity("stable", "1.8.0", 100, 100, 1)
                .newer_than(&stable)
                .unwrap()
        );
        assert!(
            identity("stable", "1.9.0", 21, 1, 1)
                .newer_than(&stable)
                .unwrap()
        );
        assert!(!stable.newer_than(&stable).unwrap());
        let nightly = identity("nightly", "1.0.0", 1, 50, 2);
        assert!(nightly.newer_than(&stable).unwrap());
        assert!(stable.newer_than(&nightly).unwrap());
        assert!(
            nightly
                .newer_than(&identity("nightly", "9.0.0", 1, 50, 1))
                .unwrap()
        );
        assert!(
            !nightly
                .newer_than(&identity("nightly", "0.1.0", 1, 51, 1))
                .unwrap()
        );
    }

    #[test]
    fn rejects_malformed_identity() {
        for v in ["1.0", "01.0.0", "1.0.0-beta", "65536.0.0", "1.0.-1"] {
            assert!(version(v).is_err());
        }
        let mut id = identity("stable", "1.0.0", 1, 1, 1);
        id.run_attempt = 0;
        assert!(id.validate().is_err());
    }
}
