use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use app_update::{Asset, Identity, Manifest};
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::models::{
    UpdateChannel,
    signals::update::{AppUpdateErrorKind, AppUpdateRelease},
};

pub(super) const API: &str = "https://api.github.com/repos/skrimix/yaas";

#[derive(Debug)]
pub(super) struct Failure(pub AppUpdateErrorKind, pub String);
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1.fmt(f)
    }
}
impl std::error::Error for Failure {}
pub(super) fn failure(kind: AppUpdateErrorKind, message: impl Into<String>) -> anyhow::Error {
    Failure(kind, message.into()).into()
}

#[derive(Clone)]
pub(super) struct Candidate {
    pub info: AppUpdateRelease,
    pub identity: Identity,
    pub asset: Asset,
    pub asset_id: u64,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    html_url: String,
    assets: Vec<GithubAsset>,
}
#[derive(Deserialize)]
struct GithubAsset {
    id: u64,
    name: String,
    size: u64,
    state: String,
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
    if response.status() == StatusCode::NOT_FOUND {
        return Err(failure(
            AppUpdateErrorKind::NoRelease,
            "No published release exists for this channel",
        ));
    }
    let bytes = small_response(response.error_for_status()?).await?;
    let release: GithubRelease = serde_json::from_slice(&bytes)
        .map_err(|e| failure(AppUpdateErrorKind::InvalidMetadata, e.to_string()))?;
    if release.draft
        || release.body.as_deref().unwrap_or_default().starts_with("Preparation incomplete.")
    {
        return Err(failure(
            AppUpdateErrorKind::IncompleteRelease,
            "Release publication is not complete; check again later",
        ));
    }
    if release.prerelease != (channel == UpdateChannel::Nightly) {
        return Err(failure(AppUpdateErrorKind::InvalidMetadata, "Unexpected release channel"));
    }
    let metadata = unique_asset(&release.assets, "release.json")?;
    let response = client
        .get(format!("{api}/releases/assets/{}", metadata.id))
        .header("Accept", "application/octet-stream")
        .send()
        .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(failure(
            AppUpdateErrorKind::IncompleteRelease,
            "Release metadata was replaced; check again later",
        ));
    }
    let bytes = small_response(response.error_for_status()?).await?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| failure(AppUpdateErrorKind::InvalidMetadata, e.to_string()))?;
    manifest
        .validate(channel.as_str(), &release.tag_name)
        .map_err(|e| failure(AppUpdateErrorKind::InvalidMetadata, e.to_string()))?;
    let asset = manifest
        .asset(std::env::consts::OS, std::env::consts::ARCH)
        .map_err(|e| failure(AppUpdateErrorKind::NoPackage, e.to_string()))?
        .clone();
    let remote = unique_asset(&release.assets, &asset.name)?;
    if remote.size != asset.size {
        return Err(failure(
            AppUpdateErrorKind::IncompleteRelease,
            "Release package and manifest differ; check again later",
        ));
    }
    if !manifest.identity.newer_than(installed)? {
        return Ok(None);
    }
    let identity = manifest.identity;
    Ok(Some(Candidate {
        info: AppUpdateRelease {
            candidate_id: uuid::Uuid::new_v4().to_string(),
            version: identity.version.clone(),
            build_number: identity.build_number.into(),
            channel,
            commit: identity.commit.clone(),
            run_number: identity.run_number,
            run_attempt: identity.run_attempt,
            notes: release.body.unwrap_or_default(),
            release_url: release.html_url,
            package_size: asset.size,
        },
        identity,
        asset,
        asset_id: remote.id,
    }))
}

fn unique_asset<'a>(assets: &'a [GithubAsset], name: &str) -> Result<&'a GithubAsset> {
    let found = assets.iter().filter(|a| a.name == name).collect::<Vec<_>>();
    if found.len() != 1 || found[0].state != "uploaded" {
        return Err(failure(
            AppUpdateErrorKind::IncompleteRelease,
            format!("Release asset is missing or incomplete: {name}"),
        ));
    }
    Ok(found[0])
}

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
            if response.status() == StatusCode::NOT_FOUND {
                return Err(failure(
                    AppUpdateErrorKind::IncompleteRelease,
                    "This release asset was replaced; check for updates again",
                ));
            }
            let response = response.error_for_status()?;
            let mut stream = response.bytes_stream();
            let mut file = tokio::fs::File::create(&partial).await?;
            let mut received = 0;
            let mut hash = Sha256::new();
            let mut last = Instant::now();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                received += chunk.len() as u64;
                if received > candidate.asset.size {
                    return Err(failure(
                        AppUpdateErrorKind::Integrity,
                        "Package exceeds expected size",
                    ));
                }
                hash.update(&chunk);
                file.write_all(&chunk).await?;
                if last.elapsed() >= Duration::from_millis(100) {
                    let _ = progress.send(received);
                    last = Instant::now();
                }
            }
            if received != candidate.asset.size
                || !const_hex::encode(hash.finalize()).eq_ignore_ascii_case(&candidate.asset.sha256)
            {
                return Err(failure(
                    AppUpdateErrorKind::Integrity,
                    "Package size or SHA-256 checksum mismatch",
                ));
            }
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
        let names = [
            ("YAAS-windows-x64.zip", "windows", vec!["x86_64"]),
            ("YAAS-linux-x86_64.AppImage", "linux", vec!["x86_64"]),
            ("YAAS-macos.zip", "macos", vec!["x86_64", "aarch64"]),
        ];
        Manifest {
            schema_version: 1,
            identity: Identity { version: "1.1.0".into(), ..installed("stable") },
            tag: "v1.1.0".into(),
            assets: names
                .into_iter()
                .map(|(name, os, architectures)| Asset {
                    name: name.into(),
                    os: os.into(),
                    architectures: architectures.into_iter().map(String::from).collect(),
                    size: 7,
                    sha256: const_hex::encode(Sha256::digest(b"package")),
                })
                .collect(),
        }
    }
    async fn publish(server: &MockServer, manifest: &Manifest, body: &str) {
        let mut assets =
            vec![json!({"id": 10, "name": "release.json", "size": 100, "state": "uploaded"})];
        assets.extend(
            manifest.assets.iter().enumerate().map(
                |(i, a)| json!({"id": 20+i, "name": a.name, "size": a.size, "state":"uploaded"}),
            ),
        );
        Mock::given(method("GET")).and(path("/releases/latest")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tag_name": manifest.tag, "draft": false, "prerelease": false, "body": body, "html_url": "https://github.com/skrimix/yaas/releases/tag/v1.1.0", "assets": assets
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
    async fn rejects_incomplete_and_invalid_publication() {
        let server = MockServer::start().await;
        publish(&server, &manifest(), "Preparation incomplete. Wait").await;
        let error =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .err()
                .unwrap();
        assert!(matches!(
            error.downcast_ref::<Failure>().unwrap().0,
            AppUpdateErrorKind::IncompleteRelease
        ));
        server.reset().await;
        let mut malformed = manifest();
        malformed.schema_version = 2;
        publish(&server, &malformed, "Notes").await;
        let error =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .err()
                .unwrap();
        assert!(matches!(
            error.downcast_ref::<Failure>().unwrap().0,
            AppUpdateErrorKind::InvalidMetadata
        ));
    }
    #[tokio::test]
    async fn distinguishes_no_release_from_network_failure() {
        let server = MockServer::start().await;
        let error =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .err()
                .unwrap();
        assert!(matches!(
            error.downcast_ref::<Failure>().unwrap().0,
            AppUpdateErrorKind::NoRelease
        ));
        Mock::given(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        let error =
            check(&client().unwrap(), &server.uri(), UpdateChannel::Stable, &installed("stable"))
                .await
                .err()
                .unwrap();
        assert!(error.downcast_ref::<reqwest::Error>().is_some());
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
