//! Manual application updates and the handoff to a generated install script.

mod install;
mod manifest;
mod package;
mod release;
mod script;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Result, ensure};
use release::Candidate;
use rinf::{DartSignal, RustSignal};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::models::{Settings, UpdateChannel, signals::update::*};

/// Output of the last install script; kept when workspaces are cleared.
const LOG_NAME: &str = "update.log";

pub(crate) struct UpdateManager {
    ready: watch::Receiver<Option<ReadyUpdate>>,
}

/// A staged update that [`ReadyUpdate::launch`] installs once the app exits.
#[derive(Clone)]
pub(crate) struct ReadyUpdate {
    root: PathBuf,
    workspace: PathBuf,
    portable: bool,
}

impl ReadyUpdate {
    /// Starts the install script, detached. The app must exit right after.
    pub(crate) fn launch(&self) -> Result<()> {
        install::launch(&self.root, &self.workspace, self.portable)
    }
}

enum Outcome {
    Checked(Option<Box<Candidate>>),
    Downloaded,
}
struct Operation {
    task: JoinHandle<Result<Outcome>>,
    cancel: CancellationToken,
}

struct Manager {
    /// The `updates` directory: candidate workspaces, the install script, and its log.
    root: PathBuf,
    /// Relaunch the updated app with `--portable`.
    portable: bool,
    channel: UpdateChannel,
    phase: AppUpdatePhase,
    received_bytes: u64,
    installation_unavailable_reason: Option<String>,
    error: Option<String>,
    candidate: Option<Candidate>,
    operation: Option<Operation>,
    progress_tx: mpsc::UnboundedSender<u64>,
    ready_tx: watch::Sender<Option<ReadyUpdate>>,
}

impl UpdateManager {
    pub(crate) fn start(
        root: PathBuf,
        portable: bool,
        settings: watch::Receiver<Settings>,
    ) -> Arc<Self> {
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready) = watch::channel(None);
        let channel = settings.borrow().update_channel;
        let manager = Manager {
            root: root.join("updates"),
            portable,
            channel,
            phase: AppUpdatePhase::Idle,
            received_bytes: 0,
            installation_unavailable_reason: install::unavailable_reason(),
            error: None,
            candidate: None,
            operation: None,
            progress_tx,
            ready_tx,
        };
        tokio::spawn(manager.run(settings, progress_rx));
        Arc::new(Self { ready })
    }

    /// The staged update, if one is ready to install.
    pub(crate) fn ready(&self) -> Option<ReadyUpdate> {
        self.ready.borrow().clone()
    }
}

/// Workspace for a package, keyed by its checksum so a repeated check keeps a
/// finished download.
fn workspace(root: &Path, sha256: &str) -> PathBuf {
    root.join(sha256.chars().take(16).collect::<String>().to_ascii_lowercase())
}

/// Removes everything in `root` except the script log and `keep`.
fn clear_workspaces(root: &Path, keep: Option<&Path>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == LOG_NAME || keep == Some(path.as_path()) {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

impl Manager {
    fn publish(&self) {
        self.ready_tx.send_replace((self.phase == AppUpdatePhase::Ready).then(|| ReadyUpdate {
            root: self.root.clone(),
            workspace: self.workspace().unwrap(),
            portable: self.portable,
        }));
        AppUpdateStateChanged {
            channel: self.channel,
            phase: self.phase,
            release: self.candidate.as_ref().map(|c| c.info.clone()),
            received_bytes: self.received_bytes,
            installation_unavailable_reason: self.installation_unavailable_reason.clone(),
            error: self.error.clone(),
        }
        .send_signal_to_dart();
    }
    fn set_error(&mut self, error: anyhow::Error) {
        self.error = Some(format!("{error:#}"));
    }
    fn workspace(&self) -> Option<PathBuf> {
        self.candidate.as_ref().map(|c| workspace(&self.root, &c.asset.sha256))
    }
    /// Sets the phase from the candidate and its workspace when nothing is running.
    fn settle(&mut self) {
        self.phase = match self.workspace() {
            None => AppUpdatePhase::Idle,
            Some(workspace) if install::staged_path(&workspace).exists() => AppUpdatePhase::Ready,
            Some(_) => AppUpdatePhase::Available,
        };
        self.received_bytes = match (&self.candidate, self.phase) {
            (Some(candidate), AppUpdatePhase::Ready) => candidate.info.package_size,
            _ => 0,
        };
    }
    async fn cancel(&mut self) {
        if let Some(operation) = self.operation.take() {
            operation.cancel.cancel();
            // Staging can't be interrupted; wait so the workspace is consistent.
            let _ = operation.task.await;
        }
        self.settle();
    }
    async fn reset(&mut self, channel: UpdateChannel) {
        self.cancel().await;
        self.candidate = None;
        self.channel = channel;
        self.error = None;
        self.settle();
        clear_workspaces(&self.root, None);
    }

    fn begin_check(&mut self) -> Result<()> {
        ensure!(self.operation.is_none(), "An update operation is already active");
        let channel = self.channel;
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            token
                .run_until_cancelled(async {
                    let client = release::client()?;
                    let candidate = tokio::time::timeout(
                        Duration::from_secs(60),
                        release::check(
                            &client,
                            release::API,
                            channel,
                            &install::installed_identity(),
                        ),
                    )
                    .await??;
                    Ok(Outcome::Checked(candidate.map(Box::new)))
                })
                .await
                .unwrap_or_else(|| Err(anyhow::anyhow!("Check cancelled")))
        });
        self.operation = Some(Operation { task, cancel });
        self.error = None;
        self.phase = AppUpdatePhase::Checking;
        Ok(())
    }

    /// Downloads, verifies, and stages the candidate. A failed or cancelled
    /// download removes its workspace.
    fn begin_download(&mut self) -> Result<()> {
        ensure!(
            self.operation.is_none() && self.phase == AppUpdatePhase::Available,
            "No update is available to download"
        );
        let candidate = self.candidate.clone().unwrap();
        let workspace = self.workspace().unwrap();
        let progress = self.progress_tx.clone();
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            let result = async {
                let client = release::client()?;
                release::download(&client, release::API, &candidate, &workspace, &token, progress)
                    .await?;
                ensure!(!token.is_cancelled(), "Download cancelled");
                let directory = workspace.clone();
                tokio::task::spawn_blocking(move || install::stage(&directory)).await?
            }
            .await;
            if result.is_err() {
                let _ = tokio::fs::remove_dir_all(&workspace).await;
            }
            result.map(|()| Outcome::Downloaded)
        });
        self.operation = Some(Operation { task, cancel });
        self.error = None;
        self.received_bytes = 0;
        self.phase = AppUpdatePhase::Downloading;
        Ok(())
    }

    fn completed(&mut self, result: Result<Outcome>) {
        self.operation = None;
        match result {
            Ok(Outcome::Checked(candidate)) => {
                self.error = None;
                self.candidate = candidate.map(|candidate| *candidate);
                clear_workspaces(&self.root, self.workspace().as_deref());
                self.settle();
                if self.candidate.is_none() {
                    self.phase = AppUpdatePhase::UpToDate;
                }
            }
            Ok(Outcome::Downloaded) => {
                self.error = None;
                self.settle();
            }
            Err(error) => {
                self.settle();
                self.set_error(error);
            }
        }
    }

    async fn run(
        mut self,
        mut settings: watch::Receiver<Settings>,
        mut progress: mpsc::UnboundedReceiver<u64>,
    ) {
        let state_rx = GetAppUpdateStateRequest::get_dart_signal_receiver();
        let check_rx = CheckAppUpdateRequest::get_dart_signal_receiver();
        let download_rx = DownloadAppUpdateRequest::get_dart_signal_receiver();
        let cancel_rx = CancelAppUpdateRequest::get_dart_signal_receiver();
        // Downloads don't survive restarts; drop leftovers from earlier sessions.
        clear_workspaces(&self.root, None);
        self.publish();
        loop {
            tokio::select! {
                changed = settings.changed() => {
                    if changed.is_err() { break; }
                    let channel = settings.borrow_and_update().update_channel;
                    if channel != self.channel {
                        self.reset(channel).await;
                        self.publish();
                    }
                }
                request = state_rx.recv() => {
                    if request.is_none() { break; }
                    self.publish();
                }
                request = cancel_rx.recv() => {
                    if request.is_none() { break; }
                    self.cancel().await;
                    while progress.try_recv().is_ok() {}
                    self.error = None;
                    self.publish();
                }
                request = check_rx.recv() => {
                    if request.is_none() { break; }
                    if let Err(error) = self.begin_check() {
                        self.set_error(error);
                    }
                    self.publish();
                }
                request = download_rx.recv() => {
                    if request.is_none() { break; }
                    while progress.try_recv().is_ok() {}
                    if let Err(error) = self.begin_download() {
                        self.set_error(error);
                    }
                    self.publish();
                }
                bytes = progress.recv() => {
                    if let Some(bytes) = bytes && self.phase == AppUpdatePhase::Downloading {
                        self.received_bytes = bytes;
                        self.publish();
                    }
                }
                result = async { (&mut self.operation.as_mut().unwrap().task).await }, if self.operation.is_some() => {
                    self.completed(result.map_err(anyhow::Error::from).and_then(|r| r));
                    self.publish();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{manifest::Asset, *};

    fn candidate(sha256: &str) -> Candidate {
        Candidate {
            info: AppUpdateRelease {
                version: "2.0.0".into(),
                build_number: 1,
                channel: UpdateChannel::Stable,
                commit: "a".repeat(40),
                run_number: 2,
                run_attempt: 1,
                notes: String::new(),
                release_url: String::new(),
                package_size: 7,
            },
            asset: Asset { name: "package".into(), size: 7, sha256: sha256.into() },
            asset_id: 1,
        }
    }

    fn manager(root: PathBuf) -> Manager {
        Manager {
            root,
            portable: false,
            channel: UpdateChannel::Stable,
            phase: AppUpdatePhase::Available,
            received_bytes: 0,
            installation_unavailable_reason: None,
            error: None,
            candidate: Some(candidate(&"a".repeat(64))),
            operation: None,
            progress_tx: mpsc::unbounded_channel().0,
            ready_tx: watch::channel(None).0,
        }
    }

    /// Creates a staged payload for the manager's candidate.
    fn stage(manager: &Manager) -> PathBuf {
        let workspace = manager.workspace().unwrap();
        let staged = install::staged_path(&workspace);
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::write(&staged, "package").unwrap();
        workspace
    }

    #[tokio::test]
    async fn rejects_concurrent_operations() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        manager.operation = Some(Operation {
            cancel,
            task: tokio::spawn(async move {
                token.cancelled().await;
                Err(anyhow::anyhow!("cancelled"))
            }),
        });
        assert!(manager.begin_check().is_err());
        assert!(manager.begin_download().is_err());
        manager.cancel().await;
        assert!(manager.operation.is_none());
        assert_eq!(manager.phase, AppUpdatePhase::Available);
    }

    #[tokio::test]
    async fn repeated_check_keeps_staged_download() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        let workspace = stage(&manager);
        manager.settle();
        assert_eq!(manager.phase, AppUpdatePhase::Ready);

        let same = candidate(&"a".repeat(64));
        manager.completed(Ok(Outcome::Checked(Some(Box::new(same)))));
        assert_eq!(manager.phase, AppUpdatePhase::Ready);
        assert_eq!(manager.received_bytes, 7);
        assert!(workspace.exists());

        let newer = candidate(&"b".repeat(64));
        manager.completed(Ok(Outcome::Checked(Some(Box::new(newer)))));
        assert_eq!(manager.phase, AppUpdatePhase::Available);
        assert!(!workspace.exists());
    }

    #[tokio::test]
    async fn failed_check_keeps_staged_download() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        stage(&manager);
        manager.completed(Err(anyhow::anyhow!("offline")));
        assert_eq!(manager.phase, AppUpdatePhase::Ready);
        assert_eq!(manager.error.as_deref(), Some("offline"));
    }

    #[tokio::test]
    async fn changing_channel_clears_candidate_and_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        let workspace = stage(&manager);
        manager.reset(UpdateChannel::Nightly).await;
        assert_eq!(manager.channel, UpdateChannel::Nightly);
        assert_eq!(manager.phase, AppUpdatePhase::Idle);
        assert!(manager.candidate.is_none());
        assert!(!workspace.exists());
    }

    #[test]
    fn clearing_workspaces_keeps_log_and_current_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["old", "current", "downloads/id"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(root.join(LOG_NAME), "log").unwrap();
        fs::write(root.join("update.sh"), "script").unwrap();
        clear_workspaces(root, Some(&root.join("current")));
        let mut left: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["current", LOG_NAME]);
    }
}
