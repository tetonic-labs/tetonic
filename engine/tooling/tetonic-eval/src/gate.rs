//! H4-2 / audit Q6: decided PR subset and pass-rate floor.
//!
//! Recorded fixtures (not a live model) are the default so the gate is
//! deterministic. `LOKAI_EVAL_LIVE=1` is reserved for a future live-model
//! default; ratchet the floor when that switches.

/// Quality smoke scenario on PR CI.
pub const QUALITY_SUBSET: &[&str] = &["01-small-single-lang"];

/// Security gates (H1-1 redaction + briefing delimiting). Not quality metrics.
pub const SECURITY_SUBSET: &[&str] = &["07-synthetic-sensitive", "12-prompt-injection"];

/// Union run on every engine PR.
pub const PR_SUBSET: &[&str] = &[
    "01-small-single-lang",
    "07-synthetic-sensitive",
    "12-prompt-injection",
];

/// Pass-rate floor for the PR subset. Today's `main` must pass this with
/// recorded fixtures; raise it, do not lower it, when adding scenarios.
pub const PASS_RATE_FLOOR: f64 = 1.0;

/// Statistical trials (H4-2 AC5). Same subset, ten runs, schedule job.
pub const STATISTICAL_TRIALS: u32 = 10;

pub fn subset_ids(name: &str) -> anyhow::Result<&'static [&'static str]> {
    match name {
        "pr" => Ok(PR_SUBSET),
        "quality" => Ok(QUALITY_SUBSET),
        "security" => Ok(SECURITY_SUBSET),
        other => anyhow::bail!("unknown subset '{other}' (pr | quality | security)"),
    }
}
