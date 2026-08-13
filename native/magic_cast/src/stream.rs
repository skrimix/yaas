//! Paced playout and HTTP serving for a live casting session.
//!
//! Buffers incoming XRSP media, paces it, and serves it as live Matroska to one HTTP client.
//! The player disconnecting stops the session.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use oxideav_mkv::avc::annexb_to_avcc;
use tracing::{debug, info, warn};

use crate::cadence::{AdaptivePacer, PacingAction, PacingConfig};
use crate::matroska::{self, AUDIO_TRACK, Tracks, VIDEO_TRACK};
use crate::session::{
    AUDIO_CHANNELS, AUDIO_SAMPLE_RATE, AudioPacket, CastConfig, LiveControl, LiveSession,
    LiveStats, SessionEvent, StreamEvent, VideoPacket, effective_fixed_fps,
    h264_dimensions_from_avcc,
};

const AUDIO_TIMESTAMP_JITTER_TOLERANCE_MS: i64 = 20;
// Keep a small amount of PCM ready for the player without delaying the video timeline.
const AUDIO_PLAYOUT_LEAD_MS: i64 = 50;
const AUDIO_SILENCE_CHUNK_MS: i64 = 20;
const HTTP_PATH: &str = "/cast.mkv";
const MAX_HTTP_REQUEST_BYTES: usize = 8 * 1024;
/// Request a device sync frame before hard catch-up (reference tooling default: off).
const RESYNC_ON_LAG: bool = false;

#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// Device serial passed to adb (`-s`). `None` uses adb's default device selection.
    pub serial: Option<String>,
    /// Path to the adb binary.
    pub adb: PathBuf,
    /// Requested capture frame rate (`0` = device default, about 30).
    pub fps: u32,
    /// Requested capture width.
    pub width: u32,
    /// Requested capture height.
    pub height: u32,
    /// Enable device audio, muxed into the Matroska stream.
    pub audio: bool,
    /// Device-facing XRSP port, exposed to the headset with `adb reverse`.
    pub xrsp_port: u16,
    /// HTTP port for the Matroska stream; `0` lets the OS assign a free port.
    pub http_port: u16,
}

/// A running native casting session.
///
/// The session serves one non-seekable Matroska stream at [`CastingSession::url`]. Only one HTTP
/// client is supported; the client disconnecting stops the session. When the session ends for any
/// reason, the device-side casting state is torn down best-effort (panel streaming is disabled
/// and the `adb reverse` forward is removed) before the terminal [`SessionEvent::Ended`] is sent.
pub struct CastingSession {
    url: String,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl CastingSession {
    /// Binds the HTTP listener, starts the XRSP server (with ADB bring-up) and the paced playout
    /// thread, and returns the session handle plus its event stream.
    pub fn start(
        config: SessionConfig,
    ) -> crate::session::Result<(Self, mpsc::Receiver<SessionEvent>)> {
        let listener = TcpListener::bind(("127.0.0.1", config.http_port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let url = format!("http://127.0.0.1:{port}{HTTP_PATH}");
        info!(%url, "serving paced Matroska stream");

        let stop = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let (stream_tx, stream_rx) = mpsc::channel();
        let (session_tx, session_rx) = mpsc::channel();

        let cast_config = CastConfig {
            serial: config.serial.clone(),
            fps: config.fps,
            width: config.width,
            height: config.height,
            audio: config.audio,
            adaptively_skip_frames: false,
            port: config.xrsp_port,
            adb: config.adb.clone(),
        };
        let session = LiveSession::new(cast_config, stop.clone(), stream_tx, session_tx.clone());
        let control = session.control();

        let server_thread = {
            let session_tx = session_tx.clone();
            let server_stop = stop.clone();
            let ended = ended.clone();
            thread::Builder::new()
                .name("xrsp-server".to_string())
                .spawn(move || {
                    let result = session.run_server();
                    if let Err(error) = &result {
                        warn!("casting session failed: {error}");
                    }
                    // Make sure the sink thread exits as well.
                    server_stop.store(true, Ordering::SeqCst);
                    send_ended_once(
                        &ended,
                        &session_tx,
                        result.err().map(|error| error.to_string()),
                    );
                })?
        };

        let sink_thread = {
            let stop = stop.clone();
            let playout_config = PlayoutConfig::new(&config);
            thread::Builder::new()
                .name("xrsp-sink".to_string())
                .spawn(move || {
                    let sink_stop = stop.clone();
                    let sink_tx = session_tx.clone();
                    if let Err(error) = run_http_sink(
                        stream_rx,
                        stop,
                        control,
                        listener,
                        playout_config,
                        session_tx,
                    ) {
                        warn!("HTTP sink failed: {error}");
                        sink_stop.store(true, Ordering::SeqCst);
                        // Report sink failures as session failures instead of a clean stop.
                        send_ended_once(&ended, &sink_tx, Some(error.to_string()));
                    }
                })?
        };

        Ok((
            Self {
                url,
                stop,
                threads: vec![server_thread, sink_thread],
            },
            session_rx,
        ))
    }

    /// URL of the live Matroska HTTP stream.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Stops the session and joins its threads.
    pub fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

/// Sends the terminal [`SessionEvent::Ended`] exactly once, whichever thread finishes first.
fn send_ended_once(ended: &AtomicBool, tx: &mpsc::Sender<SessionEvent>, error: Option<String>) {
    if !ended.swap(true, Ordering::SeqCst) {
        let _ = tx.send(SessionEvent::Ended(error));
    }
}

/// Playback/muxing settings derived from the session config.
struct PlayoutConfig {
    audio: bool,
    width: u32,
    height: u32,
    resync_on_lag: bool,
    pacing: PacingConfig,
}

impl PlayoutConfig {
    fn new(config: &SessionConfig) -> Self {
        Self {
            audio: config.audio,
            width: config.width,
            height: config.height,
            resync_on_lag: RESYNC_ON_LAG,
            pacing: PacingConfig {
                initial_fps: f64::from(effective_fixed_fps(config.fps)),
                rate_window_seconds: 0.25,
                min_buffer_frames: 2,
                max_buffer_frames: 10,
                stall_threshold_ms: 100.0,
                occupancy_gain: 0.10,
                max_rate_adjustment: 0.20,
                low_buffer_gain_multiplier: 2.0,
                transient_recovery_seconds: 4.0,
                stable_grace_seconds: 5.0,
                target_decay_interval_seconds: 3.0,
                hard_latency_ms: 300.0,
            },
        }
    }
}

fn run_http_sink(
    rx: mpsc::Receiver<StreamEvent>,
    stop: Arc<AtomicBool>,
    control: LiveControl,
    listener: TcpListener,
    config: PlayoutConfig,
    session_tx: mpsc::Sender<SessionEvent>,
) -> crate::session::Result<()> {
    let Some(mut stream) = accept_player(&listener, &stop)? else {
        return Ok(());
    };
    let _ = session_tx.send(SessionEvent::PlayerConnected);

    let mut playout = PacedPlayout::new(&config)?;
    playout.set_event_sender(session_tx.clone());
    while !stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if !playout.started && playout.ready_to_start() {
            if let Err(error) =
                write_video_header(&config, playout.video_config_record.as_deref(), &mut stream)
            {
                info!("player stream closed before playback start: {error}");
                let _ = session_tx.send(SessionEvent::PlayerDisconnected);
                stop.store(true, Ordering::SeqCst);
                break;
            }
            playout.start(now);
            let _ = session_tx.send(SessionEvent::PlaybackStarted);
            info!(
                audio = config.audio,
                audio_lead_ms = if config.audio {
                    AUDIO_PLAYOUT_LEAD_MS
                } else {
                    0
                },
                buffer_frames = config.pacing.min_buffer_frames,
                max_buffer_frames = config.pacing.max_buffer_frames,
                "started paced Matroska playback"
            );
        }
        if playout.started {
            let ready = playout.take_ready(now, &control);
            if let Err(error) = write_live_packets(&mut stream, ready) {
                info!("player stream closed: {error}");
                let _ = session_tx.send(SessionEvent::PlayerDisconnected);
                stop.store(true, Ordering::SeqCst);
                break;
            }
        }

        match rx.recv_timeout(playout.wait_timeout(Instant::now())) {
            Ok(StreamEvent::Video(video_packet)) => playout.push_video(video_packet),
            Ok(StreamEvent::Audio(audio_packet)) => playout.push_audio(audio_packet),
            Ok(StreamEvent::Discontinuity) => playout.handle_discontinuity(Instant::now()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    playout.log_stats();
    Ok(())
}

fn accept_player(
    listener: &TcpListener,
    stop: &AtomicBool,
) -> crate::session::Result<Option<TcpStream>> {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nodelay(true)?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                match read_http_request(&mut stream) {
                    Ok(HttpRequest::Stream) => {
                        stream.write_all(http_ok_response())?;
                        stream.flush()?;
                        info!("player connected to {HTTP_PATH}");
                        return Ok(Some(stream));
                    }
                    Ok(HttpRequest::NotFound) => {
                        write_http_error(&mut stream, "404 Not Found")?;
                    }
                    Ok(HttpRequest::MethodNotAllowed) => {
                        write_http_error(&mut stream, "405 Method Not Allowed")?;
                    }
                    Err(error) => {
                        let _ = write_http_error(&mut stream, "400 Bad Request");
                        debug!("rejected local HTTP request: {error}");
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HttpRequest {
    Stream,
    NotFound,
    MethodNotAllowed,
}

fn read_http_request(stream: &mut TcpStream) -> crate::session::Result<HttpRequest> {
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err("HTTP client closed before sending a complete request".into());
        }
        if request.len() + count > MAX_HTTP_REQUEST_BYTES {
            return Err("HTTP request is too large".into());
        }
        request.extend_from_slice(&chunk[..count]);
    }

    parse_http_request(&request)
}

fn parse_http_request(request: &[u8]) -> crate::session::Result<HttpRequest> {
    let request = std::str::from_utf8(request)?;
    let mut fields = request
        .lines()
        .next()
        .ok_or("HTTP request line is missing")?
        .split_whitespace();
    let method = fields.next().ok_or("HTTP method is missing")?;
    let path = fields.next().ok_or("HTTP path is missing")?;
    let version = fields.next().ok_or("HTTP version is missing")?;
    if fields.next().is_some() || !version.starts_with("HTTP/1.") {
        return Err("invalid HTTP request line".into());
    }
    if method != "GET" {
        return Ok(HttpRequest::MethodNotAllowed);
    }
    if path != HTTP_PATH {
        return Ok(HttpRequest::NotFound);
    }
    Ok(HttpRequest::Stream)
}

fn http_ok_response() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: video/x-matroska\r\nCache-Control: no-store\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
}

fn write_http_error(stream: &mut TcpStream, status: &str) -> crate::session::Result<()> {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn write_video_header(
    config: &PlayoutConfig,
    config_record: Option<&[u8]>,
    output: &mut impl Write,
) -> crate::session::Result<()> {
    let config_record = config_record.ok_or("H.264 configuration was not available")?;
    let (pixel_width, pixel_height) =
        h264_dimensions_from_avcc(config_record).unwrap_or((config.width, config.height));
    output.write_all(&matroska::live_header(&Tracks {
        video_config_record: config_record,
        pixel_width,
        pixel_height,
        display_width: config.width,
        display_height: config.height,
        video_default_duration_ns: None,
        audio: config.audio,
        audio_sample_rate: AUDIO_SAMPLE_RATE,
        audio_channels: AUDIO_CHANNELS,
    }))?;
    output.flush()?;
    Ok(())
}

struct BufferedVideoFrame {
    received_ns: u64,
    layer_id: u32,
    gop_index: u32,
    is_keyframe: bool,
    data: Vec<u8>,
}

struct PendingResync {
    requested_at: Instant,
    previous_gop_index: u32,
}

struct PacedPlayout {
    pacer: AdaptivePacer,
    audio_enabled: bool,
    resync_enabled: bool,
    video_config_record: Option<Vec<u8>>,
    anchor: Option<Instant>,
    playback_started_at: Option<Instant>,
    playback_origin_ns: Option<u64>,
    last_pts_ms: Option<i64>,
    video_buffer: VecDeque<BufferedVideoFrame>,
    audio_buffer: VecDeque<AudioPacket>,
    audio_source_origin_ms: Option<i64>,
    last_audio_source_timestamp_ms: Option<i64>,
    audio_playback_origin_ms: i64,
    next_audio_pts_ms: i64,
    started: bool,
    awaiting_keyframe: bool,
    last_canonical_index: Option<u32>,
    timeline_index: u64,
    input_video_frames: usize,
    admitted_video_frames: usize,
    status_window_start: Option<Instant>,
    status_window_input_frames: usize,
    latency_window_sum_ms: f64,
    latency_window_count: usize,
    max_buffer_age: Duration,
    frame_latency_ms: Vec<f64>,
    event_tx: Option<mpsc::Sender<SessionEvent>>,
    rebuffer_started: Option<Instant>,
    rebuffer_time: Duration,
    pending_resync: Option<PendingResync>,
    last_resync_request: Option<Instant>,
    resync_requests: usize,
    resync_successes: usize,
    resync_timeouts: usize,
    muxed_audio_packets: usize,
    generated_silence_ms: i64,
    dropped_audio_packets: usize,
}

impl PacedPlayout {
    fn new(config: &PlayoutConfig) -> crate::session::Result<Self> {
        Ok(Self {
            pacer: AdaptivePacer::new(config.pacing.clone())?,
            audio_enabled: config.audio,
            resync_enabled: config.resync_on_lag,
            video_config_record: None,
            anchor: None,
            playback_started_at: None,
            playback_origin_ns: None,
            last_pts_ms: None,
            video_buffer: VecDeque::new(),
            audio_buffer: VecDeque::new(),
            audio_source_origin_ms: None,
            last_audio_source_timestamp_ms: None,
            audio_playback_origin_ms: 0,
            next_audio_pts_ms: 0,
            started: false,
            awaiting_keyframe: true,
            last_canonical_index: None,
            timeline_index: 0,
            input_video_frames: 0,
            admitted_video_frames: 0,
            status_window_start: None,
            status_window_input_frames: 0,
            latency_window_sum_ms: 0.0,
            latency_window_count: 0,
            max_buffer_age: Duration::ZERO,
            frame_latency_ms: Vec::new(),
            event_tx: None,
            rebuffer_started: None,
            rebuffer_time: Duration::ZERO,
            pending_resync: None,
            last_resync_request: None,
            resync_requests: 0,
            resync_successes: 0,
            resync_timeouts: 0,
            muxed_audio_packets: 0,
            generated_silence_ms: 0,
            dropped_audio_packets: 0,
        })
    }

    /// Sets the channel for [`SessionEvent::Stats`] and playback-resumed notifications.
    fn set_event_sender(&mut self, tx: mpsc::Sender<SessionEvent>) {
        self.event_tx = Some(tx);
    }

    fn emit(&self, event: SessionEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    fn handle_discontinuity(&mut self, now: Instant) {
        self.video_buffer.clear();
        self.audio_buffer.clear();
        self.audio_source_origin_ms = None;
        self.last_audio_source_timestamp_ms = None;
        self.last_canonical_index = None;
        self.awaiting_keyframe = true;
        self.pending_resync = None;
        self.last_resync_request = None;
        self.pacer.force_refill();
        if self.started && self.rebuffer_started.is_none() {
            self.rebuffer_started = Some(now);
        }
        info!("device stream restarted; cleared playback buffers");
    }

    fn push_video(&mut self, packet: VideoPacket) {
        let repack = annexb_to_avcc(&packet.data);
        if self.video_config_record.is_none() && !repack.config_record.is_empty() {
            self.video_config_record = Some(repack.config_record);
        }
        if repack.packetized.is_empty() {
            return;
        }
        self.input_video_frames += 1;
        if self.awaiting_keyframe {
            if !packet.is_keyframe {
                return;
            }
            self.awaiting_keyframe = false;
            if self.anchor.is_none() {
                self.anchor = Some(packet.received_at);
            }
        }
        let step = self
            .last_canonical_index
            .map(|previous| packet.canonical_index.wrapping_sub(previous))
            .filter(|step| (1..=10_000).contains(step))
            .unwrap_or(1);
        if self.last_canonical_index.is_some() {
            self.timeline_index = self.timeline_index.saturating_add(u64::from(step));
        }
        self.last_canonical_index = Some(packet.canonical_index);

        let received_ns = self.instant_ns(packet.received_at);
        self.pacer.observe_frame(received_ns, self.timeline_index);
        self.video_buffer.push_back(BufferedVideoFrame {
            received_ns,
            layer_id: packet.layer_id,
            gop_index: packet.gop_index,
            is_keyframe: packet.is_keyframe,
            data: repack.packetized,
        });
        self.admitted_video_frames += 1;
        self.log_live_status(packet.received_at);
    }

    fn push_audio(&mut self, packet: AudioPacket) {
        if self.audio_enabled {
            self.audio_buffer.push_back(packet);
        }
    }

    fn ready_to_start(&self) -> bool {
        self.video_config_record.is_some()
            && self.anchor.is_some()
            && self.video_buffer.len() >= self.pacer.target_buffer_frames() as usize
    }

    fn start(&mut self, now: Instant) {
        self.started = true;
        self.playback_started_at = Some(now);
        let now_ns = self.instant_ns(now);
        self.playback_origin_ns = Some(now_ns);
        self.pacer.resume_if_ready(now_ns, self.video_buffer.len());
    }

    fn take_ready(&mut self, now: Instant, control: &LiveControl) -> Vec<TimedPacket> {
        if !self.started {
            return Vec::new();
        }
        let now_ns = self.instant_ns(now);
        self.update_max_buffer_age(now_ns);
        self.resolve_pending_resync(now, now_ns);

        let mut ready = Vec::new();
        let catch_up = self.pacer.catch_up_frames(
            now_ns,
            self.video_buffer.len(),
            self.video_buffer.front().map(|frame| frame.received_ns),
        );
        if catch_up > 0 && self.pending_resync.is_none() {
            if self.should_request_resync(now) {
                let frame = self
                    .video_buffer
                    .front()
                    .expect("catch-up requires a buffered frame");
                match control.request_resync_frame(frame.layer_id) {
                    Ok(()) => {
                        self.pending_resync = Some(PendingResync {
                            requested_at: now,
                            previous_gop_index: frame.gop_index,
                        });
                        self.last_resync_request = Some(now);
                        self.resync_requests += 1;
                    }
                    Err(error) => warn!("ResyncFrame request failed: {error}"),
                }
            }
            if self.pending_resync.is_none() {
                self.emit_catch_up_frames(now_ns, catch_up, &mut ready);
            }
        }

        if self.pacer.is_filling() && self.pacer.resume_if_ready(now_ns, self.video_buffer.len()) {
            if let Some(started) = self.rebuffer_started.take() {
                self.rebuffer_time += now.saturating_duration_since(started);
            }
            self.emit(SessionEvent::PlaybackStarted);
        }

        match self.pacer.poll(now_ns, self.video_buffer.len()) {
            PacingAction::Present { presentation_ns } => {
                if let Some(frame) = self.video_buffer.pop_front() {
                    ready.push(self.timed_packet(frame, presentation_ns));
                }
            }
            PacingAction::Rebuffer {
                target_buffer_frames,
                refill_buffer_frames,
            } => {
                info!(
                    target_buffer_frames,
                    refill_buffer_frames, "playback buffer underflow; refilling"
                );
                self.rebuffer_started = Some(now);
            }
            PacingAction::Wait => {}
        }
        self.take_ready_audio(now, &mut ready);
        ready.sort_by_key(TimedPacket::pts_ms);
        ready
    }

    fn take_ready_audio(&mut self, now: Instant, ready: &mut Vec<TimedPacket>) {
        if !self.audio_enabled {
            return;
        }
        let Some(playback_started_at) = self.playback_started_at else {
            return;
        };
        let horizon_ms = signed_duration_ms(now, playback_started_at)
            .max(0)
            .saturating_add(AUDIO_PLAYOUT_LEAD_MS);

        while let Some(packet) = self.audio_buffer.front() {
            let first_audio_packet = self.audio_source_origin_ms.is_none();
            if first_audio_packet {
                self.audio_source_origin_ms = Some(packet.timestamp_ms);
                self.audio_playback_origin_ms =
                    signed_duration_ms(packet.received_at, playback_started_at)
                        .max(self.next_audio_pts_ms);
            }
            let target_pts_ms = self.audio_playback_origin_ms + packet.timestamp_ms
                - self.audio_source_origin_ms.expect("audio origin was set");
            if target_pts_ms > horizon_ms + AUDIO_TIMESTAMP_JITTER_TOLERANCE_MS {
                break;
            }

            let packet = self.audio_buffer.pop_front().expect("audio packet exists");
            if self
                .last_audio_source_timestamp_ms
                .is_some_and(|timestamp_ms| packet.timestamp_ms <= timestamp_ms)
            {
                self.dropped_audio_packets += 1;
                continue;
            }
            self.last_audio_source_timestamp_ms = Some(packet.timestamp_ms);
            if target_pts_ms > self.next_audio_pts_ms
                && (first_audio_packet
                    || target_pts_ms > self.next_audio_pts_ms + AUDIO_TIMESTAMP_JITTER_TOLERANCE_MS)
            {
                self.emit_silence_until(target_pts_ms, ready);
            }

            // Silence already sent to the player cannot be replaced. Append late audio at the
            // current frontier so short sounds are preserved instead of trimming their beginning.
            let data = packet.data.to_vec();
            let duration_ms = audio_duration_ms(data.len());
            if duration_ms == 0 {
                self.dropped_audio_packets += 1;
                continue;
            }
            ready.push(TimedPacket::Audio {
                pts_ms: self.next_audio_pts_ms,
                data,
            });
            self.next_audio_pts_ms += duration_ms;
            self.muxed_audio_packets += 1;
        }

        self.emit_silence_until(horizon_ms, ready);
    }

    fn emit_silence_until(&mut self, target_pts_ms: i64, ready: &mut Vec<TimedPacket>) {
        while self.next_audio_pts_ms < target_pts_ms {
            let duration_ms = (target_pts_ms - self.next_audio_pts_ms).min(AUDIO_SILENCE_CHUNK_MS);
            ready.push(TimedPacket::Audio {
                pts_ms: self.next_audio_pts_ms,
                data: vec![0; audio_bytes_for_ms(duration_ms)],
            });
            self.next_audio_pts_ms += duration_ms;
            self.generated_silence_ms += duration_ms;
        }
    }

    fn resolve_pending_resync(&mut self, now: Instant, now_ns: u64) {
        let Some(pending) = self.pending_resync.as_ref() else {
            return;
        };
        if let Some(position) = self
            .video_buffer
            .iter()
            .position(|frame| frame.is_keyframe && frame.gop_index != pending.previous_gop_index)
        {
            for _ in 0..position {
                self.video_buffer.pop_front();
            }
            self.pending_resync = None;
            self.resync_successes += 1;
            self.pacer.force_refill();
            self.rebuffer_started = Some(now);
            info!("ResyncFrame request produced a keyframe; refilling playback buffer");
        } else if now.saturating_duration_since(pending.requested_at) >= Duration::from_millis(250)
        {
            self.pending_resync = None;
            self.resync_timeouts += 1;
            let catch_up = self.pacer.catch_up_frames(
                now_ns,
                self.video_buffer.len(),
                self.video_buffer.front().map(|frame| frame.received_ns),
            );
            if catch_up > 0 {
                info!("ResyncFrame request timed out; catching up without a new keyframe");
            }
        }
    }

    fn should_request_resync(&self, now: Instant) -> bool {
        let interval_elapsed = match self.last_resync_request {
            Some(last) => now.saturating_duration_since(last) >= Duration::from_secs(2),
            None => true,
        };
        self.resync_enabled && interval_elapsed
    }

    fn emit_catch_up_frames(
        &mut self,
        presentation_ns: u64,
        count: usize,
        output: &mut Vec<TimedPacket>,
    ) {
        for _ in 0..count {
            if let Some(frame) = self.video_buffer.pop_front() {
                output.push(self.timed_packet(frame, presentation_ns));
            }
        }
        self.pacer.record_catch_up(count);
    }

    fn timed_packet(&mut self, frame: BufferedVideoFrame, presentation_ns: u64) -> TimedPacket {
        let origin_ns = self
            .playback_origin_ns
            .expect("paced playback origin was set at start");
        let candidate = presentation_ns.saturating_sub(origin_ns) / 1_000_000;
        let pts_ms = monotonic_pts_ms(candidate as i64, &mut self.last_pts_ms);
        let latency_ms = presentation_ns.saturating_sub(frame.received_ns) as f64 / 1_000_000.0;
        self.frame_latency_ms.push(latency_ms);
        self.latency_window_sum_ms += latency_ms;
        self.latency_window_count += 1;
        TimedPacket::Video {
            pts_ms,
            is_keyframe: frame.is_keyframe,
            data: frame.data,
        }
    }

    fn instant_ns(&self, instant: Instant) -> u64 {
        instant
            .saturating_duration_since(self.anchor.unwrap_or(instant))
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64
    }

    fn update_max_buffer_age(&mut self, now_ns: u64) {
        if let Some(oldest) = self.video_buffer.front() {
            self.max_buffer_age = self.max_buffer_age.max(Duration::from_nanos(
                now_ns.saturating_sub(oldest.received_ns),
            ));
        }
    }

    fn log_live_status(&mut self, now: Instant) {
        let start = *self.status_window_start.get_or_insert(now);
        let elapsed = now.saturating_duration_since(start);
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let now_ns = self.instant_ns(now);
        let buffer_age_ms = self
            .video_buffer
            .front()
            .map(|oldest| now_ns.saturating_sub(oldest.received_ns) as f64 / 1_000_000.0)
            .unwrap_or(0.0);
        let input_frames = self.input_video_frames;
        let window_frames = input_frames.saturating_sub(self.status_window_input_frames);
        let fps = window_frames as f64 / elapsed.as_secs_f64();
        debug!(
            source_fps = format_args!("{:.2}", self.pacer.estimated_fps()),
            input_fps = format_args!("{fps:.2}"),
            buffered_frames = self.video_buffer.len(),
            buffer_age_ms = format_args!("{buffer_age_ms:.1}"),
            target_buffer_frames = self.pacer.target_buffer_frames(),
            "paced playback status"
        );
        if self.started {
            let latency_ms = if self.latency_window_count > 0 {
                self.latency_window_sum_ms / self.latency_window_count as f64
            } else {
                0.0
            };
            self.emit(SessionEvent::Stats(LiveStats {
                fps,
                buffered_frames: self.video_buffer.len() as u32,
                buffer_age_ms,
                latency_ms,
            }));
        }
        self.status_window_start = Some(now);
        self.status_window_input_frames = input_frames;
        self.latency_window_sum_ms = 0.0;
        self.latency_window_count = 0;
    }

    fn wait_timeout(&self, now: Instant) -> Duration {
        let poll = Duration::from_millis(20);
        if !self.started {
            return poll;
        }
        let now_ns = self.instant_ns(now);
        let deadline_wait = self
            .pacer
            .next_deadline_ns()
            .map(|deadline| Duration::from_nanos(deadline.saturating_sub(now_ns)))
            .unwrap_or(poll);
        let resync_wait = self
            .pending_resync
            .as_ref()
            .map(|pending| {
                Duration::from_millis(250)
                    .saturating_sub(now.saturating_duration_since(pending.requested_at))
            })
            .unwrap_or(poll);
        deadline_wait.min(resync_wait).min(poll)
    }

    fn log_stats(&self) {
        let stats = self.pacer.stats();
        let buffer_age_ns = self
            .video_buffer
            .front()
            .map(|oldest| {
                self.instant_ns(Instant::now())
                    .saturating_sub(oldest.received_ns)
            })
            .unwrap_or(0);
        info!(
            input_video_frames = self.input_video_frames,
            admitted_video_frames = self.admitted_video_frames,
            estimated_source_fps = format_args!("{:.2}", self.pacer.estimated_fps()),
            target_buffer_frames = self.pacer.target_buffer_frames(),
            buffered_frames = self.video_buffer.len(),
            buffer_age_ms = buffer_age_ns as f64 / 1_000_000.0,
            max_buffer_age_ms = self.max_buffer_age.as_secs_f64() * 1000.0,
            underflow_events = stats.underflow_events,
            rebuffer_events = stats.rebuffer_events,
            transient_recovery_events = stats.transient_recovery_events,
            target_promotions = stats.target_promotions,
            rebuffer_time_ms = self.rebuffer_time.as_secs_f64() * 1000.0,
            catch_up_events = stats.catch_up_events,
            catch_up_frames = stats.catch_up_frames,
            resync_requests = self.resync_requests,
            resync_successes = self.resync_successes,
            resync_timeouts = self.resync_timeouts,
            muxed_audio_packets = self.muxed_audio_packets,
            generated_silence_ms = self.generated_silence_ms,
            dropped_audio_packets = self.dropped_audio_packets,
            "paced playback stats"
        );
        log_metric_stats("frame_latency", &self.frame_latency_ms);
    }
}

fn signed_duration_ms(later: Instant, earlier: Instant) -> i64 {
    if later >= earlier {
        later.duration_since(earlier).as_millis() as i64
    } else {
        -(earlier.duration_since(later).as_millis() as i64)
    }
}

fn log_metric_stats(name: &str, values: &[f64]) {
    if values.is_empty() {
        return;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    info!(
        mean_ms = format_args!("{mean:.1}"),
        p95_ms = format_args!("{:.1}", percentile(&sorted, 0.95)),
        p99_ms = format_args!("{:.1}", percentile(&sorted, 0.99)),
        max_ms = format_args!("{:.1}", sorted.last().unwrap()),
        "{name} stats"
    );
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    let rank = quantile * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = rank - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
    }
}

fn write_live_packets(
    output: &mut impl Write,
    packets: impl IntoIterator<Item = TimedPacket>,
) -> crate::session::Result<()> {
    for packet in packets {
        let cluster = match packet {
            TimedPacket::Video {
                pts_ms,
                is_keyframe,
                data,
            } => matroska::packet_cluster(VIDEO_TRACK, pts_ms, is_keyframe, &data),
            TimedPacket::Audio { pts_ms, data } => {
                matroska::packet_cluster(AUDIO_TRACK, pts_ms, true, &data)
            }
        };
        output.write_all(&cluster)?;
    }
    output.flush()?;
    Ok(())
}

#[derive(Debug)]
enum TimedPacket {
    Video {
        pts_ms: i64,
        is_keyframe: bool,
        data: Vec<u8>,
    },
    Audio {
        pts_ms: i64,
        data: Vec<u8>,
    },
}

impl TimedPacket {
    fn pts_ms(&self) -> i64 {
        match self {
            Self::Video { pts_ms, .. } | Self::Audio { pts_ms, .. } => *pts_ms,
        }
    }
}

fn monotonic_pts_ms(candidate: i64, last_pts_ms: &mut Option<i64>) -> i64 {
    let pts_ms = match *last_pts_ms {
        Some(last) if candidate <= last => last + 1,
        _ => candidate,
    };
    *last_pts_ms = Some(pts_ms);
    pts_ms
}

fn audio_bytes_for_ms(duration_ms: i64) -> usize {
    let sample_frames = duration_ms.max(0) as usize * AUDIO_SAMPLE_RATE as usize / 1000;
    sample_frames * audio_bytes_per_sample_frame()
}

fn audio_duration_ms(byte_len: usize) -> i64 {
    let sample_frames = byte_len / audio_bytes_per_sample_frame();
    (sample_frames as i64 * 1000) / i64::from(AUDIO_SAMPLE_RATE)
}

fn audio_bytes_per_sample_frame() -> usize {
    usize::from(AUDIO_CHANNELS) * 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn local_http_request_round_trip_writes_stream_headers() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
        let address = listener.local_addr().expect("listener address");
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).expect("connect test client");
            stream
                .write_all(b"GET /cast.mkv HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .expect("write request");
            let mut response = Vec::new();
            stream.read_to_end(&mut response).expect("read response");
            response
        });

        let (mut stream, _) = listener.accept().expect("accept test client");
        assert_eq!(
            read_http_request(&mut stream).expect("parse request"),
            HttpRequest::Stream
        );
        stream
            .write_all(http_ok_response())
            .expect("write response");
        drop(stream);

        let response = client.join().expect("join test client");
        let response = String::from_utf8(response).expect("UTF-8 response");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: video/x-matroska\r\n"));
        assert!(response.contains("Cache-Control: no-store\r\n"));
        assert!(response.ends_with("\r\n\r\n"));
    }

    #[test]
    fn http_request_rejects_wrong_method_and_path() {
        assert_eq!(
            parse_http_request(b"HEAD /cast.mkv HTTP/1.1\r\n\r\n").expect("parse method"),
            HttpRequest::MethodNotAllowed
        );
        assert_eq!(
            parse_http_request(b"GET /video.h264 HTTP/1.1\r\n\r\n").expect("parse path"),
            HttpRequest::NotFound
        );
        assert!(parse_http_request(b"GET /cast.mkv\r\n\r\n").is_err());
    }

    #[test]
    fn paced_start_waits_for_config_keyframe_and_target_buffer() {
        let mut config = test_playout_config();
        config.pacing.min_buffer_frames = 2;
        let now = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");

        playout.push_video(test_video_packet(now, false, 7));
        playout.push_video(test_video_packet(now, true, 5));
        assert!(!playout.ready_to_start());

        let mut second = test_video_packet(now + Duration::from_millis(17), false, 1);
        second.canonical_index = 1;
        playout.push_video(second);
        assert!(playout.ready_to_start());
    }

    #[test]
    fn paced_discontinuity_refills_from_keyframe_and_accepts_restarted_audio() {
        let config = test_playout_config();
        let now = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.push_video(test_video_packet(now, false, 7));
        playout.push_video(test_video_packet(now, true, 5));
        playout.start(now);
        playout.last_pts_ms = Some(120);
        playout.next_audio_pts_ms = 500;
        playout.audio_source_origin_ms = Some(90_000);
        playout.last_audio_source_timestamp_ms = Some(90_080);
        playout.push_audio(test_audio_packet(90_160, now, 20));

        playout.handle_discontinuity(now + Duration::from_millis(100));

        assert!(playout.video_buffer.is_empty());
        assert!(playout.audio_buffer.is_empty());
        assert_eq!(playout.last_pts_ms, Some(120));
        assert_eq!(playout.next_audio_pts_ms, 500);
        assert!(playout.audio_source_origin_ms.is_none());
        assert!(playout.last_audio_source_timestamp_ms.is_none());
        assert!(playout.awaiting_keyframe);
        assert!(playout.pacer.is_filling());

        playout.push_video(test_video_packet(
            now + Duration::from_millis(200),
            false,
            1,
        ));
        assert!(playout.video_buffer.is_empty());
        playout.push_video(test_video_packet(now + Duration::from_millis(220), true, 5));
        assert_eq!(playout.video_buffer.len(), 1);

        playout.push_audio(test_audio_packet(5, now + Duration::from_millis(600), 20));
        let mut ready = Vec::new();
        playout.take_ready_audio(now + Duration::from_millis(700), &mut ready);
        assert_eq!(playout.muxed_audio_packets, 1);
        assert!(ready
            .iter()
            .any(|packet| matches!(packet, TimedPacket::Audio { data, .. } if data.iter().any(|byte| *byte != 0))));
    }

    #[test]
    fn paced_hard_catch_up_emits_dependency_frames_with_monotonic_pts() {
        let mut config = test_playout_config();
        config.audio = false;
        config.pacing.min_buffer_frames = 2;
        config.pacing.hard_latency_ms = 10.0;
        let now = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.push_video(test_video_packet(now, false, 7));
        playout.push_video(test_video_packet(now, true, 5));
        for index in 1..=5 {
            let mut packet = test_video_packet(now + Duration::from_millis(index), false, 1);
            packet.canonical_index = index as u32;
            playout.push_video(packet);
        }
        playout.start(now + Duration::from_millis(6));

        let (tx, _rx) = mpsc::channel();
        let (session_tx, _session_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let session = LiveSession::new(test_cast_config(), stop, tx, session_tx);
        let output = playout.take_ready(now + Duration::from_millis(20), &session.control());
        let pts = output
            .iter()
            .filter_map(|packet| match packet {
                TimedPacket::Video { pts_ms, .. } => Some(*pts_ms),
                TimedPacket::Audio { .. } => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(playout.pacer.stats().catch_up_frames, 4);
        assert_eq!(output.len(), 5);
        assert!(pts.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(&playout.frame_latency_ms[..4], &[20.0, 19.0, 18.0, 17.0]);
        assert_eq!(playout.video_buffer.len(), 1);
    }

    #[test]
    fn paced_resync_discards_only_through_a_new_idr() {
        let mut config = test_playout_config();
        config.resync_on_lag = true;
        let now = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.pending_resync = Some(PendingResync {
            requested_at: now,
            previous_gop_index: 2,
        });
        playout.video_buffer.push_back(BufferedVideoFrame {
            received_ns: 0,
            layer_id: 0,
            gop_index: 3,
            is_keyframe: false,
            data: vec![1],
        });

        playout.resolve_pending_resync(now + Duration::from_millis(10), 10_000_000);
        assert!(playout.pending_resync.is_some());
        assert_eq!(playout.video_buffer.len(), 1);

        playout.video_buffer.push_back(BufferedVideoFrame {
            received_ns: 20_000_000,
            layer_id: 0,
            gop_index: 3,
            is_keyframe: true,
            data: vec![5],
        });
        playout.resolve_pending_resync(now + Duration::from_millis(20), 20_000_000);

        assert!(playout.pending_resync.is_none());
        assert_eq!(playout.resync_successes, 1);
        assert_eq!(playout.video_buffer.len(), 1);
        assert!(
            playout
                .video_buffer
                .front()
                .is_some_and(|frame| frame.is_keyframe)
        );
    }

    #[test]
    fn paced_resync_times_out_and_is_rate_limited() {
        let mut config = test_playout_config();
        config.resync_on_lag = true;
        let now = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.pending_resync = Some(PendingResync {
            requested_at: now,
            previous_gop_index: 2,
        });
        playout.last_resync_request = Some(now);

        playout.resolve_pending_resync(now + Duration::from_millis(249), 249_000_000);
        assert!(playout.pending_resync.is_some());
        playout.resolve_pending_resync(now + Duration::from_millis(250), 250_000_000);
        assert!(playout.pending_resync.is_none());
        assert_eq!(playout.resync_timeouts, 1);
        assert!(!playout.should_request_resync(now + Duration::from_millis(1_999)));
        assert!(playout.should_request_resync(now + Duration::from_secs(2)));
    }

    #[test]
    fn paced_header_declares_audio_only_when_enabled() {
        let mut config = test_playout_config();
        let avcc = annexb_to_avcc(&test_video_packet(Instant::now(), false, 7).data).config_record;
        let mut output = Vec::new();
        write_video_header(&config, Some(&avcc), &mut output).expect("write audio header");
        assert!(contains_bytes(&output, b"A_PCM/INT/LIT"));

        config.audio = false;
        output.clear();
        write_video_header(&config, Some(&avcc), &mut output).expect("write video header");
        assert!(!contains_bytes(&output, b"A_PCM/INT/LIT"));
    }

    #[test]
    fn paced_audio_emits_silence_without_device_packets() {
        let config = test_playout_config();
        let start = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.start(start);
        let mut ready = Vec::new();

        playout.take_ready_audio(start + Duration::from_millis(100), &mut ready);

        assert_eq!(playout.generated_silence_ms, 150);
        assert_eq!(audio_packet_duration_ms(&ready), 150);
        assert!(ready.iter().all(|packet| match packet {
            TimedPacket::Audio { data, .. } => data.iter().all(|byte| *byte == 0),
            TimedPacket::Video { .. } => false,
        }));
    }

    #[test]
    fn paced_audio_lead_does_not_offset_video_timestamps() {
        let config = test_playout_config();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.playback_origin_ns = Some(1_000_000);
        let packet = playout.timed_packet(test_buffered_video_frame(), 6_000_000);
        assert!(matches!(packet, TimedPacket::Video { pts_ms: 5, .. }));

        let mut video_only = test_playout_config();
        video_only.audio = false;
        let mut playout = PacedPlayout::new(&video_only).expect("create video-only playout");
        playout.playback_origin_ns = Some(1_000_000);
        let packet = playout.timed_packet(test_buffered_video_frame(), 6_000_000);
        assert!(matches!(packet, TimedPacket::Video { pts_ms: 5, .. }));
    }

    #[test]
    fn paced_audio_preserves_short_sound_after_silence() {
        let config = test_playout_config();
        let start = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.start(start);
        let mut ready = Vec::new();
        playout.take_ready_audio(start + Duration::from_millis(980), &mut ready);
        ready.clear();
        playout.push_audio(test_audio_packet(
            10_000,
            start + Duration::from_millis(1_000),
            10,
        ));

        playout.take_ready_audio(start + Duration::from_millis(1_000), &mut ready);

        assert_eq!(playout.generated_silence_ms, 1_040);
        assert!(ready.iter().any(|packet| matches!(
            packet,
            TimedPacket::Audio { pts_ms: 1_030, data }
                if data.len() == audio_bytes_for_ms(10)
                    && data.iter().all(|byte| *byte == 1)
        )));
    }

    #[test]
    fn paced_audio_fills_timestamp_gaps_and_ignores_small_jitter() {
        let config = test_playout_config();
        let start = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.start(start);
        let mut ready = Vec::new();
        playout.push_audio(test_audio_packet(10_000, start, 80));
        playout.take_ready_audio(start, &mut ready);
        assert_eq!(playout.next_audio_pts_ms, 80);

        playout.push_audio(test_audio_packet(
            10_083,
            start + Duration::from_millis(83),
            80,
        ));
        ready.clear();
        playout.take_ready_audio(start + Duration::from_millis(83), &mut ready);
        assert!(matches!(
            ready.first(),
            Some(TimedPacket::Audio { pts_ms: 80, .. })
        ));
        assert_eq!(playout.generated_silence_ms, 0);

        playout.take_ready_audio(start + Duration::from_millis(1_980), &mut ready);
        playout.push_audio(test_audio_packet(
            12_000,
            start + Duration::from_millis(2_000),
            80,
        ));
        ready.clear();
        playout.take_ready_audio(start + Duration::from_millis(2_000), &mut ready);
        assert!(matches!(
            ready.last(),
            Some(TimedPacket::Audio { pts_ms: 2_030, data })
                if data.len() == audio_bytes_for_ms(80)
                    && data.iter().all(|byte| *byte == 1)
        ));
        assert_eq!(playout.generated_silence_ms, 1_870);
    }

    #[test]
    fn paced_audio_preserves_late_packets_and_drops_nonmonotonic_packets() {
        let config = test_playout_config();
        let start = Instant::now();
        let mut playout = PacedPlayout::new(&config).expect("create paced playout");
        playout.start(start);
        let mut ready = Vec::new();
        playout.push_audio(test_audio_packet(10_000, start, 80));
        playout.take_ready_audio(start, &mut ready);
        ready.clear();
        playout.take_ready_audio(start + Duration::from_millis(100), &mut ready);

        playout.push_audio(test_audio_packet(
            10_100,
            start + Duration::from_millis(100),
            80,
        ));
        ready.clear();
        playout.take_ready_audio(start + Duration::from_millis(100), &mut ready);
        assert!(matches!(
            ready.first(),
            Some(TimedPacket::Audio { pts_ms: 150, data }) if data.len() == audio_bytes_for_ms(80)
        ));

        playout.push_audio(test_audio_packet(
            10_000,
            start + Duration::from_millis(200),
            80,
        ));
        ready.clear();
        playout.take_ready_audio(start + Duration::from_millis(200), &mut ready);
        assert_eq!(playout.dropped_audio_packets, 1);
    }

    fn test_playout_config() -> PlayoutConfig {
        PlayoutConfig::new(&SessionConfig {
            serial: None,
            adb: PathBuf::from("adb"),
            fps: 60,
            width: 1800,
            height: 1920,
            audio: true,
            xrsp_port: 4445,
            http_port: 0,
        })
    }

    fn test_cast_config() -> CastConfig {
        CastConfig {
            serial: None,
            fps: 60,
            width: 1800,
            height: 1920,
            audio: true,
            adaptively_skip_frames: false,
            port: 4445,
            adb: PathBuf::from("adb"),
        }
    }

    fn test_audio_packet(timestamp_ms: i64, received_at: Instant, duration_ms: i64) -> AudioPacket {
        AudioPacket {
            timestamp_ms,
            received_at,
            data: Bytes::from(vec![1; audio_bytes_for_ms(duration_ms)]),
        }
    }

    fn test_buffered_video_frame() -> BufferedVideoFrame {
        BufferedVideoFrame {
            received_ns: 0,
            layer_id: 0,
            gop_index: 0,
            is_keyframe: true,
            data: vec![1],
        }
    }

    fn audio_packet_duration_ms(packets: &[TimedPacket]) -> i64 {
        packets
            .iter()
            .map(|packet| match packet {
                TimedPacket::Audio { data, .. } => audio_duration_ms(data.len()),
                TimedPacket::Video { .. } => 0,
            })
            .sum()
    }

    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn test_video_packet(received_at: Instant, is_keyframe: bool, nal_type: u8) -> VideoPacket {
        let data = if nal_type == 7 {
            vec![
                0, 0, 0, 1, 0x67, 0x64, 0, 0x28, 0xDE, 0xAD, 0, 0, 0, 1, 0x68, 0xEE, 0x3C, 0x80,
            ]
        } else {
            vec![0, 0, 0, 1, nal_type]
        };
        VideoPacket {
            received_at,
            layer_id: 0,
            gop_index: 2,
            canonical_index: 0,
            is_keyframe,
            data: Bytes::from(data),
        }
    }
}
