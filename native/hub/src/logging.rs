use std::{
    collections::BTreeMap,
    iter,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rinf::{DartSignal, RustSignal};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    time,
};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::models::signals::logging::{
    GetLogsDirectoryRequest, GetLogsDirectoryResponse, LogBatch, LogEntry, LogKind, LogLevel,
    SpanInfo, SpanTrace,
};

/// Cached span field information stored in span extensions
#[derive(Clone, Debug)]
struct CachedSpanFields {
    /// Stable ID assigned by this layer
    id: String,
    /// Parameters captured from the span fields
    parameters: BTreeMap<String, String>,
}

/// A custom tracing layer that forwards log events to Flutter via Rinf signals
pub(crate) struct SignalLayer {
    sender: Sender<LogEntry>,
    /// Counter for assigning stable span IDs
    span_id_counter: AtomicU64,
}

impl SignalLayer {
    /// Get current timestamp in milliseconds since Unix epoch
    fn current_timestamp_ms() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
    }

    pub(crate) fn new() -> (Self, Receiver<LogEntry>) {
        // Use bounded channel with capacity of 1000 entries
        let (sender, receiver) = mpsc::channel(1000);
        let layer = Self { sender, span_id_counter: AtomicU64::new(1) };
        (layer, receiver)
    }

    /// Start the background task that batches and sends log entries to Flutter
    pub(crate) fn start_forwarder(mut receiver: Receiver<LogEntry>) {
        tokio::spawn(async move {
            let mut buffer = Vec::new();
            let mut interval = time::interval(Duration::from_millis(100));

            loop {
                tokio::select! {
                    maybe_entry = receiver.recv() => {
                        match maybe_entry {
                            Some(entry) => {
                                buffer.push(entry);

                                // Send immediately for errors, or when buffer is getting full
                                if buffer.len() >= 10 || buffer.last().map(|e| matches!(e.level, LogLevel::Error)).unwrap_or(false) {
                                    Self::flush_buffer(&mut buffer).await;
                                }
                            }
                            None => {
                                panic!("Log entry channel closed unexpectedly");
                            }
                        }
                    }

                    // Periodic flush
                    _ = interval.tick() => {
                        if !buffer.is_empty() {
                            Self::flush_buffer(&mut buffer).await;
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn start_request_handler(logs_dir: PathBuf) {
        tokio::spawn(async move {
            let directory_receiver = GetLogsDirectoryRequest::get_dart_signal_receiver();

            while directory_receiver.recv().await.is_some() {
                let logs_path = logs_dir.to_string_lossy().to_string();
                GetLogsDirectoryResponse { path: logs_path }.send_signal_to_dart();
            }
            panic!("GetLogsDirectoryRequest receiver closed");
        });
    }

    async fn flush_buffer(buffer: &mut Vec<LogEntry>) {
        if !buffer.is_empty() {
            LogBatch { entries: std::mem::take(buffer) }.send_signal_to_dart();
        }
    }
}

impl SignalLayer {
    /// Build a span trace from the current context
    fn build_span_trace<S>(&self, ctx: &Context<'_, S>) -> Option<SpanTrace>
    where
        S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    {
        if let Some(span_id) = ctx.current_span().id() {
            if let Some(scope) = ctx.span_scope(span_id) {
                let mut spans = Vec::new();

                // Walk through the span scope
                for span in scope.from_root() {
                    let name = span.name().to_string();
                    let target = span.metadata().target().to_string();

                    // Get cached parameters
                    let (id, parameters) =
                        if let Some(cached) = span.extensions().get::<CachedSpanFields>() {
                            let params = if cached.parameters.is_empty() {
                                None
                            } else {
                                Some(cached.parameters.clone())
                            };
                            (cached.id.clone(), params)
                        } else {
                            // Fallback if extensions missing
                            (format!("{:?}", span.id()), None)
                        };

                    spans.push(SpanInfo { id, name, target, parameters });
                }

                if !spans.is_empty() { Some(SpanTrace { spans }) } else { None }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Build a span trace from the current event context  
    fn build_span_trace_for_event<S>(
        &self,
        ctx: &Context<'_, S>,
        event: &Event<'_>,
    ) -> Option<SpanTrace>
    where
        S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    {
        if let Some(scope) = ctx.event_scope(event) {
            let mut spans = Vec::new();

            for span in scope.from_root() {
                let name = span.name().to_string();
                let target = span.metadata().target().to_string();

                // Get cached id + parameters
                let (id, parameters) =
                    if let Some(cached) = span.extensions().get::<CachedSpanFields>() {
                        let params = if cached.parameters.is_empty() {
                            None
                        } else {
                            Some(cached.parameters.clone())
                        };
                        (cached.id.clone(), params)
                    } else {
                        (format!("{:?}", span.id()), None)
                    };

                spans.push(SpanInfo { id, name, target, parameters });
            }

            if !spans.is_empty() { Some(SpanTrace { spans }) } else { None }
        } else {
            None
        }
    }
}

impl<S> Layer<S> for SignalLayer
where
    S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        let mut fields = visitor.fields;

        // Add location information if available
        if let (Some(file), Some(line)) = (event.metadata().file(), event.metadata().line()) {
            fields.insert("location".to_string(), format!("{file}:{line}"));
        }

        // Collect span information
        let span_trace = self.build_span_trace_for_event(&ctx, event);

        let entry = LogEntry {
            timestamp: Self::current_timestamp_ms(),
            level: LogLevel::from(*event.metadata().level()),
            target: event.metadata().target().to_string(),
            message: visitor.message.unwrap_or_default(),
            kind: LogKind::Event,
            fields: if fields.is_empty() { None } else { Some(fields) },
            span_trace,
        };

        // Send to the forwarder (drop entry if channel is full to prevent backpressure)
        let _ = self.sender.try_send(entry);
    }

    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id) {
            // Skip if span fields are already cached
            if span.extensions().get::<CachedSpanFields>().is_some() {
                return;
            }

            // Extract and cache span attributes
            let mut visitor = FieldVisitor::new();
            attrs.record(&mut visitor);

            // Assign a stable ID and cache fields for this span
            let next_id = self.span_id_counter.fetch_add(1, Ordering::Relaxed);
            let span_id_str = format!("{:016x}", next_id);
            let cached_fields =
                CachedSpanFields { id: span_id_str, parameters: visitor.fields.clone() };
            span.extensions_mut().insert(cached_fields);

            let mut fields = BTreeMap::new();
            if let (Some(file), Some(line)) = (attrs.metadata().file(), attrs.metadata().line()) {
                fields.insert("location".to_string(), format!("{file}:{line}"));
            }

            // Include span parameters in fields for richer UI display
            fields.extend(visitor.fields.clone());

            // Create log entry for span creation event
            let entry = LogEntry {
                timestamp: Self::current_timestamp_ms(),
                level: LogLevel::from(*attrs.metadata().level()),
                target: attrs.metadata().target().to_string(),
                message: format!(
                    "span::new {}::{}",
                    attrs.metadata().target(),
                    attrs.metadata().name()
                ),
                kind: LogKind::SpanNew,
                fields: if fields.is_empty() { None } else { Some(fields) },
                span_trace: self.build_span_trace(&ctx),
            };

            // Send to the forwarder (drop entry if channel is full to prevent backpressure)
            let _ = self.sender.try_send(entry);
        }
    }

    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id) {
            // Create log entry for span close event
            let entry = LogEntry {
                timestamp: Self::current_timestamp_ms(),
                level: LogLevel::from(*span.metadata().level()),
                target: span.metadata().target().to_string(),
                message: format!("span::close {}::{}", span.metadata().target(), span.name()),
                kind: LogKind::SpanClose,
                fields: None,
                span_trace: self.build_span_trace(&ctx),
            };

            // Send to the forwarder (drop entry if channel is full to prevent backpressure)
            let _ = self.sender.try_send(entry);
        }
    }
}

/// Visitor to extract fields from tracing events
struct FieldVisitor {
    message: Option<String>,
    fields: BTreeMap<String, String>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self { message: None, fields: BTreeMap::new() }
    }
}

impl tracing::field::Visit for FieldVisitor {
    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        let parts: Vec<String> =
            iter::successors(Some(value), |e| e.source()).map(ToString::to_string).collect();

        let top = parts.first().cloned().unwrap_or_default();
        self.fields.insert(field.name().to_string(), top);

        self.fields.insert(
            "error_chain".to_string(),
            parts.iter().rev().cloned().collect::<Vec<_>>().join(": "),
        );
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        } else {
            self.fields.insert(field.name().to_string(), format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(field.name().to_string(), value.to_string());
        }
    }
}
