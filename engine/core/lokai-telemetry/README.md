# Lokai Telemetry & Trace Correlation

`lokai-telemetry` provides foundational tracing, privacy preservation, and cross-boundary correlation for the entire Lokai execution path.

## Status

**M0-3 foundational tracing** remains the base. **M6-3 Complete** — coordinator-observed
timing, worker-local queue/execute overlay, verification Instant on result accept,
stage segments, sampling always-retain, bounded storage vs recovery reservation,
`emit_safe_metric` cardinality gate, and local critical-path reports are
production-wired.

See epic sprint `M6-3-distributed-tracing.md`.

## TraceContext Propagation

`lokai_telemetry::TraceContext` is the local correlation carrier for this crate
(inject/extract, child spans, TLS fallback). Business ids (`session_id`, `run_id`,
`task_id`, …) are **strings** holding live Lokai ids — not fabricated UUIDs (R02).

`inject_session_context` / `inject_turn_context` are called from `lokai-app`
session start and `execute_turn` (shared CLI + daemon path).

Broker/fabric also propagate `lokai_domain::TraceContext` / `FabricTraceContext`
with `scheduler_decision_id` for Infer correlation.

## Compute spans (M6-3)

Use `lokai_telemetry::span_names` / `record_compute_stage` /
`record_stage_segments` / `record_retained_outcome` for broker and fabric
stages. Attributes are bounded (job kind, outcomes, target type, durations) —
never prompts, source, secrets, or raw model output.

Key types:

* `CoordinatorObservedTiming` / `critical_path_ms` — Instant-only e2e
* `WorkerLocalDurations` / `TransferDurations` / `StageSegments`
* `TraceSampler` / `RetentionClass::AlwaysRetain`
* `TraceWriteGate` / `admit_trace_write` / `TraceStorageBudget`

## Event Schema

Events are serialized into local `.log` files via the `TraceEvent` schema. The
formatter consults `TraceWriteGate` (quota + recovery reservation + sampling)
before persistence.

## Privacy Rules (Sanitization)

1. **Raw Payloads Disabled**: Fields named `prompt`, `source`, `model_output`, `process_output`, or `environment` are forcibly replaced with `[DROPPED_PAYLOAD]` unless the user enables `DiagnosticMode::UnsafeRawPayloads`.
2. **Sentinel Secret Scrubbing**: Values run through `ScannerEngine` (`lokai-secrets::shared_scanner`) then a global regex replacing residual AWS keys / passwords with `[REDACTED_SECRET]` (R4-3).

Tracing strictly limits output to local, size-bounded files.

## Tests

```bash
cargo test -p lokai-telemetry
```

Covers timing matrix (missing spans, separate stages, skew saturating, worker
overlay vs e2e), sampling always-retain / high event rate, storage vs recovery
reservation, oversized writes, and safe labels.
