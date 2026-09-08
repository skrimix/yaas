//! XRSP protocol session with a Meta Quest device.
//!
//! Runs the device-facing TCP server, performs the ADB casting bring-up,
//! and forwards decoded video/audio packets as [`StreamEvent`]s.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::adb;

const MESSAGE_HANDSHAKE: u32 = 1;
const MESSAGE_PING: u32 = 3;
const MESSAGE_VIDEO_SEGMENT: u32 = 100;
const MESSAGE_AUDIO_SEGMENT: u32 = 102;
const MESSAGE_LAYER_CONFIGURATION: u32 = 300;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);
const VIDEO_STALL_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONSECUTIVE_NO_VIDEO_FAILURES: usize = 3;

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug)]
pub struct CastConfig {
    pub device: forensic_adb::Device,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub audio: bool,
    pub adaptively_skip_frames: bool,
    pub port: u16,
}

#[derive(Debug)]
pub struct XrspPacket {
    pub word0: u16,
    pub topic: u16,
    pub payload: Bytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppHeader {
    pub app_id: u32,
    pub datagram_id: u32,
    pub message_id: u32,
    pub partial_index: u32,
    pub partial_count: u32,
    pub qos: u32,
}

#[derive(Debug)]
pub struct AppDatagram {
    pub header: AppHeader,
    pub body: Bytes,
    pub received_at: Instant,
}

#[derive(Debug)]
pub struct VideoPacket {
    pub received_at: Instant,
    pub layer_id: u32,
    pub gop_index: u32,
    pub canonical_index: u32,
    pub is_keyframe: bool,
    pub data: Bytes,
}

#[derive(Debug)]
pub struct AudioPacket {
    pub timestamp_ms: i64,
    pub received_at: Instant,
    pub data: Bytes,
}

#[derive(Debug)]
pub enum StreamEvent {
    Video(VideoPacket),
    Audio(AudioPacket),
    /// Marks the end of a device casting session.
    ///
    /// Packets after this event belong to a new session and may restart source counters.
    Discontinuity,
}

/// Live playback stats, reported about once per second while playback is running.
#[derive(Debug, Clone, Copy)]
pub struct LiveStats {
    /// Measured input frame rate over the last reporting window.
    pub fps: f64,
    /// Frames currently held in the playback buffer.
    pub buffered_frames: u32,
    /// Age of the oldest buffered frame; how far playout lags behind capture.
    pub buffer_age_ms: f64,
    /// Mean presentation latency of the frames presented in the last window.
    pub latency_ms: f64,
}

/// Session-level events reported to the embedding application.
#[derive(Debug)]
pub enum SessionEvent {
    /// The device opened an XRSP stream connection.
    DeviceConnected,
    /// The device stream dropped and a recovery attempt is starting.
    Recovering,
    /// The HTTP player client connected to the stream.
    PlayerConnected,
    /// Paced playback started or resumed after a rebuffer.
    PlaybackStarted,
    /// The HTTP player disconnected.
    PlayerDisconnected,
    /// Live playback stats.
    Stats(LiveStats),
    /// The session ended; `Some(error)` on failure.
    Ended(Option<String>),
}

#[derive(Debug, Default)]
struct SharedState {
    app_id: u32,
    next_datagram_id: u32,
    next_topic: u16,
    output_name: Option<String>,
    output_socket: Option<TcpStream>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            app_id: 0x1D83_3066,
            next_datagram_id: 1,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct LiveSession {
    config: CastConfig,
    state: Arc<Mutex<SharedState>>,
    stop: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    connections: Arc<Mutex<Vec<TcpStream>>>,
    video_segments: Arc<AtomicUsize>,
    last_video_at: Arc<Mutex<Option<Instant>>>,
    event_tx: mpsc::Sender<StreamEvent>,
    session_tx: mpsc::Sender<SessionEvent>,
}

/// Handle for sending live casting control messages.
#[derive(Clone)]
pub struct LiveControl {
    state: Arc<Mutex<SharedState>>,
}

impl LiveControl {
    /// Requests a new sync frame for `layer_id`.
    pub fn request_resync_frame(&self, layer_id: u32) -> Result<()> {
        let layer_id = i32::try_from(layer_id).map_err(|_| "layer ID is too large")?;
        send_shared_message(&self.state, &encode_resync_frame(layer_id))
    }
}

#[derive(Debug, Eq, PartialEq)]
enum RecoveryOutcome {
    Recover {
        reason: String,
        consecutive_no_video: usize,
    },
    CrashLoop {
        reason: String,
    },
}

#[derive(Debug)]
struct RecoveryTracker {
    handled_connections: usize,
    video_segments_at_attempt_start: usize,
    consecutive_no_video: usize,
    waiting_since: Instant,
}

impl RecoveryTracker {
    fn new(now: Instant, accepted: usize, video_segments: usize) -> Self {
        Self {
            handled_connections: accepted,
            video_segments_at_attempt_start: video_segments,
            consecutive_no_video: 0,
            waiting_since: now,
        }
    }

    fn begin_attempt(&mut self, now: Instant, accepted: usize, video_segments: usize) {
        self.handled_connections = accepted;
        self.video_segments_at_attempt_start = video_segments;
        self.waiting_since = now;
    }

    fn poll(
        &mut self,
        now: Instant,
        accepted: usize,
        active_connections: usize,
        video_segments: usize,
        last_video_at: Option<Instant>,
    ) -> Option<RecoveryOutcome> {
        let produced_video = video_segments > self.video_segments_at_attempt_start;
        if produced_video {
            self.consecutive_no_video = 0;
        }

        let reason = if active_connections == 0 && accepted > self.handled_connections {
            self.handled_connections = accepted;
            "device stream closed".to_string()
        } else if active_connections == 0
            && accepted == self.handled_connections
            && now.saturating_duration_since(self.waiting_since) >= CALLBACK_TIMEOUT
        {
            format!(
                "device did not open a stream connection within {:?}",
                CALLBACK_TIMEOUT
            )
        } else if active_connections > 0
            && produced_video
            && last_video_at
                .is_some_and(|last| now.saturating_duration_since(last) >= VIDEO_STALL_TIMEOUT)
        {
            // The connection is held open but video stopped flowing. Only arm the watchdog
            // once the current attempt has produced video, so bring-up grace is unaffected.
            format!("no video for {:?}", VIDEO_STALL_TIMEOUT)
        } else {
            return None;
        };

        if !produced_video {
            self.consecutive_no_video += 1;
        }
        if self.consecutive_no_video >= MAX_CONSECUTIVE_NO_VIDEO_FAILURES {
            Some(RecoveryOutcome::CrashLoop { reason })
        } else {
            Some(RecoveryOutcome::Recover {
                reason,
                consecutive_no_video: self.consecutive_no_video,
            })
        }
    }
}

#[derive(Debug)]
struct PendingPartials {
    partial_count: usize,
    partials: Vec<XrspPacket>,
}

#[derive(Debug)]
struct PendingAppFragments {
    header: AppHeader,
    bodies: Vec<Bytes>,
}

impl LiveSession {
    pub fn new(
        config: CastConfig,
        stop: Arc<AtomicBool>,
        event_tx: mpsc::Sender<StreamEvent>,
        session_tx: mpsc::Sender<SessionEvent>,
    ) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(SharedState::new())),
            stop,
            active_connections: Arc::new(AtomicUsize::new(0)),
            connections: Arc::new(Mutex::new(Vec::new())),
            video_segments: Arc::new(AtomicUsize::new(0)),
            last_video_at: Arc::new(Mutex::new(None)),
            event_tx,
            session_tx,
        }
    }

    /// Returns a handle that can send control messages while the server is running.
    pub fn control(&self) -> LiveControl {
        LiveControl {
            state: self.state.clone(),
        }
    }

    pub fn run_server(&self) -> Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", self.config.port))?;
        listener.set_nonblocking(true)?;
        info!(port = self.config.port, "XRSP server listening");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = self.run_accept_loop(&listener, &runtime);
        runtime.block_on(run_adb_teardown(&self.config));
        result
    }

    fn run_accept_loop(
        &self,
        listener: &TcpListener,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<()> {
        runtime.block_on(run_adb_setup_with_reconnect(&self.config, &self.stop))?;

        let mut accepted = 0usize;
        let mut workers = Vec::new();
        let mut recovery = RecoveryTracker::new(
            Instant::now(),
            accepted,
            self.video_segments.load(Ordering::SeqCst),
        );

        while !self.stop.load(Ordering::SeqCst) {
            let active_connections = self.active_connections.load(Ordering::SeqCst);
            let video_segments = self.video_segments.load(Ordering::SeqCst);
            let last_video_at = *self.last_video_at.lock().expect("last video lock poisoned");
            if let Some(outcome) = recovery.poll(
                Instant::now(),
                accepted,
                active_connections,
                video_segments,
                last_video_at,
            ) {
                match outcome {
                    RecoveryOutcome::Recover {
                        reason,
                        consecutive_no_video,
                    } => {
                        warn!(
                            reason,
                            consecutive_no_video, "restarting device casting after failure"
                        );
                        let _ = self.event_tx.send(StreamEvent::Discontinuity);
                        let _ = self.session_tx.send(SessionEvent::Recovering);
                        self.reset_host_session_state();
                        reap_finished_workers(&mut workers);
                        runtime
                            .block_on(run_adb_recovery_with_reconnect(&self.config, &self.stop))?;
                        recovery.begin_attempt(
                            Instant::now(),
                            accepted,
                            self.video_segments.load(Ordering::SeqCst),
                        );
                        continue;
                    }
                    RecoveryOutcome::CrashLoop { reason } => {
                        let _ = self.event_tx.send(StreamEvent::Discontinuity);
                        return Err(format!(
                            "device casting produced no video in \
                             {MAX_CONSECUTIVE_NO_VIDEO_FAILURES} consecutive recovery attempts; \
                             giving up (last failure: {reason})"
                        )
                        .into());
                    }
                }
            }

            match listener.accept() {
                Ok((stream, address)) => {
                    accepted += 1;
                    let name = format!("conn{accepted}");
                    info!(
                        %name,
                        ip = %address.ip(),
                        port = address.port(),
                        "device connected"
                    );
                    stream.set_nonblocking(false)?;
                    stream.set_nodelay(true).ok();
                    self.active_connections.fetch_add(1, Ordering::SeqCst);
                    let _ = self.session_tx.send(SessionEvent::DeviceConnected);

                    {
                        let mut state = self.state.lock().expect("state lock poisoned");
                        if state.output_socket.is_none() {
                            state.output_name = Some(name.clone());
                            state.output_socket = Some(stream.try_clone()?);
                            debug!(%name, "using this connection for host-to-device messages");
                        }
                    }

                    self.connections
                        .lock()
                        .expect("connections lock poisoned")
                        .push(stream.try_clone()?);

                    let worker_session = self.clone();
                    workers.push(thread::spawn(move || {
                        worker_session.connection_loop(name, stream);
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    reap_finished_workers(&mut workers);
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error.into()),
            }
        }

        for socket in self
            .connections
            .lock()
            .expect("connections lock poisoned")
            .iter()
        {
            let _ = socket.shutdown(Shutdown::Both);
        }

        for worker in workers {
            let _ = worker.join();
        }

        Ok(())
    }

    fn reset_host_session_state(&self) {
        let mut state = self.state.lock().expect("state lock poisoned");
        state.output_socket = None;
        state.output_name = None;
        state.next_datagram_id = 1;
        state.next_topic = 0;
        self.connections
            .lock()
            .expect("connections lock poisoned")
            .clear();
    }

    fn connection_loop(&self, name: String, mut stream: TcpStream) {
        let mut pending_partials: Option<PendingPartials> = None;
        let mut pending_app_fragments: Option<PendingAppFragments> = None;

        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }

            let packet = match read_xrsp_packet(&mut stream) {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    info!(%name, "connection closed");
                    break;
                }
                Err(error) => {
                    warn!(%name, "connection error: {error}");
                    break;
                }
            };

            if packet.payload.starts_with(b"MGIK") && packet.payload.len() >= 8 {
                let partial_count = read_u32_le(&packet.payload, 4).unwrap_or(0) as usize;
                pending_partials = Some(PendingPartials {
                    partial_count,
                    partials: Vec::with_capacity(partial_count),
                });
                continue;
            }

            if let Some(pending) = pending_partials.as_mut() {
                pending.partials.push(packet);
                if pending.partials.len() == pending.partial_count {
                    let pending = pending_partials.take().expect("pending partials exist");
                    if let Some(datagram) = datagram_from_partials(pending.partials) {
                        if let Some(datagram) =
                            reassemble_app_fragment(datagram, &mut pending_app_fragments)
                        {
                            self.handle_datagram(&name, datagram);
                        }
                    } else {
                        debug!(%name, "ignoring partial group without an app header");
                    }
                }
                continue;
            }

            if let Some(header) = parse_app_header(&packet.payload) {
                let datagram = AppDatagram {
                    header,
                    body: packet.payload.slice(24..),
                    received_at: Instant::now(),
                };
                if let Some(datagram) =
                    reassemble_app_fragment(datagram, &mut pending_app_fragments)
                {
                    self.handle_datagram(&name, datagram);
                }
            } else {
                debug!(
                    %name,
                    word0 = format_args!("0x{:04x}", packet.word0),
                    topic = packet.topic,
                    payload_bytes = packet.payload.len(),
                    payload_prefix_hex = hex_head(&packet.payload),
                    "unhandled packet"
                );
            }
        }

        self.active_connections.fetch_sub(1, Ordering::SeqCst);
        let mut state = self.state.lock().expect("state lock poisoned");
        if state.output_name.as_deref() == Some(&name) {
            state.output_socket = None;
            state.output_name = None;
        }
    }

    fn handle_datagram(&self, name: &str, datagram: AppDatagram) {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state.app_id = datagram.header.app_id;
        }

        let message_type = match read_u32_be(&datagram.body, 0) {
            Some(value) => value,
            None => return,
        };

        match message_type {
            MESSAGE_HANDSHAKE => {
                info!(%name, "received Handshake; sending initial casting configuration");
                self.send_initial_casting_messages();
            }
            MESSAGE_PING => {
                if let Some(timestamp) = read_i32_be(&datagram.body, 4) {
                    self.send_message(&encode_pong(timestamp));
                }
            }
            MESSAGE_LAYER_CONFIGURATION => {
                info!(%name, "received LayerConfiguration; activating stream layer");
                self.send_message(&encode_resync_frame(0));
                self.send_message(&encode_input_forwarding_state(0));
                if self.config.audio {
                    self.send_message(&encode_set_audio_state(1));
                }
                self.send_message(&encode_resync_frame(0));
                self.send_message(&encode_activate_layer(0));
                self.send_message(&encode_resync_frame(0));
            }
            MESSAGE_VIDEO_SEGMENT => {
                if datagram.body.len() < 16 {
                    return;
                }
                let payload = datagram.body.slice(16..);
                let layer_id = read_u32_be(&datagram.body, 4).unwrap_or(0);
                let gop_index = read_u32_be(&datagram.body, 8).unwrap_or(0);
                let canonical_index = read_u32_be(&datagram.body, 12).unwrap_or(0);
                let nal_types = h264_nal_types(&payload);
                let is_keyframe = nal_types.contains(&5);

                if self.video_segments.fetch_add(1, Ordering::SeqCst) == 0 {
                    info!(
                        %name,
                        bytes = payload.len(),
                        is_keyframe,
                        nal_types = join_numbers(&nal_types),
                        "first VideoSegment"
                    );
                }
                *self.last_video_at.lock().expect("last video lock poisoned") =
                    Some(datagram.received_at);
                let _ = self.event_tx.send(StreamEvent::Video(VideoPacket {
                    received_at: datagram.received_at,
                    layer_id,
                    gop_index,
                    canonical_index,
                    is_keyframe,
                    data: payload,
                }));
            }
            MESSAGE_AUDIO_SEGMENT => {
                if !self.config.audio || datagram.body.len() < 16 {
                    return;
                }
                let Some(timestamp_ms) = read_i64_be(&datagram.body, 8) else {
                    return;
                };
                let pcm = pcm_s16be_to_s16le(&datagram.body[16..]);
                let _ = self.event_tx.send(StreamEvent::Audio(AudioPacket {
                    timestamp_ms,
                    received_at: datagram.received_at,
                    data: Bytes::from(pcm),
                }));
            }
            _ => {}
        }
    }

    fn send_initial_casting_messages(&self) {
        self.send_message(&encode_set_device_eye_fov_config(0, 0, 0, 0));
        self.send_message(&encode_start_casting(
            self.config.width,
            self.config.height,
            0.0,
            1,
            self.config.fps,
            0,
        ));
    }

    fn send_message(&self, body: &[u8]) {
        if let Err(error) = send_shared_message(&self.state, body) {
            warn!("failed to send message to device: {error}");
        }
    }
}

fn reap_finished_workers(workers: &mut Vec<thread::JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            let _ = worker.join();
        } else {
            index += 1;
        }
    }
}

pub fn datagram_from_partials(partials: Vec<XrspPacket>) -> Option<AppDatagram> {
    let first = partials.first()?;
    let header = parse_app_header(&first.payload)?;
    let received_at = Instant::now();
    let body_len = partials
        .iter()
        .skip(1)
        .map(|packet| packet.payload.len())
        .sum();
    let mut body = BytesMut::with_capacity(body_len);
    for packet in partials.into_iter().skip(1) {
        body.extend_from_slice(&packet.payload);
    }
    Some(AppDatagram {
        header,
        body: body.freeze(),
        received_at,
    })
}

fn reassemble_app_fragment(
    datagram: AppDatagram,
    pending: &mut Option<PendingAppFragments>,
) -> Option<AppDatagram> {
    if datagram.header.partial_count <= 1 {
        *pending = None;
        return Some(datagram);
    }

    if datagram.header.partial_index == 0 {
        *pending = Some(PendingAppFragments {
            header: datagram.header,
            bodies: vec![datagram.body],
        });
        return None;
    }

    let group = pending.as_mut()?;

    let expected_index = group.bodies.len() as u32;
    let matches_group = group.header.app_id == datagram.header.app_id
        && group.header.message_id == datagram.header.message_id
        && group.header.partial_count == datagram.header.partial_count
        && datagram.header.partial_index == expected_index;
    if !matches_group {
        *pending = None;
        return None;
    }

    group.bodies.push(datagram.body);
    if group.bodies.len() < group.header.partial_count as usize {
        return None;
    }

    let group = pending.take().expect("pending app fragments exist");
    let byte_len = group.bodies.iter().map(Bytes::len).sum();
    let mut body = BytesMut::with_capacity(byte_len);
    for fragment in group.bodies {
        body.extend_from_slice(&fragment);
    }

    Some(AppDatagram {
        header: group.header,
        body: body.freeze(),
        received_at: datagram.received_at,
    })
}

pub fn parse_app_header(payload: &[u8]) -> Option<AppHeader> {
    if payload.len() < 24 {
        return None;
    }
    let header = AppHeader {
        app_id: read_u32_be(payload, 0)?,
        datagram_id: read_u32_be(payload, 4)?,
        message_id: read_u32_be(payload, 8)?,
        partial_index: read_u32_be(payload, 12)?,
        partial_count: read_u32_be(payload, 16)?,
        qos: read_u32_be(payload, 20)?,
    };
    is_plausible_app_header(header).then_some(header)
}

fn is_plausible_app_header(header: AppHeader) -> bool {
    header.app_id != 0
        && header.partial_index < header.partial_count.max(1)
        && (1..=128).contains(&header.partial_count)
        && header.qos <= 8
        && header.datagram_id < 1_000_000
        && header.message_id < 1_000_000
}

pub fn make_xrsp_packet(payload: &[u8], topic: u16) -> Vec<u8> {
    let total_length = 8 + payload.len();
    let padded_length = (total_length + 3) & !3;
    let mut packet = Vec::with_capacity(padded_length);
    packet.extend_from_slice(&0x0210u16.to_le_bytes());
    packet.extend_from_slice(&((padded_length / 4 - 1) as u16).to_le_bytes());
    packet.extend_from_slice(&topic.to_le_bytes());
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(payload);
    packet.resize(padded_length, 0);
    packet
}

pub fn h264_nal_types(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut index = 0usize;
    while let Some(start) = find_annex_b_start(payload, index) {
        let nal_offset = if payload[start..].starts_with(&[0, 0, 1]) {
            start + 3
        } else {
            start + 4
        };
        if nal_offset < payload.len() {
            types.push(payload[nal_offset] & 0x1F);
        }
        index = nal_offset.saturating_add(1);
    }
    types
}

pub fn h264_dimensions_from_avcc(config_record: &[u8]) -> Option<(u32, u32)> {
    if config_record.len() < 8 || config_record[0] != 1 {
        return None;
    }
    let sps_count = config_record[5] & 0x1F;
    if sps_count == 0 {
        return None;
    }
    let sps_len = u16::from_be_bytes([config_record[6], config_record[7]]) as usize;
    let sps = config_record.get(8..8 + sps_len)?;
    h264_dimensions_from_sps(sps)
}

pub fn pcm_s16be_to_s16le(data: &[u8]) -> Vec<u8> {
    let even_len = data.len() & !1;
    let mut output = Vec::with_capacity(even_len);
    for chunk in data[..even_len].as_chunks::<2>().0 {
        output.push(chunk[1]);
        output.push(chunk[0]);
    }
    output
}

pub fn effective_fixed_fps(fps: u32) -> u32 {
    // TODO: this is wrong above 60 (test device seems to max out at ~63)
    match fps {
        0 => 30,
        1..=60 => fps,
        _ => 60,
    }
}

fn read_xrsp_packet(stream: &mut TcpStream) -> io::Result<Option<XrspPacket>> {
    let mut header = [0u8; 8];
    if !read_exact_or_closed(stream, &mut header)? {
        return Ok(None);
    }

    let word0 = u16::from_le_bytes([header[0], header[1]]);
    let length_words_minus_one = u16::from_le_bytes([header[2], header[3]]);
    let topic = u16::from_le_bytes([header[4], header[5]]);
    let total_length = (usize::from(length_words_minus_one) + 1) * 4;
    if total_length < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad XRSP packet length: {total_length}"),
        ));
    }

    let mut payload = vec![0u8; total_length - 8];
    if !read_exact_or_closed(stream, &mut payload)? {
        return Ok(None);
    }

    Ok(Some(XrspPacket {
        word0,
        topic,
        payload: Bytes::from(payload),
    }))
}

fn read_exact_or_closed(stream: &mut TcpStream, output: &mut [u8]) -> io::Result<bool> {
    let mut read = 0usize;
    while read < output.len() {
        match stream.read(&mut output[read..]) {
            Ok(0) => return Ok(false),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn app_payload(app_id: u32, datagram_id: u32, message_id: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(24 + body.len());
    for value in [app_id, datagram_id, message_id, 0, 1, 2] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.extend_from_slice(body);
    payload
}

fn send_shared_message(state: &Arc<Mutex<SharedState>>, body: &[u8]) -> Result<()> {
    let mut state = state.lock().expect("state lock poisoned");
    if state.output_socket.is_none() {
        return Err("no device connection available for host-to-device message".into());
    }

    let datagram_id = state.next_datagram_id;
    state.next_datagram_id = state.next_datagram_id.wrapping_add(1);
    let topic = state.next_topic;
    state.next_topic = state.next_topic.wrapping_add(1);

    let payload = app_payload(state.app_id, datagram_id, datagram_id, body);
    let packet = make_xrsp_packet(&payload, topic);
    state
        .output_socket
        .as_mut()
        .expect("output socket was checked above")
        .write_all(&packet)?;
    Ok(())
}

fn encode_set_device_eye_fov_config(inward: i32, outward: i32, up: i32, down: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(20);
    body.extend_from_slice(&600u32.to_be_bytes());
    for value in [inward, outward, up, down] {
        body.extend_from_slice(&value.to_be_bytes());
    }
    body
}

fn encode_start_casting(
    width: u32,
    height: u32,
    fov: f32,
    eye: u32,
    fps: u32,
    stabilization: u32,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(28);
    body.extend_from_slice(&7u32.to_be_bytes());
    body.extend_from_slice(&width.to_be_bytes());
    body.extend_from_slice(&height.to_be_bytes());
    body.extend_from_slice(&fov.to_be_bytes());
    body.extend_from_slice(&eye.to_be_bytes());
    body.extend_from_slice(&fps.to_be_bytes());
    body.extend_from_slice(&stabilization.to_be_bytes());
    body
}

fn encode_pong(timestamp: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(8);
    body.extend_from_slice(&4u32.to_be_bytes());
    body.extend_from_slice(&timestamp.to_be_bytes());
    body
}

fn encode_resync_frame(layer_id: i32) -> Vec<u8> {
    encode_i32_message(101, layer_id)
}

fn encode_activate_layer(layer_id: i32) -> Vec<u8> {
    encode_i32_message(301, layer_id)
}

fn encode_input_forwarding_state(state: i32) -> Vec<u8> {
    encode_i32_message(205, state)
}

fn encode_set_audio_state(state: i32) -> Vec<u8> {
    encode_i32_message(503, state)
}

fn encode_i32_message(message_type: u32, value: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(8);
    body.extend_from_slice(&message_type.to_be_bytes());
    body.extend_from_slice(&value.to_be_bytes());
    body
}

fn h264_dimensions_from_sps(sps: &[u8]) -> Option<(u32, u32)> {
    let rbsp = h264_rbsp_from_ebsp(sps.get(1..)?);
    let mut bits = BitReader::new(&rbsp);
    let profile_idc = bits.read_bits(8)? as u8;
    bits.read_bits(8)?;
    bits.read_bits(8)?;
    bits.read_ue()?;

    let mut chroma_format_idc = 1;
    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = bits.read_ue()?;
        if chroma_format_idc == 3 {
            bits.read_bit()?;
        }
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_bit()?;
        if bits.read_bit()? != 0 {
            return None;
        }
    }

    bits.read_ue()?;
    let pic_order_cnt_type = bits.read_ue()?;
    if pic_order_cnt_type == 0 {
        bits.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        bits.read_bit()?;
        bits.read_se()?;
        bits.read_se()?;
        let cycle_count = bits.read_ue()?;
        for _ in 0..cycle_count {
            bits.read_se()?;
        }
    }

    bits.read_ue()?;
    bits.read_bit()?;
    let width_in_mbs = bits.read_ue()? + 1;
    let height_in_map_units = bits.read_ue()? + 1;
    let frame_mbs_only_flag = bits.read_bit()?;
    if frame_mbs_only_flag == 0 {
        bits.read_bit()?;
    }
    bits.read_bit()?;

    let mut crop_left = 0;
    let mut crop_right = 0;
    let mut crop_top = 0;
    let mut crop_bottom = 0;
    if bits.read_bit()? != 0 {
        crop_left = bits.read_ue()?;
        crop_right = bits.read_ue()?;
        crop_top = bits.read_ue()?;
        crop_bottom = bits.read_ue()?;
    }

    let width = width_in_mbs * 16;
    let height = (2 - frame_mbs_only_flag) * height_in_map_units * 16;
    let crop_unit_x = if chroma_format_idc == 0 { 1 } else { 2 };
    let crop_unit_y = match chroma_format_idc {
        0 => 2 - frame_mbs_only_flag,
        1 => 2 * (2 - frame_mbs_only_flag),
        _ => 2 - frame_mbs_only_flag,
    };

    Some((
        width.saturating_sub((crop_left + crop_right) * crop_unit_x),
        height.saturating_sub((crop_top + crop_bottom) * crop_unit_y),
    ))
}

fn h264_rbsp_from_ebsp(ebsp: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(ebsp.len());
    let mut zeros = 0u8;
    for &byte in ebsp {
        if zeros >= 2 && byte == 0x03 {
            zeros = 0;
            continue;
        }
        rbsp.push(byte);
        zeros = if byte == 0 {
            zeros.saturating_add(1)
        } else {
            0
        };
    }
    rbsp
}

struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn read_bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.bit / 8)?;
        let shift = 7 - (self.bit % 8);
        self.bit += 1;
        Some(u32::from((byte >> shift) & 1))
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.read_bit()?;
        }
        Some(value)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zero_bits = 0;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return None;
            }
        }
        let suffix = if leading_zero_bits == 0 {
            0
        } else {
            self.read_bits(leading_zero_bits)?
        };
        Some((1 << leading_zero_bits) - 1 + suffix)
    }

    fn read_se(&mut self) -> Option<i32> {
        let value = self.read_ue()? as i32;
        Some(if value % 2 == 0 {
            -(value / 2)
        } else {
            (value + 1) / 2
        })
    }
}

fn find_annex_b_start(payload: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index + 3 <= payload.len() {
        if payload[index..].starts_with(&[0, 0, 1])
            || (index + 4 <= payload.len() && payload[index..].starts_with(&[0, 0, 0, 1]))
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

async fn run_adb_setup_with_reconnect(config: &CastConfig, stop: &AtomicBool) -> Result<()> {
    match run_adb_setup(config).await {
        Ok(()) => Ok(()),
        Err(error) => {
            warn!("initial ADB setup failed: {error}; reconnecting ADB");
            reconnect_adb_and_setup(config, stop).await
        }
    }
}

async fn run_adb_recovery_with_reconnect(config: &CastConfig, stop: &AtomicBool) -> Result<()> {
    match run_adb_recovery(config).await {
        Ok(()) => Ok(()),
        Err(error) => {
            warn!("casting recovery failed: {error}; reconnecting ADB");
            reconnect_adb_and_setup(config, stop).await
        }
    }
}

async fn reconnect_adb_and_setup(config: &CastConfig, stop: &AtomicBool) -> Result<()> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Err(error) = adb::reconnect(&config.device).await {
            debug!("ADB reconnect failed: {error}");
        }
        info!("waiting for ADB device");
        if !adb::wait_for_device(&config.device, stop).await? {
            return Ok(());
        }
        info!("ADB device available; rebuilding casting setup");

        match run_adb_setup(config).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if adb::device_available(&config.device).await? {
                    return Err(
                        format!("ADB setup failed while device was available: {error}").into(),
                    );
                }
                info!("device disappeared during ADB setup; waiting for it again");
            }
        }
    }
}

async fn run_adb_setup(config: &CastConfig) -> Result<()> {
    let serial = adb::shell(&config.device, &["getprop", "ro.serialno"]).await?;
    let session_id = Uuid::new_v4().to_string();
    let reconnect_delay = Duration::from_millis(500);
    let init_delay = Duration::from_secs(1);
    let fps = effective_fixed_fps(config.fps).to_string();
    let min_fps = if config.adaptively_skip_frames {
        ""
    } else {
        fps.as_str()
    };

    adb::shell_best_effort(
        &config.device,
        &[
            "am",
            "startservice",
            "-a",
            "STOP_CASTING",
            "com.oculus.metacam/com.oculus.metacam.casting.CastingService",
        ],
    )
    .await?;
    tokio::time::sleep(reconnect_delay).await;
    adb::shell_best_effort(
        &config.device,
        &[
            "am",
            "broadcast",
            "-a",
            "com.oculus.magicislandcastingservice.STOP_CASTING",
        ],
    )
    .await?;
    adb::shell_best_effort(
        &config.device,
        &[
            "am",
            "broadcast",
            "-a",
            "com.oculus.magicislandcastingservice.DISABLE_PANEL_STREAMING",
        ],
    )
    .await?;
    tokio::time::sleep(reconnect_delay).await;
    adb::shell(
        &config.device,
        &["setprop", "debug.oculus.command_line_media_capture", "true"],
    )
    .await?;
    adb::shell(
        &config.device,
        &["setprop", "debug.oculus.magic.enabled", "1"],
    )
    .await?;
    adb::shell(
        &config.device,
        &["setprop", "debug.oculus.magic.serialNumber", serial.trim()],
    )
    .await?;
    adb::shell(
        &config.device,
        &["setprop", "debug.oculus.magic.maxFps", &fps],
    )
    .await?;
    adb::shell(
        &config.device,
        &["setprop", "debug.oculus.magic.minFps", min_fps],
    )
    .await?;
    if let Err(error) = adb::remove_reverse(&config.device, config.port).await {
        debug!("casting reverse removal failed: {error}");
    }
    adb::shell(
        &config.device,
        &[
            "setprop",
            "debug.oculus.magic.port",
            &config.port.to_string(),
        ],
    )
    .await?;
    adb::reverse(&config.device, config.port).await?;
    adb::shell(
        &config.device,
        &[
            "am",
            "start-foreground-service",
            "-n",
            "com.oculus.magicislandcastingservice/.CastingService",
            "--ez",
            "use_openxr",
            "true",
        ],
    )
    .await?;
    tokio::time::sleep(init_delay).await;
    adb::shell(
        &config.device,
        &[
            "am",
            "startservice",
            "-a",
            "CONNECT_MAGIC_CASTING",
            "--es",
            "session_id",
            &session_id,
            "--es",
            "SESSION_ID",
            &session_id,
            "com.oculus.metacam/com.oculus.metacam.casting.CastingService",
        ],
    )
    .await?;
    Ok(())
}

async fn run_adb_recovery(config: &CastConfig) -> Result<()> {
    adb::shell(
        &config.device,
        &[
            "am",
            "broadcast",
            "-a",
            "com.oculus.magicisland.sdk.intent.CONNECT",
        ],
    )
    .await?;
    adb::shell(
        &config.device,
        &[
            "am",
            "broadcast",
            "-a",
            "com.oculus.vrguardianservice.JsonCmdUserBroadcast",
            "--es",
            "cmd",
            r#"{"settings":{"name":"guardian_paused","action":"set","val":true}}"#,
        ],
    )
    .await?;
    adb::shell(
        &config.device,
        &[
            "am",
            "start-foreground-service",
            "-n",
            "com.oculus.magicislandcastingservice/.CastingService",
            "--ez",
            "use_openxr",
            "true",
        ],
    )
    .await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    adb::shell(
        &config.device,
        &[
            "am",
            "broadcast",
            "-a",
            "com.oculus.magicislandcastingservice.CONNECT",
        ],
    )
    .await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok(())
}

/// Best-effort device cleanup after a session ends, matching the official tool's shutdown
/// broadcast and removing the `adb reverse` forward set up at session start.
async fn run_adb_teardown(config: &CastConfig) {
    if let Err(error) = adb::shell_best_effort(
        &config.device,
        &[
            "am",
            "broadcast",
            "-a",
            "com.oculus.magicislandcastingservice.DISABLE_PANEL_STREAMING",
        ],
    )
    .await
    {
        debug!("casting teardown broadcast failed: {error}");
    }
    if let Err(error) = adb::remove_reverse(&config.device, config.port).await {
        debug!("casting teardown reverse removal failed: {error}");
    }
}

pub fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().expect("slice length checked")))
}

pub fn read_i32_be(data: &[u8], offset: usize) -> Option<i32> {
    data.get(offset..offset + 4)
        .map(|bytes| i32::from_be_bytes(bytes.try_into().expect("slice length checked")))
}

pub fn read_i64_be(data: &[u8], offset: usize) -> Option<i64> {
    data.get(offset..offset + 8)
        .map(|bytes| i64::from_be_bytes(bytes.try_into().expect("slice length checked")))
}

pub fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("slice length checked")))
}

fn hex_head(data: &[u8]) -> String {
    data.iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn join_numbers(numbers: &[u8]) -> String {
    numbers
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_tracker_allows_repeated_healthy_recoveries() {
        let now = Instant::now();
        let mut tracker = RecoveryTracker::new(now, 0, 0);

        for connection in 1..=5 {
            assert_eq!(tracker.poll(now, connection, 1, connection - 1, None), None);
            assert_eq!(
                tracker.poll(now, connection, 0, connection, None),
                Some(RecoveryOutcome::Recover {
                    reason: "device stream closed".to_string(),
                    consecutive_no_video: 0,
                })
            );
            tracker.begin_attempt(now, connection, connection);
        }
    }

    #[test]
    fn recovery_tracker_stops_after_three_no_video_connections() {
        let now = Instant::now();
        let mut tracker = RecoveryTracker::new(now, 0, 0);

        for connection in 1..=2 {
            assert_eq!(
                tracker.poll(now, connection, 0, 0, None),
                Some(RecoveryOutcome::Recover {
                    reason: "device stream closed".to_string(),
                    consecutive_no_video: connection,
                })
            );
            tracker.begin_attempt(now, connection, 0);
        }
        assert_eq!(
            tracker.poll(now, 3, 0, 0, None),
            Some(RecoveryOutcome::CrashLoop {
                reason: "device stream closed".to_string()
            })
        );
    }

    #[test]
    fn recovery_tracker_counts_callback_timeouts_and_resets_after_video() {
        let now = Instant::now();
        let timeout = now + CALLBACK_TIMEOUT;
        let mut tracker = RecoveryTracker::new(now, 0, 0);

        assert_eq!(
            tracker.poll(timeout, 0, 0, 0, None),
            Some(RecoveryOutcome::Recover {
                reason: format!(
                    "device did not open a stream connection within {:?}",
                    CALLBACK_TIMEOUT
                ),
                consecutive_no_video: 1,
            })
        );
        tracker.begin_attempt(timeout, 0, 0);
        assert_eq!(tracker.poll(timeout, 1, 1, 1, None), None);
        assert_eq!(tracker.consecutive_no_video, 0);
    }

    #[test]
    fn recovery_tracker_recovers_on_video_stall() {
        let now = Instant::now();
        let mut tracker = RecoveryTracker::new(now, 0, 0);

        // Video is flowing on an open connection.
        assert_eq!(tracker.poll(now, 1, 1, 10, Some(now)), None);
        // Video arrived within the stall window.
        assert_eq!(
            tracker.poll(
                now + VIDEO_STALL_TIMEOUT,
                1,
                1,
                20,
                Some(now + VIDEO_STALL_TIMEOUT)
            ),
            None
        );
        // No new video for the whole stall window while the connection stays open.
        assert_eq!(
            tracker.poll(
                now + VIDEO_STALL_TIMEOUT * 2,
                1,
                1,
                20,
                Some(now + VIDEO_STALL_TIMEOUT)
            ),
            Some(RecoveryOutcome::Recover {
                reason: format!("no video for {:?}", VIDEO_STALL_TIMEOUT),
                consecutive_no_video: 0,
            })
        );

        // A new attempt disarms the watchdog until it produces video again.
        tracker.begin_attempt(now + VIDEO_STALL_TIMEOUT * 2, 1, 20);
        assert_eq!(
            tracker.poll(
                now + VIDEO_STALL_TIMEOUT * 3,
                1,
                1,
                20,
                Some(now + VIDEO_STALL_TIMEOUT)
            ),
            None
        );
    }

    #[test]
    fn recovery_tracker_stall_watchdog_waits_for_first_video() {
        let now = Instant::now();
        let mut tracker = RecoveryTracker::new(now, 0, 0);

        // Connected but no video yet; an old timestamp must not trigger the watchdog.
        assert_eq!(
            tracker.poll(now + VIDEO_STALL_TIMEOUT * 2, 1, 1, 0, Some(now)),
            None
        );
        assert_eq!(
            tracker.poll(now + VIDEO_STALL_TIMEOUT * 2, 1, 1, 0, None),
            None
        );
    }

    #[test]
    fn resync_frame_message_contains_type_and_layer() {
        assert_eq!(encode_resync_frame(7), [0, 0, 0, 101, 0, 0, 0, 7]);
    }

    #[test]
    fn app_header_parses_big_endian() {
        let mut payload = Vec::new();
        for value in [7u32, 1, 2, 0, 1, 2] {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        let header = parse_app_header(&payload).expect("header");
        assert_eq!(header.app_id, 7);
        assert_eq!(header.datagram_id, 1);
        assert_eq!(header.message_id, 2);
    }

    #[test]
    fn xrsp_packet_round_trip_header() {
        let packet = make_xrsp_packet(b"abcde", 42);
        assert_eq!(u16::from_le_bytes([packet[0], packet[1]]), 0x0210);
        assert_eq!(u16::from_le_bytes([packet[4], packet[5]]), 42);
        assert_eq!(packet.len() % 4, 0);
        assert_eq!(&packet[8..13], b"abcde");
    }

    #[test]
    fn partials_reassemble_body_after_header_packet() {
        let mut header_payload = Vec::new();
        for value in [7u32, 1, 1, 0, 1, 2] {
            header_payload.extend_from_slice(&value.to_be_bytes());
        }
        let partials = vec![
            XrspPacket {
                word0: 0x0210,
                topic: 0,
                payload: Bytes::from(header_payload),
            },
            XrspPacket {
                word0: 0x0210,
                topic: 0,
                payload: Bytes::from_static(b"hello"),
            },
            XrspPacket {
                word0: 0x0210,
                topic: 0,
                payload: Bytes::from_static(b" world"),
            },
        ];
        let datagram = datagram_from_partials(partials).expect("datagram");
        assert_eq!(&datagram.body[..], b"hello world");
    }

    #[test]
    fn app_fragments_reassemble_large_video_message() {
        let first = AppDatagram {
            header: AppHeader {
                app_id: 7,
                datagram_id: 10,
                message_id: 20,
                partial_index: 0,
                partial_count: 2,
                qos: 2,
            },
            body: Bytes::from_static(b"\x00\x00\x00\x64video-prefix"),
            received_at: Instant::now(),
        };
        let second = AppDatagram {
            header: AppHeader {
                app_id: 7,
                datagram_id: 11,
                message_id: 20,
                partial_index: 1,
                partial_count: 2,
                qos: 2,
            },
            body: Bytes::from_static(b"-continuation"),
            received_at: Instant::now(),
        };
        let mut pending = None;

        assert!(reassemble_app_fragment(first, &mut pending).is_none());
        let datagram =
            reassemble_app_fragment(second, &mut pending).expect("complete app datagram");

        assert_eq!(datagram.header.message_id, 20);
        assert_eq!(
            &datagram.body[..],
            b"\x00\x00\x00\x64video-prefix-continuation"
        );
    }

    #[test]
    fn finds_h264_nal_types() {
        let payload = b"\x00\x00\x00\x01\x67abc\x00\x00\x01\x65def";
        assert_eq!(h264_nal_types(payload), vec![7, 5]);
    }

    #[test]
    fn swaps_pcm_s16be_to_s16le() {
        assert_eq!(
            pcm_s16be_to_s16le(&[0x12, 0x34, 0xab, 0xcd]),
            vec![0x34, 0x12, 0xcd, 0xab]
        );
        assert_eq!(pcm_s16be_to_s16le(&[0x12]), Vec::<u8>::new());
    }

    #[test]
    fn fixed_fps_policy_matches_plan() {
        assert_eq!(effective_fixed_fps(0), 30);
        assert_eq!(effective_fixed_fps(45), 45);
        assert_eq!(effective_fixed_fps(90), 60);
    }
}
