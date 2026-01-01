use std::collections::BTreeMap;

use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};
use tracing::Level;

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) enum LogKind {
    Event,
    SpanNew,
    SpanClose,
}

impl From<Level> for LogLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::TRACE => LogLevel::Trace,
            Level::DEBUG => LogLevel::Debug,
            Level::INFO => LogLevel::Info,
            Level::WARN => LogLevel::Warn,
            Level::ERROR => LogLevel::Error,
        }
    }
}

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => Level::TRACE,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) struct SpanInfo {
    /// Stable identifier assigned by the core for this span instance
    pub id: String,
    pub name: String,
    pub target: String,
    pub parameters: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) struct SpanTrace {
    pub spans: Vec<SpanInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) struct LogEntry {
    pub timestamp: u64, // Milliseconds since Unix epoch
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub kind: LogKind,
    pub fields: Option<BTreeMap<String, String>>,
    pub span_trace: Option<SpanTrace>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct LogBatch {
    pub entries: Vec<LogEntry>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetLogsDirectoryRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct GetLogsDirectoryResponse {
    pub path: String,
}
