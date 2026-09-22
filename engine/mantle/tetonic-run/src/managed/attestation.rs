//! Seal Attempt-bound candidate bytes. Session rows are not the outcome source.

use super::ManagedRunError;
use std::sync::Arc;
use tetonic_domain::artifact::{ArtifactDeclaration, ArtifactKind, ArtifactStore, RetentionPolicy};
use tetonic_domain::classify::DataClass;
use tetonic_domain::ids::{ArtifactId, AttemptId, RunId, TaskId};
use tetonic_domain::CandidateOutcome;

pub struct SealedTurn {
    pub artifact_id: ArtifactId,
    pub digest: String,
}

pub fn encode_candidate_bytes(outcome: &CandidateOutcome) -> Result<Vec<u8>, ManagedRunError> {
    let (tag, message) = match outcome {
        CandidateOutcome::Completed { summary, .. } => ("completed", summary.as_str()),
        CandidateOutcome::Canceled { reason } => ("canceled", reason.as_str()),
        CandidateOutcome::Limited { message, .. } => ("limited", message.as_str()),
        CandidateOutcome::Failed { message } => ("failed", message.as_str()),
    };
    serde_json::to_vec(&serde_json::json!({
        "v": 1,
        "outcome": tag,
        "message": message,
    }))
    .map_err(|e| ManagedRunError::InternalViolation(e.to_string()))
}

pub async fn seal_output_set(
    artifacts: &Arc<dyn ArtifactStore>,
    run_id: &RunId,
    task_id: &TaskId,
    attempt_id: &AttemptId,
    bytes: Vec<u8>,
) -> Result<SealedTurn, ManagedRunError> {
    let mut writer = artifacts
        .begin_write(ArtifactDeclaration {
            kind: ArtifactKind::FileSnapshot,
            producer_run_id: run_id.clone(),
            producer_task_id: task_id.clone(),
            producer_attempt_id: attempt_id.clone(),
            worker_id: None,
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            retention_policy: RetentionPolicy::ProjectHistory,
        })
        .await
        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
    writer
        .write_chunk(&bytes)
        .await
        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
    let meta = writer
        .seal()
        .await
        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
    tetonic_artifact::verify_stored_content(artifacts.as_ref(), &meta.artifact_id)
        .await
        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
    Ok(SealedTurn {
        artifact_id: meta.artifact_id,
        digest: format!("sha256:{}", meta.content_digest.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_output_encodes_identically() {
        let outcome = CandidateOutcome::Completed {
            summary: "done".into(),
            kind: tetonic_domain::CompletionKind::Finish,
        };
        let a = encode_candidate_bytes(&outcome).unwrap();
        let b = encode_candidate_bytes(&outcome).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn one_changed_byte_changes_encoding() {
        let a = encode_candidate_bytes(&CandidateOutcome::Completed {
            summary: "x".into(),
            kind: tetonic_domain::CompletionKind::Finish,
        })
        .unwrap();
        let b = encode_candidate_bytes(&CandidateOutcome::Completed {
            summary: "y".into(),
            kind: tetonic_domain::CompletionKind::Finish,
        })
        .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn error_encoding_is_not_a_success_encoding() {
        let success = encode_candidate_bytes(&CandidateOutcome::Completed {
            summary: String::new(),
            kind: tetonic_domain::CompletionKind::Finish,
        })
        .unwrap();
        let failed = encode_candidate_bytes(&CandidateOutcome::Failed {
            message: "boom".into(),
        })
        .unwrap();
        assert_ne!(success, failed);
        assert!(!String::from_utf8_lossy(&failed).contains("completed"));
    }
}
