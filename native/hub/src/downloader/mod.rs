mod rclone;
pub(crate) use rclone::TransferStats;
mod cloud_api;
pub(crate) mod config;
pub(crate) mod controller;
pub(crate) mod download_metadata;
mod http_cache;
pub(crate) mod manager;
mod repo;
mod service;
pub(crate) use service::Downloader;
pub(crate) mod downloads_catalog;
pub(crate) mod sources;

#[derive(Debug, Clone)]
pub(crate) enum AppDownloadProgress {
    Status(String),
    Transfer(TransferStats),
}
