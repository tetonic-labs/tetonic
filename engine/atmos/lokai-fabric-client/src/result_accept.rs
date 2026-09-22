//! Safe remote result acceptance pipeline (M5-4).
//!
//! ```text
//! Remote result
//! → envelope validation
//! → stale-result rejection
//! → artifact quarantine
//! → schema and digest validation
//! → local or redundant verification
//! → explicit acceptance
//! → exact authorization (caller)
//! → transactional application (caller / patch pipeline)
//! ```
//!
//! Fabric-client may validate and quarantine. It does not construct or submit
//! `CompleteAttempt`; that remains a manager-only RunSupervisor command.

use chrono::{DateTime, Utc};
use lokai_artifact::quarantine::{
    inspect_remote_bytes, validate_remote_artifact_with, QuarantineLimits,
};
use lokai_domain::artifact::{
    ArtifactKind, ArtifactLocation, ArtifactMetadata, ArtifactState, RetentionPolicy,
    VerificationState,
};
use lokai_domain::classify::DataClass;
use lokai_domain::ids::ArtifactId;
use lokai_domain::result_integrity::{ArtifactOrigin, WorkerBehaviorSignals};
use lokai_domain::workspace::{ContentDigest, PatchApproval, WorkspaceVersion};
use lokai_domain::{
    CompleteAttempt, LeaseProof, ResultDisposition, ResultVerificationRequirement, RunSnapshot,
};
use lokai_fabric_protocol::{
    default_message_limits, validate_result_envelope, DispositionStore, EnvelopeValidationOutcome,
    ExpectedResultBinding, ResultEnvelope, ResultSigningKeyRegistry,
};

use crate::verification::{apply_verification_policy, RedundantCandidate, VerificationOutcome};
use crate::FabricClientError;

#[derive(Clone, Debug)]
pub struct RemoteResultAcceptRequest {
    pub envelope: ResultEnvelope,
    pub expected: ExpectedResultBinding,
    pub verification: ResultVerificationRequirement,
    pub redundant: Vec<RedundantCandidate>,
    /// Artifact bytes keyed by artifact_id (already received; still untrusted).
    pub artifact_bytes: Vec<(String, Vec<u8>)>,
    pub now: DateTime<Utc>,
    /// When set, RunSupervisor winner/stale checks gate CompleteAttempt emission.
    pub run_snapshot: Option<RunSnapshot>,
    pub lease_proof: Option<LeaseProof>,
    pub command_envelope: Option<lokai_domain::CommandEnvelope>,
}

#[derive(Clone, Debug)]
pub struct RemoteResultAcceptOutcome {
    pub disposition: ResultDisposition,
    pub reason: String,
    pub complete_attempt: Option<CompleteAttempt>,
    pub verification: Option<VerificationOutcome>,
    /// True when artifacts were quarantined but not yet accepted for use.
    pub quarantined: bool,
}

/// Process a signed remote result. Never invokes local tools, never mutates the
/// workspace, never issues capabilities.
pub fn accept_remote_result(
    req: &RemoteResultAcceptRequest,
    keys: &ResultSigningKeyRegistry,
    dispositions: &mut DispositionStore,
    signals: &mut WorkerBehaviorSignals,
) -> RemoteResultAcceptOutcome {
    let limits = default_message_limits();
    let validation =
        match validate_result_envelope(&req.envelope, &req.expected, keys, &limits, req.now) {
            Ok(v) => v,
            Err(e) => {
                return finish_reject(
                    req,
                    dispositions,
                    signals,
                    ResultDisposition::RejectedSchema,
                    e.message,
                    true,
                );
            }
        };

    if validation.disposition.is_terminal_reject() {
        let audit_only = matches!(
            validation.disposition,
            ResultDisposition::RejectedStaleAttempt
                | ResultDisposition::Superseded
                | ResultDisposition::RejectedCanceled
        );
        return finish_reject(
            req,
            dispositions,
            signals,
            validation.disposition,
            validation.reason,
            audit_only,
        );
    }

    // Quarantine every referenced artifact before any use.
    if let Err(outcome) = quarantine_artifacts(req) {
        return finish_reject(
            req,
            dispositions,
            signals,
            outcome.disposition,
            outcome.reason,
            false,
        );
    }

    let verify = apply_verification_policy(&req.verification, &req.envelope, &req.redundant);
    if verify.disposition != ResultDisposition::Accepted {
        return finish_reject(
            req,
            dispositions,
            signals,
            verify.disposition,
            verify.reason.clone(),
            false,
        );
    }

    if let Err(e) = deny_remote_side_effects(&req.envelope) {
        return finish_reject(
            req,
            dispositions,
            signals,
            ResultDisposition::RejectedPolicy,
            e.to_string(),
            false,
        );
    }

    let _receipt = dispositions.record(
        &req.envelope,
        ResultDisposition::Accepted,
        "verified; signature is provenance-only, not correctness",
        false,
        req.now,
    );
    RemoteResultAcceptOutcome {
        disposition: ResultDisposition::Accepted,
        reason: "verified current result binding and policy".into(),
        complete_attempt: None,
        verification: Some(verify),
        quarantined: true,
    }
}

fn finish_reject(
    req: &RemoteResultAcceptRequest,
    dispositions: &mut DispositionStore,
    signals: &mut WorkerBehaviorSignals,
    disposition: ResultDisposition,
    reason: impl Into<String>,
    audit_only: bool,
) -> RemoteResultAcceptOutcome {
    let reason = reason.into();
    signals.record(disposition);
    let _receipt = dispositions.record(
        &req.envelope,
        disposition,
        reason.clone(),
        audit_only,
        req.now,
    );
    // M6-3: security / result rejections are always-retain (never sampled away).
    lokai_telemetry::record_retained_outcome(
        &format!("result_rejected:{}", disposition.as_str()),
        None,
    );
    RemoteResultAcceptOutcome {
        disposition,
        reason,
        complete_attempt: None,
        verification: None,
        quarantined: false,
    }
}

fn quarantine_artifacts(req: &RemoteResultAcceptRequest) -> Result<(), EnvelopeValidationOutcome> {
    let limits = QuarantineLimits {
        max_bytes: req.expected.output_limits.max_artifact_size.max(1),
        ..QuarantineLimits::default()
    };
    for art in &req.envelope.body.artifacts {
        let matches: Vec<_> = req
            .artifact_bytes
            .iter()
            .filter(|(id, _)| id == &art.artifact_id)
            .collect();
        if matches.len() != 1 {
            return Err(EnvelopeValidationOutcome::reject(
                ResultDisposition::RejectedDigestMismatch,
                format!(
                    "artifact {} requires exactly one local byte payload",
                    art.artifact_id
                ),
            ));
        }
        let bytes = matches[0].1.as_slice();
        if bytes.len() as u64 != art.size_bytes || bytes.len() as u64 > limits.max_bytes {
            return Err(EnvelopeValidationOutcome::reject(
                ResultDisposition::RejectedOversized,
                format!(
                    "artifact {} byte size differs from signed size or exceeds limit",
                    art.artifact_id
                ),
            ));
        }
        let actual = ContentDigest::new(format!("sha256:{}", sha256_hex(bytes)));
        if actual != art.digest {
            return Err(EnvelopeValidationOutcome::reject(
                ResultDisposition::RejectedDigestMismatch,
                format!("artifact {} digest mismatch", art.artifact_id),
            ));
        }
        let inspection = inspect_remote_bytes(bytes, &art.declared_paths).map_err(|e| {
            EnvelopeValidationOutcome::reject(
                ResultDisposition::RejectedVerification,
                e.to_string(),
            )
        })?;
        let mut meta = ArtifactMetadata {
            artifact_id: ArtifactId::new(&art.artifact_id),
            kind: parse_artifact_kind(&art.kind),
            schema_version: 1,
            producer_run_id: req.envelope.body.run_id.clone(),
            producer_task_id: req.envelope.body.task_id.clone(),
            producer_attempt_id: req.envelope.body.attempt_id.clone(),
            worker_id: Some(req.envelope.body.worker_id.clone()),
            content_digest: art.digest.clone(),
            size_bytes: art.size_bytes,
            workspace_version: req.envelope.body.workspace_version.clone(),
            data_class: DataClass::RepositorySource,
            storage_location: ArtifactLocation::Remote(art.artifact_id.clone()),
            lifecycle_state: ArtifactState::Sealed,
            verification_state: VerificationState::Unverified,
            origin: ArtifactOrigin::Remote,
            created_at: req.now,
            retention_policy: RetentionPolicy::SecurityAudit,
        };
        validate_remote_artifact_with(&mut meta, &art.digest, &limits, &inspection).map_err(
            |e| match e {
                lokai_domain::artifact::ArtifactError::DigestMismatch { .. } => {
                    EnvelopeValidationOutcome::reject(
                        ResultDisposition::RejectedDigestMismatch,
                        e.to_string(),
                    )
                }
                lokai_domain::artifact::ArtifactError::QuarantineRejected(msg)
                    if msg.contains("executable") =>
                {
                    EnvelopeValidationOutcome::reject(
                        ResultDisposition::RejectedExecutablePayload,
                        msg,
                    )
                }
                lokai_domain::artifact::ArtifactError::QuarantineRejected(msg)
                    if msg.contains("path") || msg.contains("unsafe") =>
                {
                    EnvelopeValidationOutcome::reject(ResultDisposition::RejectedPathUnsafe, msg)
                }
                lokai_domain::artifact::ArtifactError::QuarantineRejected(msg)
                    if msg.contains("size") || msg.contains("archive") =>
                {
                    EnvelopeValidationOutcome::reject(ResultDisposition::RejectedOversized, msg)
                }
                other => EnvelopeValidationOutcome::reject(
                    ResultDisposition::RejectedSchema,
                    other.to_string(),
                ),
            },
        )?;
    }
    Ok(())
}

fn parse_artifact_kind(raw: &str) -> ArtifactKind {
    match raw {
        "patch" => ArtifactKind::Patch,
        "test_report" => ArtifactKind::TestReport,
        "analysis_report" => ArtifactKind::AnalysisReport,
        "diff" => ArtifactKind::Diff,
        _ => ArtifactKind::ModelResponse,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Patch acceptance pipeline after a verified envelope. Never applies bytes that
/// differ from the approved digest; does not invoke remote-suggested commands.
pub fn validate_patch_acceptance(
    envelope: &ResultEnvelope,
    approval: &PatchApproval,
    staged_patch_digest: &ContentDigest,
    current_workspace: &WorkspaceVersion,
    authorization_granted: bool,
    sandboxed_verification_passed: bool,
) -> Result<(), FabricClientError> {
    if !authorization_granted {
        return Err(FabricClientError::Http(
            "patch application requires exact local authorization".into(),
        ));
    }
    if envelope.body.workspace_version.as_ref() != Some(current_workspace) {
        return Err(FabricClientError::Http(
            "workspace version changed; patch rejected".into(),
        ));
    }
    if &approval.base_version != current_workspace {
        return Err(FabricClientError::Http(
            "approval base version mismatch".into(),
        ));
    }
    if &approval.patch_digest != staged_patch_digest {
        return Err(FabricClientError::Http(
            "patch changed after approval; approval invalidated".into(),
        ));
    }
    if approval.verification_required && !sandboxed_verification_passed {
        return Err(FabricClientError::Http(
            "sandboxed verification required before commit".into(),
        ));
    }
    // Remote result cannot directly mutate workspace — caller must use lokai-transaction.
    Ok(())
}

/// Explicit deny: remote results never authorize capability issuance or process execution.
pub fn deny_remote_side_effects(envelope: &ResultEnvelope) -> Result<(), FabricClientError> {
    if envelope.payload.get("issue_capability").is_some()
        || envelope.payload.get("invoke_tool").is_some()
        || envelope.payload.get("run_process").is_some()
        || envelope.payload.get("approve_action").is_some()
    {
        return Err(FabricClientError::Http(
            "remote result cannot authorize capabilities, tools, processes, or approvals".into(),
        ));
    }
    Ok(())
}
