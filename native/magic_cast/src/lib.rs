//! Meta Quest casting ("Magic Cast") sessions.
//!
//! [`CastingSession`] handles XRSP setup and recovery, paces incoming media, and serves a live
//! Matroska stream to one HTTP client.

pub mod cadence;
pub mod matroska;
mod session;
mod stream;

pub use session::SessionEvent;
pub use stream::{CastingSession, SessionConfig};
