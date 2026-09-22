//! Human-readable capacity status for CLI and TUI.

use crate::model_fit::session_matches_profile;
use crate::status::{CapacityDiagnosis, CapacityDoctorStatus, CapacityStatus};

/// Session-aware doctor / capacity text. `session_model` is this chat's `--model`.
pub fn format_capacity_report(
    status: &CapacityStatus,
    diagnosis: Option<&CapacityDiagnosis>,
    session_model: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(session) = session_model.filter(|s| !s.is_empty()) {
        out.push_str(&format!("This chat:        {session}\n"));
        match (
            status.profile_model.as_deref(),
            status.profile_base_model.as_deref(),
        ) {
            (Some(estate), base) => {
                let same = session_matches_profile(session, Some(estate), base);
                if same {
                    out.push_str(&format!("Profile default:  {estate}  (same model)\n"));
                } else {
                    out.push_str(&format!("Profile default:  {estate}\n"));
                }
            }
            _ => out.push_str("Profile default:  (none)\n"),
        }
    } else if let Some(estate) = status.profile_model.as_deref() {
        out.push_str(&format!("Profile model:    {estate}\n"));
    }

    out.push_str("\nCapacity profile:\n");
    out.push_str(&format!("  completed: {}\n", status.completed));
    out.push_str(&format!("  doctor:    {:?}\n", status.doctor));
    out.push_str(&format!("  stale:     {}\n", status.stale));
    out.push_str(&format!("  gates_ok:  {}\n", status.gates_ok));
    if let Some(ref id) = status.active_profile_id {
        out.push_str(&format!("  profile:   {id}\n"));
    }
    if let Some(ref label) = status.active_profile_label {
        out.push_str(&format!("  label:     {label}\n"));
    }
    if let Some(ref hw) = status.hardware_summary {
        out.push_str(&format!("  hardware:  {hw}\n"));
    }
    if let Some(ref t) = status.last_setup_at {
        out.push_str(&format!("  since:     {t}\n"));
    }

    if let Some(d) = diagnosis {
        out.push('\n');
        out.push_str(&d.summary);
        out.push('\n');
        for rec in &d.recommendations {
            out.push_str(&format!("  → {rec}\n"));
        }
    }

    if let Some(session) = session_model.filter(|s| !s.is_empty()) {
        let same = session_matches_profile(
            session,
            status.profile_model.as_deref(),
            status.profile_base_model.as_deref(),
        );
        if !same && status.profile_model.is_some() {
            out.push_str(
                "\nThis session is not using the saved profile model.\n\
                 `--model` already selected this chat — you do not need `/optimize` to switch.\n\
                 The profile is only the default when a session omits `--model`.\n",
            );
            if matches!(
                status.doctor,
                CapacityDoctorStatus::Degraded | CapacityDoctorStatus::Unknown
            ) {
                out.push_str("Doctor is describing the *profile default*, not this chat.\n");
            }
        } else if !status.gates_ok && same {
            out.push_str(
                "\nThe saved profile for this model failed capacity gates.\n\
                 Chat will still try; inference aborts only if this model spills out of VRAM.\n",
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{CapacityDiagnosis, DiagnosisCode};

    #[test]
    fn override_explains_that_doctor_is_about_the_profile() {
        let status = CapacityStatus {
            completed: true,
            stale: false,
            doctor: CapacityDoctorStatus::Degraded,
            active_profile_id: Some("p1".into()),
            active_profile_label: Some("lab".into()),
            hardware_summary: Some("GPU".into()),
            gates_ok: false,
            last_setup_at: None,
            profile_model: Some("qwen3.6-estate".into()),
            profile_base_model: Some("qwen3.6:latest".into()),
        };
        let diagnosis = CapacityDiagnosis {
            status: CapacityDoctorStatus::Degraded,
            codes: vec![DiagnosisCode::GatesFailed],
            summary: "Profile `lab` degraded — too large".into(),
            recommendations: vec!["re-optimize".into()],
        };
        let text = format_capacity_report(&status, Some(&diagnosis), Some("qwen3.5:latest"));
        assert!(text.contains("This chat:        qwen3.5:latest"));
        assert!(text.contains("qwen3.6-estate"));
        assert!(text.contains("not using the saved profile"));
        assert!(text.contains("Doctor is describing the *profile default*"));
        assert!(text.contains("you do not need `/optimize` to switch"));
    }
}
