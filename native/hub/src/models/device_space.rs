use anyhow::{Context, Result, ensure};
use ubyte::ByteUnit;

pub static SPACE_INFO_COMMAND: &str = "stat -fc %S:%b:%a /data";

/// Represents storage space information for a device
///
/// Contains information about total and available storage space
/// measured in bytes using the ByteUnit type.
#[derive(Clone, Debug, Default)]
pub struct SpaceInfo {
    /// Total storage space in bytes
    pub total: ByteUnit,
    /// Available storage space in bytes
    pub available: ByteUnit,
}

impl SpaceInfo {
    /// Creates a new SpaceInfo instance from `SPACE_INFO_COMMAND`
    pub fn from_stat_output(output: &str) -> Result<Self> {
        // block_size:total_blocks:available_blocks
        let parts = output.trim().split(':').collect::<Vec<&str>>();

        ensure!(parts.len() == 3, "invalid stat output: {}", output);

        let block_size: u64 = parts[0].parse().context("failed to parse block size")?;
        let total_blocks: u64 = parts[1].parse().context("failed to parse total blocks")?;
        let available_blocks: u64 = parts[2].parse().context("failed to parse available blocks")?;

        // A small sanity check
        ensure!(available_blocks <= total_blocks, "available blocks cannot exceed total blocks");

        Ok(Self {
            total: ByteUnit::Byte(
                block_size.checked_mul(total_blocks).context("total space overflow")?,
            ),
            available: ByteUnit::Byte(
                block_size.checked_mul(available_blocks).context("available space overflow")?,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_space_info() {
        let output = "4096:1000000:500000";
        let info = SpaceInfo::from_stat_output(output).unwrap();
        assert_eq!(info.total, ByteUnit::Byte(4096 * 1000000));
        assert_eq!(info.available, ByteUnit::Byte(4096 * 500000));
    }

    #[test]
    fn test_invalid_format() {
        let output = "4096:1000000";
        assert!(SpaceInfo::from_stat_output(output).is_err());
    }

    #[test]
    fn test_invalid_numbers() {
        let output = "4096:abc:500000";
        assert!(SpaceInfo::from_stat_output(output).is_err());
    }

    #[test]
    fn test_available_exceeds_total() {
        let output = "4096:1000000:2000000";
        assert!(SpaceInfo::from_stat_output(output).is_err());
    }

    #[test]
    fn test_overflow() {
        let output = format!("{}:{}:{}", u64::MAX, u64::MAX, u64::MAX);
        assert!(SpaceInfo::from_stat_output(&output).is_err());
    }
}
