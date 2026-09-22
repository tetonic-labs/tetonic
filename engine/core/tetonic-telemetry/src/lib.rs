pub mod context;
pub mod event;
pub mod fault;
pub mod performance;
pub mod propagation;
pub mod sampling;
pub mod sanitization;
pub mod spans;
pub mod storage;
pub mod timing;
pub use performance::{PerfStage, StageTimer};

pub use context::TraceContext;
pub use event::TraceEvent;
pub use propagation::{
    enter_stage_child, extract_context, inject_context, inject_session_context, inject_turn_context,
};
pub use sampling::{retention_for_outcome, RetentionClass, TraceSampler};
pub use sanitization::{
    init_subscriber, init_subscriber_cli, init_subscriber_stderr, DiagnosticMode,
};
pub use spans::{
    attrs as span_attrs, emit_safe_metric, names as span_names, record_compute_stage,
    record_retained_outcome, record_stage_segments, record_store_wait,
};
pub use storage::{
    admit_trace_write, configure_trace_gate, global_trace_gate, is_safe_metric_label,
    TraceStorageBudget, TraceStorageError, TraceWriteGate, SAFE_METRIC_LABELS,
};
pub use timing::{
    critical_path_ms, format_critical_path_report, saturating_ms_between,
    CoordinatorObservedTiming, CriticalPathReport, StageSegments, TransferDurations,
    WorkerLocalDurations,
};
