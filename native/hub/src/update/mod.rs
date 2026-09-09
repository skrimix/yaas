//! Manual application updates and the handoff to the installation helper.

mod install;
mod release;

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result, ensure};
use install::Prepared;
use release::{Candidate, Failure};
use rinf::{DartSignal, RustSignal};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::models::{Settings, UpdateChannel, signals::update::*};

pub(crate) struct UpdateManager {
    sender: mpsc::UnboundedSender<ShutdownCommand>,
}
enum ShutdownCommand {
    Validate(String, oneshot::Sender<Result<()>>),
    Commit(String, oneshot::Sender<Result<()>>),
    Stop(oneshot::Sender<()>),
}

enum Outcome {
    Checked(Option<Candidate>),
    Downloaded,
    Prepared(Prepared),
}
struct Operation {
    task: JoinHandle<Result<Outcome>>,
    cancel: CancellationToken,
}

struct Manager {
    root: PathBuf,
    channel: UpdateChannel,
    phase: AppUpdatePhase,
    received_bytes: u64,
    installation_unavailable_reason: Option<String>,
    error_kind: Option<AppUpdateErrorKind>,
    error: Option<String>,
    candidate: Option<Candidate>,
    operation: Option<Operation>,
    prepared: Option<Prepared>,
    exiting: bool,
    recovery_block: Option<String>,
    progress_tx: mpsc::UnboundedSender<u64>,
}

impl UpdateManager {
    pub(crate) fn start(root: PathBuf, settings: watch::Receiver<Settings>) -> Arc<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let channel = settings.borrow().update_channel;
        let manager = Manager {
            root: root.join("updates"),
            channel,
            phase: AppUpdatePhase::Idle,
            received_bytes: 0,
            installation_unavailable_reason: install::unavailable_reason(),
            error_kind: None,
            error: None,
            candidate: None,
            operation: None,
            prepared: None,
            exiting: false,
            recovery_block: None,
            progress_tx,
        };
        tokio::spawn(manager.run(settings, receiver, progress_rx));
        Arc::new(Self { sender })
    }

    pub(crate) async fn validate_exit(&self, id: String) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(ShutdownCommand::Validate(id, tx)).context("Update manager stopped")?;
        rx.await.context("Update manager stopped")?
    }
    pub(crate) async fn commit_exit(&self, id: String) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(ShutdownCommand::Commit(id, tx)).context("Update manager stopped")?;
        rx.await.context("Update manager stopped")?
    }
    pub(crate) async fn stop(&self) {
        let (tx, rx) = oneshot::channel();
        if self.sender.send(ShutdownCommand::Stop(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

impl Manager {
    fn publish(&self) {
        AppUpdateStateChanged {
            channel: self.channel,
            phase: self.phase,
            release: self.candidate.as_ref().map(|c| c.info.clone()),
            received_bytes: self.received_bytes,
            installation_unavailable_reason: self.installation_unavailable_reason.clone(),
            error_kind: self.error_kind,
            error: self.error.clone(),
        }
        .send_signal_to_dart();
    }
    fn clear_error(&mut self) {
        self.error = None;
        self.error_kind = None;
    }
    fn set_error(&mut self, error: anyhow::Error, default: AppUpdateErrorKind) {
        self.error_kind = Some(error.downcast_ref::<Failure>().map(|f| f.0).unwrap_or(default));
        self.error = Some(format!("{error:#}"));
    }
    fn download_dir(&self) -> PathBuf {
        self.root.join("downloads").join(&self.candidate.as_ref().unwrap().info.candidate_id)
    }
    fn valid_candidate(&self, id: &str) -> bool {
        self.candidate.as_ref().is_some_and(|c| c.info.candidate_id == id)
    }
    fn valid_exit(&self, id: &str) -> Result<()> {
        ensure!(
            self.valid_candidate(id)
                && self.prepared.is_some()
                && self.phase == AppUpdatePhase::AwaitingExit,
            "The prepared update is no longer available"
        );
        Ok(())
    }
    fn idle_phase(&self) -> AppUpdatePhase {
        if self.candidate.is_none() {
            AppUpdatePhase::Idle
        } else if self.download_dir().join("package").is_file() {
            AppUpdatePhase::Ready
        } else {
            AppUpdatePhase::Available
        }
    }
    async fn cancel(&mut self) {
        if self.prepared.as_ref().is_some_and(|p| p.committed) {
            return;
        }
        if let Some(operation) = self.operation.take() {
            operation.cancel.cancel();
            // File preparation runs on the blocking pool; wait before cleaning its workspace.
            if let Ok(Ok(Outcome::Prepared(prepared))) = operation.task.await {
                prepared.cancel().await;
            }
        }
        if let Some(prepared) = self.prepared.take() {
            prepared.cancel().await;
        }
        self.phase = self.idle_phase();
        self.received_bytes = if self.phase == AppUpdatePhase::Ready {
            self.candidate.as_ref().unwrap().info.package_size
        } else {
            0
        };
    }
    async fn reset(&mut self, channel: UpdateChannel) {
        self.cancel().await;
        if self.candidate.is_some() {
            let _ = tokio::fs::remove_dir_all(self.download_dir()).await;
        }
        self.candidate = None;
        self.received_bytes = 0;
        self.phase = AppUpdatePhase::Idle;
        self.channel = channel;
        self.clear_error();
    }

    async fn begin_check(&mut self) -> Result<()> {
        ensure!(
            self.operation.is_none() && self.prepared.is_none(),
            "An update operation is already active"
        );
        self.reset(self.channel).await;
        let channel = self.channel;
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            token
                .run_until_cancelled(async {
                    let client = release::client()?;
                    let candidate = tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        release::check(
                            &client,
                            release::API,
                            channel,
                            &install::installed_identity(),
                        ),
                    )
                    .await??;
                    Ok(Outcome::Checked(candidate))
                })
                .await
                .unwrap_or_else(|| Err(anyhow::anyhow!("Check cancelled")))
        });
        self.operation = Some(Operation { task, cancel });
        self.phase = AppUpdatePhase::Checking;
        Ok(())
    }

    fn require_candidate(&self, id: &str, phase: AppUpdatePhase) -> Result<()> {
        ensure!(
            self.operation.is_none()
                && self.prepared.is_none()
                && self.valid_candidate(id)
                && self.phase == phase,
            "No matching update is ready for this operation"
        );
        Ok(())
    }

    fn begin_download(&mut self, id: &str) -> Result<()> {
        self.require_candidate(id, AppUpdatePhase::Available)?;
        let candidate = self.candidate.as_ref().unwrap().clone();
        let directory = self.download_dir();
        let progress = self.progress_tx.clone();
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            release::download(
                &release::client()?,
                release::API,
                &candidate,
                &directory,
                &token,
                progress,
            )
            .await?;
            Ok(Outcome::Downloaded)
        });
        self.operation = Some(Operation { task, cancel });
        self.clear_error();
        self.received_bytes = 0;
        self.phase = AppUpdatePhase::Downloading;
        Ok(())
    }

    fn begin_install(&mut self, id: &str) -> Result<()> {
        self.require_candidate(id, AppUpdatePhase::Ready)?;
        if let Some(reason) = &self.recovery_block {
            return Err(release::failure(AppUpdateErrorKind::RecoveryRequired, reason));
        }
        if let Some(reason) = install::unavailable_reason() {
            self.installation_unavailable_reason = Some(reason.clone());
            return Err(release::failure(AppUpdateErrorKind::Installation, reason));
        }
        let root = self.root.clone();
        let download = self.download_dir();
        let candidate = self.candidate.as_ref().unwrap().clone();
        let task = tokio::spawn(async move {
            Ok(Outcome::Prepared(install::prepare(root, download, candidate).await?))
        });
        self.operation = Some(Operation { task, cancel: CancellationToken::new() });
        self.clear_error();
        self.phase = AppUpdatePhase::Preparing;
        Ok(())
    }

    fn completed(&mut self, result: Result<Outcome>) {
        self.operation = None;
        if result.is_ok() {
            self.clear_error();
        }
        match result {
            Ok(Outcome::Checked(candidate)) => {
                self.phase = if candidate.is_some() {
                    AppUpdatePhase::Available
                } else {
                    AppUpdatePhase::UpToDate
                };
                self.candidate = candidate;
            }
            Ok(Outcome::Downloaded) => {
                self.phase = AppUpdatePhase::Ready;
                self.received_bytes = self.candidate.as_ref().unwrap().info.package_size;
            }
            Ok(Outcome::Prepared(prepared)) => {
                self.prepared = Some(prepared);
                self.phase = AppUpdatePhase::AwaitingExit;
                AppUpdateExitRequested {
                    candidate_id: self.candidate.as_ref().unwrap().info.candidate_id.clone(),
                }
                .send_signal_to_dart();
            }
            Err(error) => {
                let kind = if self.phase == AppUpdatePhase::Preparing {
                    AppUpdateErrorKind::Installation
                } else {
                    AppUpdateErrorKind::Network
                };
                self.phase = self.idle_phase();
                self.set_error(error, kind);
            }
        }
        self.publish();
    }

    async fn run(
        mut self,
        mut settings: watch::Receiver<Settings>,
        mut shutdown: mpsc::UnboundedReceiver<ShutdownCommand>,
        mut progress: mpsc::UnboundedReceiver<u64>,
    ) {
        let state_rx = GetAppUpdateStateRequest::get_dart_signal_receiver();
        let check_rx = CheckAppUpdateRequest::get_dart_signal_receiver();
        let download_rx = DownloadAppUpdateRequest::get_dart_signal_receiver();
        let install_rx = InstallAppUpdateRequest::get_dart_signal_receiver();
        let cancel_rx = CancelAppUpdateRequest::get_dart_signal_receiver();
        match install::recover_startup(&self.root) {
            Ok(recovery) => {
                if let Some(reason) = recovery.block {
                    self.installation_unavailable_reason = Some(reason.clone());
                    self.recovery_block = Some(reason);
                }
                if let Some(error) = recovery.message {
                    self.set_error(anyhow::anyhow!(error), AppUpdateErrorKind::RecoveryRequired);
                }
            }
            Err(error) => {
                self.recovery_block = Some(format!("{error:#}"));
                self.installation_unavailable_reason = self.recovery_block.clone();
                self.set_error(error, AppUpdateErrorKind::RecoveryRequired);
            }
        }
        self.publish();
        loop {
            tokio::select! {
                command = shutdown.recv() => {
                    match command {
                        Some(ShutdownCommand::Validate(id, reply)) => {
                            let result = self.valid_exit(&id).and_then(|()| {
                                ensure!(self.prepared.as_mut().unwrap().helper.try_wait()?.is_none(), "Update helper stopped before shutdown");
                                Ok(())
                            });
                            self.exiting = result.is_ok();
                            if let Err(error) = &result {
                                self.error = Some(format!("{error:#}"));
                                self.error_kind = Some(AppUpdateErrorKind::Installation);
                                self.cancel().await;
                                self.publish();
                            }
                            let _ = reply.send(result);
                        }
                        Some(ShutdownCommand::Commit(id, reply)) => {
                            let result = match self.valid_exit(&id) {
                                Ok(()) => self.prepared.as_mut().unwrap().commit().await,
                                Err(error) => Err(error),
                            };
                            if let Err(error) = &result
                                && let Some(prepared) = &mut self.prepared
                            {
                                prepared.transaction.error = Some(format!("{error:#}"));
                                let _ = prepared.transaction.save(&prepared.directory);
                            }
                            let _ = reply.send(result);
                        }
                        Some(ShutdownCommand::Stop(reply)) => {
                            self.cancel().await;
                            let _ = reply.send(());
                            break;
                        }
                        None => break,
                    }
                }
                changed = settings.changed(), if !self.exiting => {
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
                request = cancel_rx.recv(), if !self.exiting => {
                    if request.is_none() { break; }
                    self.cancel().await;
                    while progress.try_recv().is_ok() {}
                    self.clear_error();
                    self.publish();
                }
                request = check_rx.recv(), if !self.exiting => {
                    if request.is_none() { break; }
                    if let Err(error) = self.begin_check().await {
                        self.set_error(error, AppUpdateErrorKind::InvalidRequest);
                    }
                    self.publish();
                }
                request = download_rx.recv(), if !self.exiting => {
                    let Some(request) = request else { break; };
                    while progress.try_recv().is_ok() {}
                    if let Err(error) = self.begin_download(&request.message.candidate_id) {
                        self.set_error(error, AppUpdateErrorKind::InvalidRequest);
                    }
                    self.publish();
                }
                request = install_rx.recv(), if !self.exiting => {
                    let Some(request) = request else { break; };
                    if let Err(error) = self.begin_install(&request.message.candidate_id) {
                        self.set_error(error, AppUpdateErrorKind::InvalidRequest);
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
                    self.completed(result.context("Update task stopped").and_then(|r| r));
                }
            }
        }
        self.cancel().await;
    }
}

#[cfg(test)]
mod tests {
    use app_update::{Asset, Identity};

    use super::*;

    fn manager(root: PathBuf) -> Manager {
        let info = AppUpdateRelease {
            candidate_id: "candidate".into(),
            version: "2.0.0".into(),
            build_number: 1,
            channel: UpdateChannel::Stable,
            commit: "a".repeat(40),
            run_number: 2,
            run_attempt: 1,
            notes: String::new(),
            release_url: String::new(),
            package_size: 7,
        };
        let candidate = Candidate {
            info: info.clone(),
            identity: Identity {
                version: info.version.clone(),
                build_number: 1,
                channel: "stable".into(),
                commit: info.commit.clone(),
                run_number: 2,
                run_attempt: 1,
            },
            asset: Asset {
                name: "package".into(),
                os: "linux".into(),
                architectures: vec!["x86_64".into()],
                size: 7,
                sha256: "a".repeat(64),
            },
            asset_id: 1,
        };
        Manager {
            root,
            channel: UpdateChannel::Stable,
            phase: AppUpdatePhase::Available,
            received_bytes: 0,
            installation_unavailable_reason: None,
            error_kind: None,
            error: None,
            candidate: Some(candidate),
            operation: None,
            prepared: None,
            exiting: false,
            recovery_block: None,
            progress_tx: mpsc::unbounded_channel().0,
        }
    }
    #[tokio::test]
    async fn rejects_stale_candidates_and_concurrent_operations() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        assert!(manager.begin_download("stale").is_err());
        assert!(manager.begin_install("candidate").is_err());
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        manager.operation = Some(Operation {
            cancel,
            task: tokio::spawn(async move {
                token.cancelled().await;
                Err(anyhow::anyhow!("cancelled"))
            }),
        });
        assert!(manager.begin_check().await.is_err());
        assert!(manager.begin_download("candidate").is_err());
        manager.cancel().await;
        assert!(manager.operation.is_none());
        assert_eq!(manager.phase, AppUpdatePhase::Available);
    }
    #[tokio::test]
    async fn changing_channel_invalidates_candidate_and_staged_download() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        let download = manager.download_dir();
        std::fs::create_dir_all(&download).unwrap();
        std::fs::write(download.join("package"), "package").unwrap();
        manager.phase = AppUpdatePhase::Ready;
        manager.reset(UpdateChannel::Nightly).await;
        assert_eq!(manager.channel, UpdateChannel::Nightly);
        assert_eq!(manager.phase, AppUpdatePhase::Idle);
        assert!(!manager.valid_candidate("candidate"));
        assert!(!download.exists());
    }
    #[tokio::test]
    async fn cancellation_keeps_a_verified_package_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager(dir.path().into());
        std::fs::create_dir_all(manager.download_dir()).unwrap();
        std::fs::write(manager.download_dir().join("package"), "package").unwrap();
        manager.phase = AppUpdatePhase::Preparing;
        manager.cancel().await;
        assert_eq!(manager.phase, AppUpdatePhase::Ready);
        assert_eq!(manager.received_bytes, 7);
        assert!(manager.valid_exit("candidate").is_err());
    }
}
