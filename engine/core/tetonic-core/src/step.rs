use serde_json::Value;
use tetonic_inference::GenUsage;

use crate::context::ContextReport;

// Observable step events emitted by the agent loop.

/// Observable events emitted as the loop runs (the CLI prints these; later the
/// RPC layer streams them to the editor).
#[derive(Debug, Clone)]
pub enum Step {
    Context(ContextReport),
    /// A streamed content delta from the model (live assistant output).
    Token(String),
    /// A streamed reasoning/thought delta from the model (e.g. from <think> tags).
    Thought(String),
    /// An out-of-band agent notice (e.g. compaction occurred).
    Note(String),
    /// Generation accounting for the step just completed (prefill/decode tokens
    /// and throughput), when the runtime reports it.
    Generation(GenUsage),
    ToolCall {
        call_id: String,
        name: String,
        args: Value,
    },
    ToolResult {
        call_id: String,
        name: String,
        ok: bool,
        summary: String,
    },
    /// User-visible answer prose (explain turns or `finish` summary).
    Answer(String),
    Stopped(String),
}

/// Render generation accounting as a one-line audit note, e.g.
/// `gen: prefill 1840 tok @ 612 tok/s · decode 95 tok @ 41 tok/s`.
pub(crate) fn format_usage(u: &GenUsage) -> String {
    let fmt = |tok: Option<u64>, tps: Option<f64>| match (tok, tps) {
        (Some(t), Some(s)) => format!("{t} tok @ {s:.0} tok/s"),
        (Some(t), None) => format!("{t} tok"),
        _ => "n/a".to_string(),
    };
    format!(
        "gen: prefill {} · decode {}",
        fmt(u.prompt_tokens, u.prefill_tps()),
        fmt(u.eval_tokens, u.decode_tps()),
    )
}
