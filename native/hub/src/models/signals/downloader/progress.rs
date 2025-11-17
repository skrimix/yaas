use rinf::RustSignal;

#[derive(serde::Serialize, serde::Deserialize, RustSignal)]
pub(crate) struct DownloaderInitProgress {
    /// Bytes downloaded so far
    pub bytes: u64,
    /// Total bytes if known
    pub total_bytes: Option<u64>,
}
