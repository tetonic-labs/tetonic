/// Global fault injection hook for testing crash recovery behavior.
///
/// Set `LOKAI_FAULT_INJECT=<point>` to terminate the process with exit code 1
/// when production code reaches that named point. Lokai-eval R06 uses
/// `after_turn_plan` (see `lokai_eval::recovery::INJECT_AFTER_TURN_PLAN`).
pub fn inject_fault(point: &str) {
    if let Ok(val) = std::env::var("LOKAI_FAULT_INJECT") {
        if val == point {
            tracing::error!("FAULT INJECTED at {}. Terminating process.", point);
            std::process::exit(1);
        }
    }
}
