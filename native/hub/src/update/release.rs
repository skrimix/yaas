use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::manifest::{Asset, Identity, Manifest, package_name};
use crate::models::{UpdateChannel, signals::update::AppUpdateRelease};

pub(super) const API: &str = "https://api.github.com/repos/skrimix/yaas";

#[derive(Clone)]
pub(super) struct Candidate {
    pub info: AppUpdateRelease,
    pub asset: Asset,
    /// GitHub asset ID, so a replaced nightly asset is never mixed with older metadata.
    pub asset_id: u64,
}

#[derive(Deserialize)]
struct GithubRelease {
    body: Option<String>,
    html_url: String,
    assets: Vec<GithubAsset>,
}
#[derive(Deserialize)]
struct GithubAsset {
    id: u64,
    name: String,
}

pub(super) fn client() -> Result<Client> {
    Client::builder()
        .user_agent(crate::USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .context("Cannot create update HTTP client")
}

async fn small_response(response: reqwest::Response) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?);
        ensure!(bytes.len() <= 2 * 1024 * 1024, "Release metadata is too large");
    }
    Ok(bytes)
}

fn find_asset<'a>(assets: &'a [GithubAsset], name: &str) -> Result<&'a GithubAsset> {
    assets
        .iter()
        .find(|a| a.name == name)
        .with_context(|| format!("Release asset is missing: {name}"))
}

pub(super) async fn check(
    client: &Client,
    api: &str,
    channel: UpdateChannel,
    installed: &Identity,
) -> Result<Option<Candidate>> {
    let endpoint = match channel {
        UpdateChannel::Stable => "releases/latest",
        UpdateChannel::Nightly => "releases/tags/nightly",
    };
    let response = client
        .get(format!("{api}/{endpoint}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    ensure!(
        response.status() != StatusCode::NOT_FOUND,
        "No published release exists for this channel"
    );
    let bytes = small_response(response.error_for_status()?).await?;
    let release: GithubRelease =
        serde_json::from_slice(&bytes).context("Invalid GitHub release response")?;
    let metadata = find_asset(&release.assets, "release.json")?;
    let response = client
        .get(format!("{api}/releases/assets/{}", metadata.id))
        .header("Accept", "application/octet-stream")
        .send()
        .await?;
    let bytes = small_response(response.error_for_status()?).await?;
    let manifest: Manifest = serde_json::from_slice(&bytes).context("Invalid release.json")?;
    let name = package_name(std::env::consts::OS, std::env::consts::ARCH)
        .context("No update package exists for this platform")?;
    let asset = manifest
        .assets
        .into_iter()
        .find(|a| a.name == name)
        .with_context(|| format!("release.json does not list {name}"))?;
    // The checksum names the update workspace directory, so it must be plain hex.
    ensure!(
        asset.sha256.len() == 64 && asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "release.json lists an invalid SHA-256 for {name}"
    );
    let remote = find_asset(&release.assets, name)?;
    if !manifest.identity.newer_than(installed)? {
        return Ok(None);
    }
    let identity = manifest.identity;
    Ok(Some(Candidate {
        info: AppUpdateRelease {
            version: identity.version,
            build_number: identity.build_number.into(),
            channel,
            commit: identity.commit,
            run_number: identity.run_number,
            run_attempt: identity.run_attempt,
            notes: release.body.unwrap_or_default(),
            release_url: release.html_url,
            package_size: asset.size,
        },
        asset,
        asset_id: remote.id,
    }))
}

/// Downloads the candidate package to `directory/package`, verifying its size and SHA-256.
pub(super) async fn download(
    client: &Client,
    api: &str,
    candidate: &Candidate,
    directory: &Path,
    cancel: &CancellationToken,
    progress: tokio::sync::mpsc::UnboundedSender<u64>,
) -> Result<()> {
    tokio::fs::create_dir_all(directory).await?;
    let partial = directory.join("package.partial");
    let target = directory.join("package");
    let result = cancel
        .run_until_cancelled(async {
            let response = client
                .get(format!("{api}/releases/assets/{}", candidate.asset_id))
                .header("Accept", "application/octet-stream")
                .send()
                .await?;
            ensure!(
                response.status() != StatusCode::NOT_FOUND,
                "This release asset was replaced; check for updates again"
            );
            let response = response.error_for_status()?;
            let mut stream = response.bytes_stream();
            let mut file = tokio::fs::File::create(&partial).await?;
            let mut received = 0;
            let mut hash = Sha256::new();
            let mut last = Instant::now();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                received += chunk.len() as u64;
                ensure!(received <= candidate.asset.size, "Package exceeds expected size");
                hash.update(&chunk);
                file.write_all(&chunk).await?;
                if last.elapsed() >= Duration::from_millis(100) {
                    let _ = progress.send(received);
                    last = Instant::now();
                }
            }
            ensure!(
                received == candidate.asset.size
                    && const_hex::encode(hash.finalize())
                        .eq_ignore_ascii_case(&candidate.asset.sha256),
                "Package size or SHA-256 checksum mismatch"
            );
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&partial, &target).await?;
            let _ = progress.send(received);
            Ok(())
        })
        .await
        .unwrap_or_else(|| Err(anyhow::anyhow!("Download cancelled")));
    if result.is_err() {
        let _ = tokio::fs::remove_file(partial).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;

    fn installed(channel: &str) -> Identity {
        Identity {
            version: "1.0.0".into(),
            build_number: 1,
            channel: channel.into(),
            commit: "a".repeat(40),
            run_number: 1,
            run_attempt: 1,
        }
    }
    fn manifest() -> Manifest {
        let names = ["YAAS-windows-x64.zip", "YAAS-linux-x86_64.AppImage", "YAAS-macos.zip"];
        Manifest {
            identity: Identity { version: "1.1.0".into(), ..installed("stable") },
            assets: names
                .into_iter()
                .map(|name| Asset {
                    name: name.into(),
                    size: 7,
                    sha256: const_hex::encode(Sha256::digest(b"package")),
                })
                .collect(),
        }
    }
    async fn publish(server: &MockServer, manifest: &Manifest, body: &str) {
        let mut assets = vec![json!({"id": 10, "name": "release.json"})];
        assets.extend(
            manifest.assets.iter().enumerate().map(|(i, a)| json!({"id": 20+i, "name": a.name})),
        );
        Mock::given(method("GET")).and(path("/releases/latest")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "body": body, "html_url": "https://github.com/skrimix/yaas/releases/tag/v1.1.0", "assets": assets
        }))).mount(server).await;
        Mock::given(path("/releases/assets/10"))
            .and(header("accept", "application/octet-stream"))
            .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
            .mount(server)
            .await;
    }
    async fn candidate(server: &MockServer) -> Candidate {
        publish(server, &manifest(), "Release notes").await;
        check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
            .await
            .unwrap()
            .unwrap()
    }
    #[tokio::test]
    async fn discovers_and_downloads_pinned_asset() {
        let server = MockServer::start().await;
        let candidate = candidate(&server).await;
        assert_eq!(candidate.info.notes, "Release notes");
        Mock::given(path(format!("/releases/assets/{}", candidate.asset_id)))
            .and(header("accept", "application/octet-stream"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"package"))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        download(
            &client().unwrap(),
            &server.uri(),
            &candidate,
            directory.path(),
            &CancellationToken::new(),
            tx,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(directory.path().join("package")).unwrap(), b"package");
        assert!(!directory.path().join("package.partial").exists());
    }
    #[tokio::test]
    async fn rejects_release_without_platform_package() {
        let server = MockServer::start().await;
        let mut incomplete = manifest();
        incomplete.assets.clear();
        publish(&server, &incomplete, "Notes").await;
        let error =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .err()
                .unwrap();
        assert!(format!("{error:#}").contains("does not list"));
    }
    #[tokio::test]
    async fn rejects_checksums_unusable_as_workspace_names() {
        for sha256 in ["", "../../../../../../..", "/home", &"g".repeat(64), &"a".repeat(63)] {
            let server = MockServer::start().await;
            let mut invalid = manifest();
            for asset in &mut invalid.assets {
                asset.sha256 = sha256.into();
            }
            publish(&server, &invalid, "Notes").await;
            let error = check(
                &client().unwrap(),
                &server.uri(),
                UpdateChannel::Stable,
                &installed("stable"),
            )
            .await
            .err()
            .unwrap();
            assert!(format!("{error:#}").contains("invalid SHA-256"), "{sha256}");
        }
    }
    #[tokio::test]
    async fn reports_up_to_date_and_missing_release() {
        let server = MockServer::start().await;
        let error =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .err()
                .unwrap();
        assert!(error.to_string().contains("No published release"));
        let mut current = manifest();
        current.identity = installed("stable");
        publish(&server, &current, "Notes").await;
        let result =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .unwrap();
        assert!(result.is_none());
    }
    #[tokio::test]
    async fn bad_downloads_never_become_ready() {
        for (status, bytes) in [
            (200, b"bad".as_slice()),
            (200, b"altered".as_slice()),
            (404, b"gone".as_slice()),
            (500, b"failed".as_slice()),
        ] {
            let server = MockServer::start().await;
            let candidate = candidate(&server).await;
            Mock::given(path(format!("/releases/assets/{}", candidate.asset_id)))
                .respond_with(ResponseTemplate::new(status).set_body_bytes(bytes))
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            let (tx, _) = tokio::sync::mpsc::unbounded_channel();
            assert!(
                download(
                    &client().unwrap(),
                    &server.uri(),
                    &candidate,
                    dir.path(),
                    &CancellationToken::new(),
                    tx
                )
                .await
                .is_err()
            );
            assert!(!dir.path().join("package").exists());
            assert!(!dir.path().join("package.partial").exists());
        }
    }
    #[tokio::test]
    async fn cancellation_cleans_partial_files() {
        let server = MockServer::start().await;
        let candidate = candidate(&server).await;
        Mock::given(path(format!("/releases/assets/{}", candidate.asset_id)))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(10))
                    .set_body_bytes(b"package"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let http = client().unwrap();
        let api = server.uri();
        let work = download(&http, &api, &candidate, dir.path(), &cancel, tx);
        let cancel_work = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token.cancel();
        };
        let (result, ()) = tokio::join!(work, cancel_work);
        assert!(result.is_err());
        assert!(!dir.path().join("package.partial").exists());
    }
}
