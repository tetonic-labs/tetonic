use crate::{TraceContext, TraceEvent};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

lazy_static! {
    static ref SECRET_REGEX: Regex =
        Regex::new(r"(?i)(AKIA-[A-Z0-9\-]+|password\s*=\s*\S+)").unwrap();
}

#[derive(Clone, Copy)]
pub enum DiagnosticMode {
    Safe,
    UnsafeRawPayloads,
}

pub struct TraceFormatter {
    mode: DiagnosticMode,
}

impl TraceFormatter {
    pub fn new(mode: DiagnosticMode) -> Self {
        Self { mode }
    }

    pub fn redact(input: &str) -> String {
        let scanned = lokai_secrets::redact_text_sync_lossy(lokai_secrets::shared_scanner(), input);
        SECRET_REGEX
            .replace_all(&scanned, "[REDACTED_SECRET]")
            .to_string()
    }
}

struct FieldExtractor<'a> {
    mode: &'a DiagnosticMode,
    fields: HashMap<String, String>,
    metrics: HashMap<String, f64>,
}

impl<'a> Visit for FieldExtractor<'a> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if value.is_finite() && matches!(field.name(), "duration_ms" | "store_wait_ms") {
            self.metrics.insert(field.name().into(), value);
            self.fields.insert(field.name().into(), value.to_string());
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if matches!(field.name(), "duration_ms" | "store_wait_ms") {
            self.record_f64(field, value as f64);
        } else {
            self.record_debug(field, &value);
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();

        if matches!(self.mode, DiagnosticMode::Safe)
            && (name == "prompt"
                || name == "source"
                || name == "model_output"
                || name == "process_output"
                || name == "environment")
        {
            self.fields
                .insert(name.to_string(), "[DROPPED_PAYLOAD]".to_string());
            return;
        }

        let value_str = format!("{:?}", value);
        self.fields
            .insert(name.to_string(), TraceFormatter::redact(&value_str));
    }
}

impl<S, N> FormatEvent<S, N> for TraceFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut extractor = FieldExtractor {
            mode: &self.mode,
            fields: HashMap::new(),
            metrics: HashMap::new(),
        };
        event.record(&mut extractor);

        let mut trace_event = TraceEvent {
            event_name: event.metadata().name().to_string(),
            component_name: event.metadata().target().to_string(),
            ..TraceEvent::default()
        };

        // Extract context by walking up the span hierarchy
        if let Some(span) = ctx.lookup_current() {
            trace_event.operation_name = span.name().to_string();
            for current_span in span.scope() {
                if let Some(trace_ctx) = current_span.extensions().get::<TraceContext>() {
                    trace_event.correlation_identifiers = trace_ctx.clone();
                    break;
                }
            }
        }

        // Push fields as metrics or unstructured data (for M0-3 we just stringify them)
        // In a real implementation we'd map these to structured types
        let mut msg = String::new();
        for (k, v) in extractor.fields {
            msg.push_str(&format!("{}={} ", k, v));
        }
        trace_event.outcome = msg.trim().to_string();
        trace_event.duration_ms = extractor
            .metrics
            .get("duration_ms")
            .map(|ms| ms.max(0.0) as u64);
        trace_event.metrics = extractor.metrics;

        let json = serde_json::to_string(&trace_event).map_err(|_| fmt::Error)?;
        let class = crate::sampling::retention_for_outcome(&trace_event.outcome);
        let allow = crate::storage::global_trace_gate().should_emit(
            &trace_event.outcome,
            class,
            json.len() as u64,
        );
        if !allow {
            return Ok(());
        }
        writeln!(writer, "{}", json)
    }
}

pub fn init_subscriber(mode: DiagnosticMode) -> impl Subscriber {
    use tracing_subscriber::prelude::*;

    crate::storage::configure_trace_gate(crate::storage::TraceStorageBudget::defaults(), 1.0);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().event_format(TraceFormatter::new(mode)))
}

pub fn init_subscriber_stderr(mode: DiagnosticMode) -> impl Subscriber {
    use tracing_subscriber::prelude::*;

    crate::storage::configure_trace_gate(crate::storage::TraceStorageBudget::defaults(), 1.0);
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .event_format(TraceFormatter::new(mode)),
    )
}

pub fn init_subscriber_cli(
    mode: DiagnosticMode,
    log_dir: impl AsRef<std::path::Path>,
    interactive: bool,
) -> (impl Subscriber, tracing_appender::non_blocking::WorkerGuard) {
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::prelude::*;

    crate::storage::configure_trace_gate(crate::storage::TraceStorageBudget::defaults(), 1.0);
    let file_appender = tracing_appender::rolling::daily(log_dir.as_ref(), "telemetry.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let json_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .event_format(TraceFormatter::new(mode));

    let stderr_layer = if interactive {
        None
    } else {
        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .without_time()
                .with_filter(
                    tracing_subscriber::EnvFilter::builder()
                        .with_default_directive(LevelFilter::WARN.into())
                        .with_env_var("LOKAI_LOG")
                        .from_env_lossy(),
                ),
        )
    };

    let subscriber = tracing_subscriber::registry()
        .with(json_layer)
        .with(stderr_layer);

    (subscriber, guard)
}
