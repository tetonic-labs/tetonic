//! Versioned candidate payload shared by the producer and authorized readers.
//! Parsing a payload does not prove acceptance; the run journal and artifact
//! provenance/digest checks must authorize it before presenting a result.
use crate::CandidateOutcome;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateArtifactOutcome {
    Completed,
    Canceled,
    Limited,
    Failed,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateArtifactV1 {
    #[serde(deserialize_with = "version_one")]
    v: u8,
    pub outcome: CandidateArtifactOutcome,
    pub message: String,
}

fn version_one<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let version = u8::deserialize(deserializer)?;
    if version != 1 {
        return Err(serde::de::Error::custom(
            "unsupported candidate artifact version",
        ));
    }
    Ok(version)
}

impl From<&CandidateOutcome> for CandidateArtifactV1 {
    fn from(outcome: &CandidateOutcome) -> Self {
        use CandidateArtifactOutcome as Tag;
        let (outcome, message) = match outcome {
            CandidateOutcome::Completed { summary, .. } => (Tag::Completed, summary),
            CandidateOutcome::Canceled { reason } => (Tag::Canceled, reason),
            CandidateOutcome::Limited { message, .. } => (Tag::Limited, message),
            CandidateOutcome::Failed { message } => (Tag::Failed, message),
        };
        Self {
            v: 1,
            outcome,
            message: message.clone(),
        }
    }
}

impl CandidateArtifactV1 {
    /// Retains the existing JSON object ordering and bytes for content digests.
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&serde_json::to_value(self)?)
    }
    pub fn completed_message(&self) -> Option<&str> {
        (self.outcome == CandidateArtifactOutcome::Completed && !self.message.trim().is_empty())
            .then_some(self.message.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_payload_bytes_and_reads_version_one() {
        let outcome = CandidateOutcome::Completed {
            summary: "A result with \"quotes\" and unicode: λ".into(),
            kind: crate::CompletionKind::Answer,
        };
        let payload = CandidateArtifactV1::from(&outcome);
        let old = serde_json::json!({"v":1,"outcome":"completed","message":payload.message});
        assert_eq!(payload.encode().unwrap(), serde_json::to_vec(&old).unwrap());
        let restored: CandidateArtifactV1 =
            serde_json::from_slice(&payload.encode().unwrap()).unwrap();
        assert_eq!(restored.completed_message(), Some(payload.message.as_str()));
    }

    #[test]
    fn rejects_ambiguous_versions_tags_missing_fields_and_duplicate_fields() {
        for json in [
            r#"{"v":2,"outcome":"completed","message":"future"}"#,
            r#"{"outcome":"completed","message":"unversioned"}"#,
            r#"{"v":1,"outcome":"succeeded","message":"unknown tag"}"#,
            r#"{"v":1,"outcome":"completed"}"#,
            r#"{"v":1,"outcome":"completed","message":null}"#,
            r#"{"v":1,"outcome":"completed","message":"first","message":"second"}"#,
            r#"{"v":1,"outcome":"completed","message":"text","accepted":true}"#,
        ] {
            assert!(
                serde_json::from_str::<CandidateArtifactV1>(json).is_err(),
                "{json}"
            );
        }
        let failed: CandidateArtifactV1 =
            serde_json::from_str(r#"{"v":1,"outcome":"failed","message":"not a result"}"#).unwrap();
        assert_eq!(failed.completed_message(), None);
    }
}
