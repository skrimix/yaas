//! Starts a casting session and serves its Matroska stream over HTTP.
//!
//! Usage: `cargo run -p magic-cast --example http_stream -- <serial> [fps]`
//!
//! Requires a running ADB server.
//!
//! Play the printed URL with mpv or any other Matroska-capable player. Ctrl+C stops casting.

use magic_cast::{CastingSession, SessionConfig, SessionEvent};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let serial = args.next();
    let fps = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create ADB runtime");
    let device = runtime
        .block_on(forensic_adb::Host::default().device_or_default(serial.as_ref()))
        .expect("select ADB device");
    let config = SessionConfig {
        device,
        fps,
        width: 1800,
        height: 1920,
        audio: true,
        xrsp_port: 4445,
        http_port: 0,
    };

    let (session, events) = CastingSession::start(config).expect("start casting session");
    println!("stream URL: {}", session.url());

    let event_thread = std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            match event {
                SessionEvent::Ended(error) => {
                    println!("session ended (error: {error:?})");
                    break;
                }
                other => println!("session event: {other:?}"),
            }
        }
    });

    event_thread.join().expect("join event thread");
    session.stop();
}
