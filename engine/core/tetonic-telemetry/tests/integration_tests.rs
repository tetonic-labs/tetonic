use std::sync::{Arc, Mutex};
use tetonic_telemetry::{sanitization::DiagnosticMode, TraceEvent};
use tracing::{info, subscriber::with_default};
use tracing_subscriber::fmt::MakeWriter;

/// A mock writer that captures tracing bytes into a shared string for assertions.
#[derive(Clone)]
struct MockWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl MockWriter {
    fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn output(&self) -> String {
        let guard = self.buffer.lock().unwrap();
        String::from_utf8(guard.clone()).unwrap()
    }
}

impl std::io::Write for MockWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for MockWriter {
    type Writer = MockWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn performance_timings_are_numeric_and_interrupted_work_is_not_success() {
    use tetonic_telemetry::{PerfStage, StageTimer};
    use tracing_subscriber::prelude::*;
    let writer = MockWriter::new();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(tetonic_telemetry::sanitization::TraceFormatter::new(
                DiagnosticMode::Safe,
            ))
            .with_writer(writer.clone()),
    );
    with_default(subscriber, || {
        StageTimer::start_visible(PerfStage::InferenceHeaders).finish(true);
        drop(StageTimer::start(PerfStage::TaskTurn));
    });
    let output = writer.output();
    let events: Vec<TraceEvent> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert!(events[0].outcome.contains("started"));
    assert_eq!(events[0].metrics["duration_ms"], 0.0);
    assert!(events[1].outcome.contains("succeeded"));
    assert!(events[2].outcome.contains("interrupted"));
    let timing_id = |event: &TraceEvent| {
        event
            .outcome
            .split_whitespace()
            .find(|field| field.starts_with("timing_id="))
            .unwrap()
            .to_string()
    };
    assert_eq!(timing_id(&events[0]), timing_id(&events[1]));
    assert_ne!(timing_id(&events[0]), timing_id(&events[2]));
    assert!(events
        .iter()
        .all(|e| e.metrics["duration_ms"] >= 0.0 && e.duration_ms.is_some()));
}

#[test]
fn test_true_json_redaction_pipeline() {
    let mock_writer = MockWriter::new();

    // We override init_subscriber to use our MockWriter instead of the default stdout
    use tracing_subscriber::prelude::*;
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(tetonic_telemetry::sanitization::TraceFormatter::new(
                DiagnosticMode::Safe,
            ))
            .with_writer(mock_writer.clone()),
    );

    with_default(subscriber, || {
        info!(
            prompt = "You are an agent.",
            source = "fn main() {}",
            process_output = "SECRET_FROM_PROCESS",
            secret = "AKIA-REAL-SECRET-12345",
            message = "Processing agent task."
        );
    });

    let output = mock_writer.output();
    println!("Captured Trace: {}", output);

    // Parse it back to ensure schema compliance
    let event: TraceEvent = serde_json::from_str(&output).expect("Trace output must be valid JSON");

    assert!(
        !output.contains("AKIA-REAL-SECRET-12345"),
        "Raw secret leaked into output!"
    );
    assert!(
        output.contains("[REDACTED_SECRET]"),
        "Secret was not properly redacted."
    );
    assert!(
        !output.contains("You are an agent."),
        "Raw prompt leaked into output!"
    );
    assert!(
        !output.contains("SECRET_FROM_PROCESS"),
        "Raw process output leaked into output!"
    );

    // Ensure the outcome field merged the redacted fields
    assert!(event.outcome.contains("[DROPPED_PAYLOAD]"));
    assert!(event.outcome.contains("[REDACTED_SECRET]"));
}
