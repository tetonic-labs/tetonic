//! Phase + health chrome. Status names state, not mood.

use std::time::Instant;

use crate::tui::failure;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnPhase {
    Idle,
    Stage(lokai_app::events::EngineStage, String),
    Generating,
    WaitingApproval,
    RunningTool(String),
    Verifying,
    Failed(String),
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ready,
    Busy,
    Degraded,
    Recovery,
}

pub const KEY_CHROME: &str =
    "Ctrl+C cancel · Ctrl+Q quit · Ctrl+O thoughts · Ctrl+L activity · Ctrl+T tool details · [/] inspector (when focused)";

pub fn health(
    recovery_chip: bool,
    degraded: bool,
    thinking: bool,
    pending_approval: bool,
) -> Health {
    if recovery_chip {
        Health::Recovery
    } else if degraded {
        Health::Degraded
    } else if thinking || pending_approval {
        Health::Busy
    } else {
        Health::Ready
    }
}

pub fn health_label(h: Health) -> &'static str {
    match h {
        Health::Ready => "Ready",
        Health::Busy => "Busy",
        Health::Degraded => "Degraded",
        Health::Recovery => "Recovery",
    }
}

pub fn display_workspace(path: &str) -> String {
    let path = path
        .strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .unwrap_or(path);
    if path.chars().count() <= 48 {
        return path.to_string();
    }
    let clipped: String = path
        .chars()
        .rev()
        .take(40)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("…{clipped}")
}

pub fn resume_banner(resume_state: &str) -> Option<String> {
    match resume_state {
        "recovery_required" | "incomplete" => {
            Some("session resumed · last turn incomplete · send again or /status".into())
        }
        "continued" => Some("session resumed · send a message or /status".into()),
        _ => None,
    }
}

pub fn failure_object(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("gpu spill") {
        "GPU spill".into()
    } else if lower.contains("egress") {
        "network policy".into()
    } else if lower.contains("fabric connect") || lower.contains("no eligible remote") {
        "worker unreachable".into()
    } else if lower.contains("canceled") {
        "canceled".into()
    } else {
        let copy = failure::explain_turn_failure(raw);
        copy.headline
            .split('—')
            .nth(1)
            .unwrap_or("execution error")
            .trim()
            .trim_end_matches('.')
            .to_string()
    }
}

pub fn status_text(
    phase: &TurnPhase,
    thinking: bool,
    turn_start: Option<Instant>,
    hint: Option<&str>,
    resume_idle: Option<&str>,
) -> String {
    if let Some(hint) = hint {
        return format!(" {hint}");
    }
    let elapsed = if thinking {
        turn_start
            .map(|s| format!(" · {:.1}s", s.elapsed().as_secs_f32()))
            .unwrap_or_default()
    } else {
        String::new()
    };
    match phase {
        TurnPhase::Stage(_stage, desc) => format!(" {desc}{elapsed}"),
        TurnPhase::Generating => format!(" Generating{elapsed}"),
        TurnPhase::WaitingApproval => format!(" Waiting for approval{elapsed}"),
        TurnPhase::RunningTool(obj) => format!(" Running {obj}{elapsed}"),
        TurnPhase::Verifying => format!(" Verifying{elapsed}"),
        TurnPhase::Failed(obj) => format!(" Failed — {obj}"),
        TurnPhase::Canceled => " Turn canceled".into(),
        TurnPhase::Idle => {
            if let Some(banner) = resume_idle {
                format!(" {banner}")
            } else {
                " Ready".into()
            }
        }
    }
}

pub fn session_status(
    session_id: &str,
    resume_state: &str,
    model: &str,
    workspace: &str,
    phase: &TurnPhase,
    health: Health,
) -> String {
    let phase_s = match phase {
        TurnPhase::Idle => "idle".into(),
        TurnPhase::Stage(_stage, desc) => format!("stage: {desc}"),
        TurnPhase::Generating => "generating".into(),
        TurnPhase::WaitingApproval => "waiting for approval".into(),
        TurnPhase::RunningTool(obj) => format!("running {obj}"),
        TurnPhase::Verifying => "verifying".into(),
        TurnPhase::Failed(obj) => format!("failed — {obj}"),
        TurnPhase::Canceled => "canceled".into(),
    };
    format!(
        "session     {session_id}\n\
         resume      {resume_state}\n\
         model       {model}\n\
         workspace   {workspace}\n\
         phase       {phase_s}\n\
         health      {}\n",
        health_label(health)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_prefers_recovery_then_degraded() {
        assert_eq!(health(true, true, true, false), Health::Recovery);
        assert_eq!(health(false, true, false, false), Health::Degraded);
        assert_eq!(health(false, false, true, false), Health::Busy);
        assert_eq!(health(false, false, false, true), Health::Busy);
        assert_eq!(health(false, false, false, false), Health::Ready);
    }

    #[test]
    fn status_names_phase_not_vibe() {
        let t = status_text(
            &TurnPhase::Generating,
            true,
            Some(Instant::now()),
            None,
            None,
        );
        assert!(t.contains("Generating"));
        assert!(!t.contains("Pondering"));
        let t = status_text(
            &TurnPhase::RunningTool("cargo test".into()),
            true,
            None,
            None,
            None,
        );
        assert_eq!(t, " Running cargo test");
        let t = status_text(
            &TurnPhase::Failed("GPU spill".into()),
            false,
            None,
            None,
            None,
        );
        assert_eq!(t, " Failed — GPU spill");
    }

    #[test]
    fn resume_banner_covers_recovery_contract() {
        let b = resume_banner("recovery_required").unwrap();
        assert!(b.contains("last turn incomplete"));
        assert!(b.contains("/status"));
        assert!(resume_banner("fresh").is_none());
    }

    #[test]
    fn key_chrome_prints_the_map() {
        assert!(KEY_CHROME.contains("Ctrl+C cancel"));
        assert!(KEY_CHROME.contains("Ctrl+Q quit"));
        assert!(KEY_CHROME.contains("[/] inspector"));
        assert!(!KEY_CHROME.contains("Ctrl+D"));
    }

    #[test]
    fn strips_windows_verbatim_prefix() {
        let p = display_workspace(r"\\?\C:\Users\developer\proj");
        assert!(!p.contains(r"\\?\"));
        assert!(p.contains(r"C:\Users\developer\proj"));
    }
}
