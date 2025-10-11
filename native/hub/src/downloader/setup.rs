use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use rinf::{DartSignal, RustSignal};
use tracing::{debug, error, info, instrument};

use crate::{
    downloader,
    models::{
        DownloaderConfig,
        signals::{
            downloader::setup::{DownloaderConfigInstallResult, InstallDownloaderConfigRequest},
            system::Toast,
        },
    },
    settings::SettingsHandler,
    task::TaskManager,
};

#[instrument(skip(app_dir, settings_handler, task_manager))]
pub fn start_setup_handler(
    app_dir: PathBuf,
    settings_handler: Arc<SettingsHandler>,
    task_manager: Arc<TaskManager>,
) {
    tokio::spawn(async move {
        let receiver = InstallDownloaderConfigRequest::get_dart_signal_receiver();
        loop {
            match receiver.recv().await {
                Some(req) => {
                    let src = PathBuf::from(req.message.source_path);
                    debug!(path = %src.display(), "Received InstallDownloaderConfigRequest");
                    let res = install_config(&app_dir, &src).await;
                    match res {
                        Ok(()) => {
                            DownloaderConfigInstallResult { success: true, error: None }
                                .send_signal_to_dart();

                            Toast::send(
                                "Downloader config installed".into(),
                                "Initializing cloud features...".into(),
                                false,
                                None,
                            );

                            if let Err(e) = downloader::init_from_disk(
                                app_dir.clone(),
                                settings_handler.clone(),
                                task_manager.clone(),
                            )
                            .await
                            {
                                error!(
                                    error = e.as_ref() as &dyn Error,
                                    "Downloader init after install failed"
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                error = e.as_ref() as &dyn Error,
                                "Failed to install downloader config"
                            );
                            DownloaderConfigInstallResult {
                                success: false,
                                error: Some(format!("{:#}", e)),
                            }
                            .send_signal_to_dart();
                            Toast::send(
                                "Failed to install downloader config".into(),
                                format!("{:#}", e),
                                true,
                                None,
                            );
                        }
                    }
                }
                None => panic!("InstallDownloaderConfigRequest receiver closed"),
            }
        }
    });
}

#[instrument(skip(app_dir, src), fields(src = %src.display()), err)]
async fn install_config(app_dir: &Path, src: &Path) -> Result<()> {
    ensure!(src.exists(), "Source file not found");
    ensure!(src.is_file(), "Source path is not a file");

    // Validate by parsing
    let cfg = DownloaderConfig::load_from_path(src)?;
    info!(
        rclone_path = %cfg.rclone_path,
        rclone_config_path = %cfg.rclone_config_path,
        "Validated downloader.json"
    );

    let dst = app_dir.join("downloader.json");
    let tmp = app_dir.join("downloader.json.tmp");
    let content =
        fs::read_to_string(src).with_context(|| format!("Failed to read {}", src.display()))?;
    fs::write(&tmp, content).context("Failed to write temporary config file")?;
    fs::rename(&tmp, &dst).with_context(|| format!("Failed to replace {}", dst.display()))?;
    info!(path = %dst.display(), "Installed downloader.json");
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn valid_config_json() -> String {
        r#"{
            "rclone_path": "/bin/echo",
            "rclone_config_path": "/tmp/rclone.conf",
            "randomize_remote": false
        }"#
        .to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn install_config_copies_and_validates() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, valid_config_json()).unwrap();

        install_config(dir.path(), &src).await.expect("install ok");

        let dst = dir.path().join("downloader.json");
        assert!(dst.is_file());
        let content = std::fs::read_to_string(dst).unwrap();
        assert!(content.contains("\"rclone_path\""));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn install_config_fails_for_missing_source() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("missing.json");
        let err = install_config(dir.path(), &src).await.unwrap_err();
        assert!(format!("{:#}", err).contains("Source file not found"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn install_config_fails_for_invalid_json() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("bad.json");
        std::fs::write(&src, "not-json").unwrap();
        let err = install_config(dir.path(), &src).await.unwrap_err();
        // Message originates from DownloaderConfig::load_from_path
        let msg = format!("{:#}", err);
        assert!(msg.contains("Failed to parse downloader.json") || msg.contains("parse"));
    }
}
