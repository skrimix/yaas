use std::{fs::File, io, path::{Path, PathBuf}};

use anyhow::{Context, Result, anyhow};
use rinf::{DartSignal, RustSignal};
use tokio::{fs, io::AsyncWriteExt};
use futures::StreamExt;
use tracing::{info, instrument};

use crate::models::signals::casting::{
    CastingStatusChanged, CastingDownloadProgress, DownloadCastingBundleRequest,
    GetCastingStatusRequest,
};

const DEFAULT_CASTING_URL: &str =
    "https://github.com/skrimix/yaas/releases/download/files/casting-bundle.zip";

#[cfg(target_os = "windows")]
fn casting_exe_relative() -> &'static str { "Casting/Casting.exe" }

#[cfg(not(target_os = "windows"))]
fn casting_exe_relative() -> &'static str { "Casting/Casting" }

fn casting_exe_path() -> PathBuf { std::env::current_dir().unwrap_or_default().join(casting_exe_relative()) }

async fn send_status() {
    let exe = casting_exe_path();
    let installed = exe.is_file();
    CastingStatusChanged {
        installed,
        exe_path: exe.to_str().map(|s| s.to_string()),
        error: None,
    }
    .send_signal_to_dart();
}

#[instrument(skip_all, err)]
async fn download_casting_bundle(url: &str) -> Result<()> {
    let target_zip = std::env::current_dir()?.join("casting-bundle.zip");
    info!(url, path = %target_zip.display(), "Downloading casting bundle");

    let client = {
        let mut builder = reqwest::Client::builder().use_rustls_tls().user_agent(crate::USER_AGENT);
        if let Some(proxy) = crate::utils::get_sys_proxy() {
            builder = builder.proxy(reqwest::Proxy::all(&proxy)?);
        }
        builder.build()?
    };

    let resp = client
        .get(url)
        .send()
        .await
        .context("Failed to send HTTP request")?;
    if !resp.status().is_success() {
        return bail_status(resp.status().as_u16());
    }

    let mut file = fs::File::create(&target_zip)
        .await
        .context("Failed to create bundle file")?;
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    // Emit initial 0%
    CastingDownloadProgress { received, total }.send_signal_to_dart();
    while let Some(chunk) = stream.next().await.transpose().context("Network error")? {
        received = received.saturating_add(chunk.len() as u64);
        file.write_all(&chunk).await.context("Failed writing bundle contents")?;
        CastingDownloadProgress { received, total }.send_signal_to_dart();
    }
    // Final event after stream ends
    CastingDownloadProgress { received, total }.send_signal_to_dart();
    file.flush().await.ok();

    // Remove existing Casting directory
    crate::utils::remove_child_dir_if_exists(&std::env::current_dir()?, "Casting").await?;
    // Extract
    unzip_to_current_dir(&target_zip).context("Failed to extract casting bundle")?;
    // Clean zip
    let _ = fs::remove_file(&target_zip).await;
    Ok(())
}

fn bail_status(code: u16) -> Result<()> { Err(anyhow!("Download failed with status {code}")) }

fn unzip_to_current_dir(zip_path: &Path) -> Result<()> {
    let file = File::open(zip_path).with_context(|| format!(
        "Failed to open downloaded zip: {}",
        zip_path.display()
    ))?;
    let mut zip = zip::ZipArchive::new(file).context("Invalid ZIP archive")?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let outpath = cwd.join(entry.mangled_name());
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath).with_context(|| format!(
                "Failed creating directory {}",
                outpath.display()
            ))?;
        } else {
            if let Some(parent) = outpath.parent() { std::fs::create_dir_all(parent)?; }
            let mut outfile = File::create(&outpath).with_context(|| format!(
                "Failed creating file {}",
                outpath.display()
            ))?;
            io::copy(&mut entry, &mut outfile)
                .with_context(|| format!("Failed extracting {}", outpath.display()))?;
        }
    }
    Ok(())
}

pub struct CastingManager;

impl CastingManager {
    pub fn start() {
        // Status requests
        tokio::spawn(async move {
            let rx = GetCastingStatusRequest::get_dart_signal_receiver();
            while rx.recv().await.is_some() {
                send_status().await;
            }
            panic!("GetCastingStatusRequest receiver closed");
        });

        // Download/update requests
        tokio::spawn(async move {
            let rx = DownloadCastingBundleRequest::get_dart_signal_receiver();
            while let Some(req) = rx.recv().await {
                let url = req.message.url.unwrap_or_else(|| DEFAULT_CASTING_URL.to_string());
                match download_casting_bundle(&url).await {
                    Ok(_) => send_status().await,
                    Err(e) => CastingStatusChanged { installed: false, exe_path: None, error: Some(format!("{:#}", e)) }.send_signal_to_dart(),
                }
            }
            panic!("DownloadCastingBundleRequest receiver closed");
        });
    }
}
