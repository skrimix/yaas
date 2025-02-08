use anyhow::{Context, Result};
use ubyte::ByteUnit;

pub static SPACE_INFO_COMMAND: &str = "stat -fc %S:%b:%a /data";

#[derive(Clone, Debug, Default)]
pub struct SpaceInfo {
    pub total: ByteUnit,
    pub available: ByteUnit,
}

impl SpaceInfo {
    pub fn from_adb_output(output: &str) -> Result<Self> {
        // block_size:total_blocks:available_blocks
        let mut parts = output.trim().split(':');
        let block_size: u64 = parts
            .next()
            .context("failed to get block size")?
            .parse()
            .context("failed to parse block size")?;
        let total_blocks: u64 = parts
            .next()
            .context("failed to get total blocks")?
            .parse()
            .context("failed to parse total blocks")?;
        let available_blocks: u64 = parts
            .next()
            .context("failed to get available blocks")?
            .parse()
            .context("failed to parse available blocks")?;
        Ok(Self {
            total: ByteUnit::Byte(block_size * total_blocks),
            available: ByteUnit::Byte(block_size * available_blocks),
        })
    }
}
