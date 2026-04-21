use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

#[allow(unused)]
use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use rinf::{DartSignal, RustSignal};
use tokio::{fs, io::AsyncWriteExt};
use tracing::{info, instrument};

use crate::{
    models::signals::casting::{
        CastingDownloadProgress, CastingStatusChanged, DownloadCastingBundleRequest,
        GetCastingStatusRequest,
    },
    utils::remove_child_dir_if_exists,
};

const CASTING_URL: &str =
    "https://github.com/skrimix/yaas/releases/download/files/casting-bundle.zip";

#[cfg(target_os = "windows")]
fn casting_exe_relative() -> &'static str {
    "Casting/Casting.exe"
}

#[cfg(not(target_os = "windows"))]
fn casting_exe_relative() -> &'static str {
    "Casting/Casting"
}

fn casting_exe_path(app_dir: &Path) -> PathBuf {
    app_dir.join(casting_exe_relative())
}

async fn send_status(app_dir: &Path) {
    let exe = casting_exe_path(app_dir);
    let installed = exe.is_file();
    CastingStatusChanged { installed, exe_path: exe.to_str().map(|s| s.to_string()), error: None }
        .send_signal_to_dart();
}

#[instrument(level = "debug", skip_all, err)]
async fn download_casting_bundle(app_dir: &Path) -> Result<()> {
    let url = CASTING_URL;
    let target_zip = app_dir.join("casting-bundle.zip");
    info!(url, path = %target_zip.display(), "Downloading casting bundle");

    let client = {
        let mut builder = reqwest::Client::builder().use_rustls_tls().user_agent(crate::USER_AGENT);
        if let Some(proxy) = crate::utils::get_sys_proxy() {
            builder = builder.proxy(reqwest::Proxy::all(&proxy)?);
        }
        builder.build()?
    };

    let resp = client.get(url).send().await.context("Failed to send HTTP request")?;
    if !resp.status().is_success() {
        return bail_status(resp.status().as_u16());
    }

    let mut file = fs::File::create(&target_zip).await.context("Failed to create bundle file")?;
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    CastingDownloadProgress { received, total }.send_signal_to_dart();
    while let Some(chunk) = stream.next().await.transpose().context("Network error")? {
        received = received.saturating_add(chunk.len() as u64);
        file.write_all(&chunk).await.context("Failed writing bundle contents")?;
        CastingDownloadProgress { received, total }.send_signal_to_dart();
    }
    CastingDownloadProgress { received, total }.send_signal_to_dart();
    file.flush().await.ok();

    remove_child_dir_if_exists(app_dir, "Casting").await;
    unzip_to_dir(&target_zip, app_dir).context("Failed to extract casting bundle")?;
    let _ = fs::remove_file(&target_zip).await;
    Ok(())
}

fn bail_status(code: u16) -> Result<()> {
    Err(anyhow!("Download failed with status {code}"))
}

fn unzip_to_dir(zip_path: &Path, output_dir: &Path) -> Result<()> {
    let file = File::open(zip_path)
        .with_context(|| format!("Failed to open downloaded zip: {}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("Invalid ZIP archive")?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let outpath = output_dir.join(entry.mangled_name());
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)
                .with_context(|| format!("Failed creating directory {}", outpath.display()))?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)
                .with_context(|| format!("Failed creating file {}", outpath.display()))?;
            io::copy(&mut entry, &mut outfile)
                .with_context(|| format!("Failed extracting {}", outpath.display()))?;
        }
    }
    Ok(())
}

pub(crate) struct CastingManager;

impl CastingManager {
    pub(crate) fn start(app_dir: PathBuf) {
        // Status requests
        let status_app_dir = app_dir.clone();
        tokio::spawn(async move {
            let rx = GetCastingStatusRequest::get_dart_signal_receiver();
            while rx.recv().await.is_some() {
                send_status(&status_app_dir).await;
            }
            panic!("GetCastingStatusRequest receiver closed");
        });

        // Download/update requests
        let download_app_dir = app_dir;
        tokio::spawn(async move {
            let rx = DownloadCastingBundleRequest::get_dart_signal_receiver();
            while rx.recv().await.is_some() {
                match download_casting_bundle(&download_app_dir).await {
                    Ok(_) => send_status(&download_app_dir).await,
                    Err(e) => CastingStatusChanged {
                        installed: false,
                        exe_path: None,
                        error: Some(format!("{:#}", e)),
                    }
                    .send_signal_to_dart(),
                }
            }
            panic!("DownloadCastingBundleRequest receiver closed");
        });
    }

    #[cfg(target_os = "windows")]
    pub(crate) async fn start_casting(
        app_dir: &Path,
        adb_path: &Path,
        device_serial: &str,
        wireless: bool,
    ) -> Result<()> {
        use std::path::PathBuf;

        use tokio::process::Command as TokioCommand;

        use crate::models::signals::system::Toast;

        // Resolve Casting.exe path (installed under app data directory)
        let exe_path = casting_exe_path(app_dir);
        if !exe_path.is_file() {
            Toast::send(
                "Casting tool not installed".to_string(),
                "Open Settings and download the Meta Quest Casting tool.".to_string(),
                true,
                None,
            );
            bail!("Casting tool not installed");
        }

        // Ensure caches directory exists: %APPDATA%/odh/casting
        let caches_dir =
            dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("odh").join("casting");
        if let Err(e) = tokio::fs::create_dir_all(&caches_dir).await {
            Toast::send("Failed to prepare caches dir".to_string(), format!("{}", e), true, None);
            bail!("Failed to prepare caches dir");
        }

        // Build command
        let mut cmd = TokioCommand::new(&exe_path);
        cmd.current_dir(exe_path.parent().unwrap_or_else(|| std::path::Path::new(".")));
        cmd.arg("--adb").arg(adb_path);
        cmd.arg("--application-caches-dir").arg(&caches_dir);
        cmd.arg("--exit-on-close");
        cmd.arg("--launch-surface").arg("MQDH");
        let target_json = format!("{{\"id\":\"{}\"}}", device_serial);
        cmd.arg("--target-device").arg(target_json);
        // Features differ based on connection type
        if wireless {
            cmd.arg("--features").args([
                "input_forwarding",
                "input_forwarding_gaze_click",
                "input_forwarding_text_input_forwarding",
                "image_stabilization",
                "update_device_fov_via_openxr_api",
                "wireless_casting",
            ]);
        } else {
            cmd.arg("--features").args([
                "input_forwarding",
                "input_forwarding_gaze_click",
                "input_forwarding_text_input_forwarding",
                "image_stabilization",
                "update_device_fov_via_openxr_api",
            ]);
        }

        match cmd.spawn() {
            Ok(_child) => Ok(()),
            Err(e) => {
                Toast::send("Failed to launch Casting".to_string(), format!("{:#}", e), true, None);
                bail!("Failed to launch Casting");
            }
        }
    }
}
