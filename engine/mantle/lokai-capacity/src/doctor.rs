//! Capacity doctor — compare live placement vs saved profile.

use crate::detect::{compute_fingerprint, hardware_summary};
use crate::profile::{observed_gpu_pct, HardwareSnapshot, RuntimeProfile};
use crate::status::{CapacityDiagnosis, CapacityDoctorStatus, DiagnosisCode};

pub fn is_fingerprint_stale(profile: &RuntimeProfile, live: &HardwareSnapshot) -> bool {
    let live_fp = compute_fingerprint(&live.gpus, live.cpu_cores, live.ram_mb, &live.os);
    profile.hardware.fingerprint != live_fp
}

pub fn diagnose(
    profile: Option<&RuntimeProfile>,
    live_hw: &HardwareSnapshot,
    live_obs: Option<&crate::profile::ObservedPlacement>,
) -> CapacityDiagnosis {
    let Some(profile) = profile else {
        return CapacityDiagnosis {
            status: CapacityDoctorStatus::NoProfile,
            codes: vec![DiagnosisCode::NoProfile],
            summary: "No active capacity profile — run `lokai estate capacity profiles import` or optimize.".into(),
            recommendations: vec![
                "Import a profile JSON or run ES5-2 optimize when available.".into(),
            ],
        };
    };

    let mut codes = Vec::new();
    let mut recommendations = Vec::new();

    if is_fingerprint_stale(profile, live_hw) {
        codes.push(DiagnosisCode::FingerprintMismatch);
        recommendations.push("Hardware fingerprint changed — consider re-optimize (ES5-2).".into());
    }

    if !profile.gates_passed {
        codes.push(DiagnosisCode::GatesFailed);
        recommendations.push("Active profile did not pass gates at creation.".into());
    }

    if let Some(live) = live_obs {
        if let Some(live_pct) = observed_gpu_pct(live) {
            if live_pct < 100.0 {
                codes.push(DiagnosisCode::DegradedPlacement);
                recommendations.push(format!(
                    "Model is CPU-offloaded ({live_pct:.1}% on GPU). Release the spilled allocation and retry a fully GPU-resident configuration."
                ));
            }
        }
        if !lokai_inference::ollama_model_matches(
            &live.resident_model,
            &profile.recipe.estate_model,
        ) {
            codes.push(DiagnosisCode::ResidentModelMismatch);
            recommendations.push(format!(
                "Expected resident `{}`, live `{}`.",
                profile.recipe.estate_model, live.resident_model
            ));
        }
    }

    let status = if codes.iter().any(|c| {
        matches!(
            c,
            DiagnosisCode::DegradedPlacement
                | DiagnosisCode::ResidentModelMismatch
                | DiagnosisCode::GatesFailed
                | DiagnosisCode::SlowRawShort
                | DiagnosisCode::FingerprintMismatch
        )
    }) {
        CapacityDoctorStatus::Degraded
    } else if live_obs.and_then(observed_gpu_pct).is_none() {
        CapacityDoctorStatus::Unknown
    } else {
        CapacityDoctorStatus::Healthy
    };

    let summary = match status {
        CapacityDoctorStatus::Healthy => format!(
            "Profile `{}` healthy — {}",
            profile.label,
            hardware_summary(live_hw)
        ),
        CapacityDoctorStatus::Degraded => format!(
            "Profile `{}` degraded — {}",
            profile.label,
            recommendations
                .first()
                .cloned()
                .unwrap_or_else(|| "check inference placement".into())
        ),
        CapacityDoctorStatus::Unknown => format!(
            "Profile `{}` — could not read live Ollama placement",
            profile.label
        ),
        CapacityDoctorStatus::NoProfile => unreachable!(),
    };

    CapacityDiagnosis {
        status,
        codes,
        summary,
        recommendations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::profile::{
        GpuInfo, GpuRole, HardwareSnapshot, InferenceRecipe, ObservedPlacement, ProfileMetrics,
        ProfileSource, TierRole, SCHEMA_VERSION,
    };

    fn profile_with_gpu(saved_pct: f32, live_pct: f32) -> (RuntimeProfile, ObservedPlacement) {
        let profile = RuntimeProfile {
            schema_version: SCHEMA_VERSION,
            id: "p1".into(),
            label: "test".into(),
            created_at: Utc::now(),
            node_id: "local".into(),
            role: TierRole::Coder,
            source: ProfileSource::Import,
            hardware: HardwareSnapshot {
                fingerprint: "fp".into(),
                gpus: vec![],
                cpu_cores: 8,
                ram_mb: 32_768,
                os: "windows-x86_64".into(),
                ollama_version: None,
            },
            recipe: InferenceRecipe {
                base_model: "qwen3.6:latest".into(),
                estate_model: "qwen3.6-estate".into(),
                num_ctx: 4096,
                num_gpu: None,
                keep_alive: "30m".into(),
                modelfile_path: None,
                env_hints: vec![],
                draft_model: None,
                draft_count: None,
            },
            observed: ObservedPlacement {
                gpu_processor_pct: Some(saved_pct),
                processor_split: None,
                vram_used_mb: 18_000,
                resident_model: "qwen3.6-estate".into(),
                load_wall_s: 0.0,
                measured_at: Utc::now(),
            },
            metrics: ProfileMetrics::default(),
            gates_passed: true,
        };
        let live = ObservedPlacement {
            gpu_processor_pct: Some(live_pct),
            processor_split: None,
            vram_used_mb: 1400,
            resident_model: "qwen3.6:latest".into(),
            load_wall_s: 0.0,
            measured_at: Utc::now(),
        };
        (profile, live)
    }

    #[test]
    fn detects_degraded_gpu() {
        let (p, live) = profile_with_gpu(85.0, 24.0);
        let hw = HardwareSnapshot {
            fingerprint: "fp".into(),
            gpus: vec![GpuInfo {
                name: "P40".into(),
                uuid: None,
                bus_id: None,
                vram_total_mb: 24_576,
                role: GpuRole::Compute,
            }],
            cpu_cores: 8,
            ram_mb: 32_768,
            os: "windows-x86_64".into(),
            ollama_version: None,
        };
        let d = diagnose(Some(&p), &hw, Some(&live));
        assert_eq!(d.status, CapacityDoctorStatus::Degraded);
        assert!(d.codes.contains(&DiagnosisCode::DegradedPlacement));
    }

    #[test]
    fn different_model_or_missing_measurement_cannot_be_healthy() {
        let (mut profile, mut live) = profile_with_gpu(100.0, 100.0);
        profile.hardware.fingerprint = compute_fingerprint(
            &profile.hardware.gpus,
            profile.hardware.cpu_cores,
            profile.hardware.ram_mb,
            &profile.hardware.os,
        );
        let diagnosis = diagnose(Some(&profile), &profile.hardware, Some(&live));
        assert_eq!(diagnosis.status, CapacityDoctorStatus::Degraded);
        assert!(diagnosis
            .codes
            .contains(&DiagnosisCode::ResidentModelMismatch));
        live.resident_model = format!("{}:latest", profile.recipe.estate_model);
        assert_eq!(
            diagnose(Some(&profile), &profile.hardware, Some(&live)).status,
            CapacityDoctorStatus::Healthy
        );
        live.gpu_processor_pct = Some(96.0);
        assert_eq!(
            diagnose(Some(&profile), &profile.hardware, Some(&live)).status,
            CapacityDoctorStatus::Degraded
        );
        live.gpu_processor_pct = None;
        assert_eq!(
            diagnose(Some(&profile), &profile.hardware, Some(&live)).status,
            CapacityDoctorStatus::Unknown
        );
    }
}
