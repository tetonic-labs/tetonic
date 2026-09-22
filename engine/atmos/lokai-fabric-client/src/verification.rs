//! Result verification policy engine (M5-4).
//!
//! A valid signature proves provenance only. Acceptance still requires the
//! configured verification policy to succeed.

use std::collections::HashSet;

use lokai_domain::ids::WorkerId;
use lokai_domain::{ResultDisposition, ResultVerificationRequirement};
use lokai_fabric_protocol::{digest_payload, ResultEnvelope, ResultStatus};
use serde::{Deserialize, Serialize};

use crate::FabricClientError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependenceEvidence {
    pub worker_id: WorkerId,
    pub host_id: String,
    pub owner_domain: String,
    pub model_instance: Option<String>,
    pub execution_nonce: String,
    pub shared_cache: bool,
}

#[derive(Clone, Debug)]
pub struct RedundantCandidate {
    pub envelope: ResultEnvelope,
    pub independence: IndependenceEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationOutcome {
    pub disposition: ResultDisposition,
    pub reason: String,
    pub locally_verified: bool,
    pub independently_verified: bool,
}

impl VerificationOutcome {
    pub fn accepted(reason: impl Into<String>) -> Self {
        Self {
            disposition: ResultDisposition::Accepted,
            reason: reason.into(),
            locally_verified: true,
            independently_verified: false,
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            disposition: ResultDisposition::RejectedVerification,
            reason: reason.into(),
            locally_verified: false,
            independently_verified: false,
        }
    }
}

/// Structural checks that never treat signature success as correctness.
pub fn verify_structural(envelope: &ResultEnvelope) -> Result<(), FabricClientError> {
    let recomputed = digest_payload(&envelope.payload).map_err(FabricClientError::Protocol)?;
    if recomputed != envelope.body.result_digest {
        return Err(FabricClientError::Http(format!(
            "result digest mismatch: envelope claims {}, recomputed {}",
            envelope.body.result_digest.0, recomputed.0
        )));
    }
    if matches!(
        envelope.body.result_status,
        ResultStatus::Ok | ResultStatus::Failed | ResultStatus::Canceled | ResultStatus::Preempted
    ) {
        // status is well-formed
    }
    // Prompt-injection / analysis text is data only — never invoke tools/processes here.
    let _ = envelope.payload.get("suggested_commands");
    Ok(())
}

pub fn verify_local_adapters(envelope: &ResultEnvelope) -> Result<(), FabricClientError> {
    verify_structural(envelope)?;
    // Fabricated test reports: require consistent summary fields when present.
    if let Some(tests) = envelope.payload.get("test_report") {
        let claimed_pass = tests.get("passed").and_then(|v| v.as_bool());
        let exit = envelope.body.execution_summary.exit_status;
        if claimed_pass == Some(true) && exit.is_some_and(|e| e != 0) {
            return Err(FabricClientError::Http(
                "fabricated test report: passed=true with non-zero exit".into(),
            ));
        }
    }
    // Patch payloads must declare base workspace version matching envelope.
    if envelope.body.artifacts.iter().any(|a| a.kind == "patch") {
        let base = envelope
            .payload
            .get("base_workspace_version")
            .cloned()
            .ok_or_else(|| {
                FabricClientError::Http("patch result missing base_workspace_version".into())
            })?;
        let env_ws = serde_json::to_value(&envelope.body.workspace_version)
            .map_err(|e| FabricClientError::Http(format!("workspace serialize: {e}")))?;
        if base != env_ws {
            return Err(FabricClientError::Http(
                "patch base workspace version mismatch".into(),
            ));
        }
    }
    Ok(())
}

pub fn independence_holds(a: &IndependenceEvidence, b: &IndependenceEvidence) -> bool {
    if a.worker_id == b.worker_id {
        return false;
    }
    if a.host_id == b.host_id {
        return false;
    }
    if a.owner_domain == b.owner_domain {
        return false;
    }
    if a.shared_cache || b.shared_cache {
        return false;
    }
    if a.execution_nonce == b.execution_nonce
        || a.execution_nonce.is_empty()
        || b.execution_nonce.is_empty()
    {
        return false;
    }
    if a.model_instance.is_some() && a.model_instance == b.model_instance {
        return false;
    }
    true
}

pub fn compare_redundant_agreement(
    primary: &ResultEnvelope,
    candidates: &[RedundantCandidate],
    required_agreement: u32,
) -> Result<u32, FabricClientError> {
    let mut agreeing = 1u32;
    let mut seen_hosts = HashSet::new();
    seen_hosts.insert(primary.body.worker_id.0.clone());

    for cand in candidates {
        if !independence_holds(
            &IndependenceEvidence {
                worker_id: primary.body.worker_id.clone(),
                host_id: format!("host:{}", primary.body.worker_id.0),
                owner_domain: "primary".into(),
                model_instance: primary.body.execution_summary.model.clone(),
                execution_nonce: primary.body.result_id.0.clone(),
                shared_cache: false,
            },
            &cand.independence,
        ) {
            return Err(FabricClientError::Http(
                "redundant results lack meaningful independence".into(),
            ));
        }
        if cand.envelope.body.result_digest == primary.body.result_digest
            || structured_claims_agree(&primary.payload, &cand.envelope.payload)
        {
            agreeing += 1;
            seen_hosts.insert(cand.independence.host_id.clone());
        }
    }
    if agreeing < required_agreement {
        return Err(FabricClientError::Http(format!(
            "redundant agreement {agreeing} < required {required_agreement}"
        )));
    }
    let _ = seen_hosts;
    Ok(agreeing)
}

fn structured_claims_agree(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a.get("claims"), b.get("claims")) {
        (Some(ca), Some(cb)) => ca == cb,
        _ => false,
    }
}

pub fn apply_verification_policy(
    requirement: &ResultVerificationRequirement,
    envelope: &ResultEnvelope,
    redundant: &[RedundantCandidate],
) -> VerificationOutcome {
    match requirement {
        ResultVerificationRequirement::StructuralValidation => match verify_structural(envelope) {
            Ok(()) => VerificationOutcome {
                disposition: ResultDisposition::Accepted,
                reason: "structural validation passed".into(),
                locally_verified: false,
                independently_verified: false,
            },
            Err(e) => VerificationOutcome::rejected(e.to_string()),
        },
        ResultVerificationRequirement::LocalVerification => match verify_local_adapters(envelope) {
            Ok(()) => VerificationOutcome::accepted("local verification passed"),
            Err(e) => VerificationOutcome::rejected(e.to_string()),
        },
        ResultVerificationRequirement::IndependentRedundantVerification { required_agreement } => {
            if let Err(e) = verify_structural(envelope) {
                return VerificationOutcome::rejected(e.to_string());
            }
            match compare_redundant_agreement(envelope, redundant, *required_agreement) {
                Ok(_) => VerificationOutcome {
                    disposition: ResultDisposition::Accepted,
                    reason: "independent redundant verification passed".into(),
                    locally_verified: false,
                    independently_verified: true,
                },
                Err(e) => VerificationOutcome::rejected(e.to_string()),
            }
        }
        ResultVerificationRequirement::LocalAndRedundantVerification => {
            if let Err(e) = verify_local_adapters(envelope) {
                return VerificationOutcome::rejected(e.to_string());
            }
            match compare_redundant_agreement(envelope, redundant, 2) {
                Ok(_) => VerificationOutcome {
                    disposition: ResultDisposition::Accepted,
                    reason: "local and redundant verification passed".into(),
                    locally_verified: true,
                    independently_verified: true,
                },
                Err(e) => VerificationOutcome::rejected(e.to_string()),
            }
        }
    }
}

/// Map placement verification flag into a result verification requirement.
pub fn requirement_from_placement(
    require_verification: bool,
    policy_id: Option<&str>,
) -> ResultVerificationRequirement {
    if !require_verification {
        return ResultVerificationRequirement::StructuralValidation;
    }
    let pid = policy_id.unwrap_or("local");
    if pid == "redundant" || pid == "independent_redundant" {
        return ResultVerificationRequirement::IndependentRedundantVerification {
            required_agreement: 2,
        };
    }
    if let Some(rest) = pid.strip_prefix("redundant:") {
        let n = rest.parse::<u32>().unwrap_or(2).max(2);
        return ResultVerificationRequirement::IndependentRedundantVerification {
            required_agreement: n,
        };
    }
    if let Some(rest) = pid.strip_prefix("independent_redundant:") {
        let n = rest.parse::<u32>().unwrap_or(2).max(2);
        return ResultVerificationRequirement::IndependentRedundantVerification {
            required_agreement: n,
        };
    }
    if pid == "local_and_redundant" {
        return ResultVerificationRequirement::LocalAndRedundantVerification;
    }
    ResultVerificationRequirement::LocalVerification
}
