//! Native casting: runs an XRSP session with the headset and serves a paced live
//! Matroska stream over local HTTP.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use magic_cast::SessionEvent;
use rinf::{DartSignal, RustSignal};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, instrument, warn};

use crate::{
    adb::AdbService,
    models::signals::{
        casting::{
            GetNativeCastingStateRequest, NativeCastingState, NativeCastingStateChanged,
            NativeCastingStats, StartNativeCastingRequest, StopNativeCastingRequest,
        },
        system::Toast,
    },
};

const DEFAULT_RESOLUTION: (u32, u32) = (1800, 1920);
const QUEST_3_RESOLUTION: (u32, u32) = (2064, 2208);
const XRSP_PORT: u16 = 4445;
/// Stop the session if no HTTP player connects within this time to avoid unbounded buffering.
const PLAYER_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

struct ActiveCasting {
    id: u64,
    session: magic_cast::CastingSession,
    player_connected: Arc<AtomicBool>,
}

pub(crate) struct NativeCastingManager {
    adb_service: Arc<AdbService>,
    session: Arc<RwLock<Option<ActiveCasting>>>,
    next_session_id: AtomicU64,
    last_state: std::sync::RwLock<NativeCastingStateChanged>,
}

impl NativeCastingManager {
    pub(crate) fn start(adb_service: Arc<AdbService>) -> Arc<Self> {
        let manager = Arc::new(Self {
            adb_service,
            session: Arc::new(RwLock::new(None)),
            next_session_id: AtomicU64::new(1),
            last_state: std::sync::RwLock::new(NativeCastingStateChanged {
                state: NativeCastingState::Idle,
                url: None,
                error: None,
            }),
        });

        {
            let manager = manager.clone();
            tokio::spawn(async move {
                let rx = StartNativeCastingRequest::get_dart_signal_receiver();
                while let Some(request) = rx.recv().await {
                    manager.start_session(request.message.audio, request.message.fps).await;
                }
                panic!("StartNativeCastingRequest receiver closed");
            });
        }

        {
            let manager = manager.clone();
            tokio::spawn(async move {
                let rx = StopNativeCastingRequest::get_dart_signal_receiver();
                while rx.recv().await.is_some() {
                    manager.stop_session().await;
                }
                panic!("StopNativeCastingRequest receiver closed");
            });
        }

        {
            let manager = manager.clone();
            tokio::spawn(async move {
                let rx = GetNativeCastingStateRequest::get_dart_signal_receiver();
                while rx.recv().await.is_some() {
                    manager.resend_state();
                }
                panic!("GetNativeCastingStateRequest receiver closed");
            });
        }

        manager
    }

    pub(crate) async fn shutdown(&self) {
        if let Some(active) = self.session.write().await.take() {
            info!("Stopping native casting session for shutdown");
            tokio::task::spawn_blocking(move || active.session.stop()).await.ok();
        }
    }

    fn emit_state(&self, state: NativeCastingState, url: Option<String>, error: Option<String>) {
        let signal = NativeCastingStateChanged { state, url, error };
        *self.last_state.write().expect("state lock poisoned") = signal.clone();
        signal.send_signal_to_dart();
    }

    fn resend_state(&self) {
        self.last_state.read().expect("state lock poisoned").clone().send_signal_to_dart();
    }

    #[instrument(level = "debug", skip_all)]
    async fn start_session(self: &Arc<Self>, audio: bool, fps: u32) {
        let mut slot = self.session.write().await;
        if slot.is_some() {
            Toast::send(
                "Casting already running".to_string(),
                "Stop the current casting session first.".to_string(),
                true,
                None,
            );
            return;
        }

        let device = match self.adb_service.current_device().await {
            Ok(device) => device,
            Err(e) => {
                Toast::send("Cannot start casting".to_string(), format!("{:#}", e), true, None);
                return;
            }
        };
        let serial = device.true_serial.clone();
        let (width, height) = if device.product.eq_ignore_ascii_case("eureka") {
            QUEST_3_RESOLUTION
        } else {
            DEFAULT_RESOLUTION
        };
        let adb = match self.adb_service.resolved_adb_path().await {
            Ok(path) => path,
            Err(e) => {
                Toast::send(
                    "Cannot start casting".to_string(),
                    format!("ADB binary not found: {:#}", e),
                    true,
                    None,
                );
                return;
            }
        };

        let config = magic_cast::SessionConfig {
            serial: Some(serial),
            adb,
            fps,
            width,
            height,
            audio,
            xrsp_port: XRSP_PORT,
            http_port: 0,
        };

        let started =
            tokio::task::spawn_blocking(move || magic_cast::CastingSession::start(config)).await;
        let (session, events) = match started {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                let error = format!("{e}");
                Toast::send("Failed to start casting".to_string(), error.clone(), true, None);
                self.emit_state(NativeCastingState::Idle, None, Some(error));
                return;
            }
            Err(e) => {
                let error = format!("casting startup task failed: {e}");
                Toast::send("Failed to start casting".to_string(), error.clone(), true, None);
                self.emit_state(NativeCastingState::Idle, None, Some(error));
                return;
            }
        };

        info!(url = session.url(), "Started native casting session");
        let url = session.url().to_string();
        let id = self.next_session_id.fetch_add(1, Ordering::SeqCst);
        let player_connected = Arc::new(AtomicBool::new(false));
        *slot = Some(ActiveCasting { id, session, player_connected: player_connected.clone() });
        self.emit_state(NativeCastingState::Starting, Some(url), None);
        drop(slot);

        self.spawn_event_bridge(events, id, player_connected.clone());
        self.spawn_player_timeout(id);
    }

    #[instrument(level = "debug", skip_all)]
    async fn stop_session(&self) {
        let active = self.session.write().await.take();
        if let Some(active) = active {
            tokio::task::spawn_blocking(move || active.session.stop()).await.ok();
        }
        self.emit_state(NativeCastingState::Idle, None, None);
    }

    fn spawn_event_bridge(
        self: &Arc<Self>,
        events: std::sync::mpsc::Receiver<SessionEvent>,
        session_id: u64,
        player_connected: Arc<AtomicBool>,
    ) {
        let (tx, mut rx) = mpsc::unbounded_channel::<SessionEvent>();
        tokio::task::spawn_blocking(move || {
            while let Ok(event) = events.recv() {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });

        let manager = self.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                manager.handle_session_event(event, session_id, &player_connected).await;
            }
        });
    }

    async fn current_url_for(&self, session_id: u64) -> Option<String> {
        self.session
            .read()
            .await
            .as_ref()
            .filter(|active| active.id == session_id)
            .map(|active| active.session.url().to_string())
    }

    async fn handle_session_event(
        &self,
        event: SessionEvent,
        session_id: u64,
        player_connected: &AtomicBool,
    ) {
        match event {
            SessionEvent::DeviceConnected => {
                info!("Native casting: device stream connected");
            }
            SessionEvent::Recovering => {
                info!("Native casting: recovering device stream");
                if let Some(url) = self.current_url_for(session_id).await {
                    self.emit_state(NativeCastingState::Reconnecting, Some(url), None);
                }
            }
            SessionEvent::PlayerConnected => {
                info!("Native casting: player connected");
                player_connected.store(true, Ordering::SeqCst);
            }
            SessionEvent::PlaybackStarted => {
                info!("Native casting: playback started");
                if let Some(url) = self.current_url_for(session_id).await {
                    self.emit_state(NativeCastingState::Streaming, Some(url), None);
                }
            }
            SessionEvent::PlayerDisconnected => {
                info!("Native casting: player disconnected; stopping session");
                self.stop_session_if_current(session_id).await;
            }
            SessionEvent::Stats(stats) => {
                NativeCastingStats {
                    fps: stats.fps,
                    buffered_frames: stats.buffered_frames,
                    buffer_age_ms: stats.buffer_age_ms,
                    latency_ms: stats.latency_ms,
                }
                .send_signal_to_dart();
            }
            SessionEvent::Ended(error) => {
                let (active, superseded) = {
                    let mut slot = self.session.write().await;
                    match slot.as_ref() {
                        // Ignore the event if a newer session has already replaced this one.
                        Some(active) if active.id != session_id => (None, true),
                        Some(_) => (slot.take(), false),
                        None => (None, false),
                    }
                };
                if superseded {
                    debug!("Native casting: ignoring Ended from a superseded session");
                    return;
                }
                match (active, error) {
                    (Some(active), error) => {
                        // Join the session threads before reporting the final state.
                        tokio::task::spawn_blocking(move || active.session.stop()).await.ok();
                        if let Some(error) = error {
                            warn!("Native casting session failed: {error}");
                            Toast::send("Casting failed".to_string(), error.clone(), true, None);
                            self.emit_state(NativeCastingState::Idle, None, Some(error));
                        } else {
                            self.emit_state(NativeCastingState::Idle, None, None);
                        }
                    }
                    (None, Some(error)) => {
                        self.emit_state(NativeCastingState::Idle, None, Some(error));
                    }
                    (None, None) => {
                        debug!("Native casting: session already stopped");
                    }
                }
            }
        }
    }

    async fn stop_session_if_current(&self, session_id: u64) {
        let active = {
            let mut slot = self.session.write().await;
            match slot.as_ref() {
                Some(active) if active.id == session_id => slot.take(),
                _ => None,
            }
        };
        if let Some(active) = active {
            tokio::task::spawn_blocking(move || active.session.stop()).await.ok();
            self.emit_state(NativeCastingState::Idle, None, None);
        }
    }

    fn spawn_player_timeout(self: &Arc<Self>, session_id: u64) {
        let manager = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PLAYER_CONNECT_TIMEOUT).await;
            let mut slot = manager.session.write().await;
            let Some(active) = slot.as_ref() else {
                return;
            };
            // A newer session may have replaced the one this timeout belongs to.
            if active.id != session_id || active.player_connected.load(Ordering::SeqCst) {
                return;
            }
            warn!("No player connected within timeout; stopping native casting session");
            let active = slot.take().expect("session slot checked above");
            tokio::task::spawn_blocking(move || active.session.stop()).await.ok();
            let error = "Player did not connect to the stream".to_string();
            Toast::send("Casting stopped".to_string(), error.clone(), true, None);
            manager.emit_state(NativeCastingState::Idle, None, Some(error));
        });
    }
}
