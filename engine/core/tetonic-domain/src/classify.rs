//! Data classification and disclosure tier (system-wide shared kernel).
//!
//! Canonical home for these enums — `lokai-inference` re-exports them for fabric RPC
//! call sites; all other crates should depend on `tetonic_domain` directly.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// M2-2 data sensitivity classes (increasing order).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    Public,
    RepositorySource,
    SensitiveSource,
    Secret,
}

impl Default for DataClass {
    /// Conservative default: ordinary repository content, never `Public`.
    fn default() -> Self {
        Self::RepositorySource
    }
}

impl DataClass {
    /// Artifact classification is never lower than its sensitive inputs without an explicit audited transformation.
    pub fn can_downgrade_to(&self, other: &DataClass) -> bool {
        // can_downgrade_to returns true ONLY if we are actually just keeping it the same or upgrading,
        // or if there is an explicit mechanism. Here we just check the natural ordering.
        // If we try to go from Secret -> Public, `other < self`, so it's a downgrade, return false.
        other >= self
    }
}

/// Schema version for classification policy decisions (M2-2).
pub type PolicyVersion = u32;

pub const CLASSIFICATION_POLICY_VERSION: PolicyVersion = 1;

/// Why content received a class (audit / UI explainability).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationSource {
    DefaultRule,
    PathPolicy,
    UserDesignation,
    SecretDetector,
    DerivedFromInput,
    ExplicitReclassification,
    ContentHeuristic,
    SessionFloor,
    AggregatedPayload,
}

impl ClassificationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultRule => "default_rule",
            Self::PathPolicy => "path_policy",
            Self::UserDesignation => "user_designation",
            Self::SecretDetector => "secret_detector",
            Self::DerivedFromInput => "derived_from_input",
            Self::ExplicitReclassification => "explicit_reclassification",
            Self::ContentHeuristic => "content_heuristic",
            Self::SessionFloor => "session_floor",
            Self::AggregatedPayload => "aggregated_payload",
        }
    }
}

/// A classification decision with provenance (M2-2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Classification {
    pub class: DataClass,
    pub sources: Vec<ClassificationSource>,
    pub policy_version: PolicyVersion,
    pub classified_at: DateTime<Utc>,
}

impl Classification {
    pub fn new(class: DataClass, sources: Vec<ClassificationSource>) -> Self {
        Self {
            class,
            sources,
            policy_version: CLASSIFICATION_POLICY_VERSION,
            classified_at: Utc::now(),
        }
    }

    pub fn summary(&self) -> ClassificationSummary {
        ClassificationSummary {
            class: self.class,
            sources: self
                .sources
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            policy_version: self.policy_version,
        }
    }

    pub fn with_source(mut self, source: ClassificationSource) -> Self {
        if !self.sources.contains(&source) {
            self.sources.push(source);
        }
        self
    }
}

/// Serializable classification metadata for logs, RPC, and artifacts (no raw content).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClassificationSummary {
    pub class: DataClass,
    pub sources: Vec<String>,
    pub policy_version: PolicyVersion,
}

impl ClassificationSummary {
    pub fn conservative_unknown() -> Self {
        Self {
            class: DataClass::RepositorySource,
            sources: vec!["missing_metadata".into()],
            policy_version: CLASSIFICATION_POLICY_VERSION,
        }
    }
}

/// Sensitivity rank (higher = more sensitive). Used for combination / floor rules.
pub fn data_class_sensitivity_rank(c: DataClass) -> u8 {
    match c {
        DataClass::Public => 0,
        DataClass::RepositorySource => 1,
        DataClass::SensitiveSource => 2,
        DataClass::Secret => 3,
    }
}

/// Combining inputs yields the highest (most sensitive) class among them.
pub fn combine_data_classes(classes: impl IntoIterator<Item = DataClass>) -> Option<DataClass> {
    classes
        .into_iter()
        .max_by_key(|c| data_class_sensitivity_rank(*c))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureTier {
    #[default]
    MetadataOnly,
    Summary,
    Auditable,
    Blind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_class_serde_stable() {
        assert_eq!(
            serde_json::to_string(&DataClass::Secret).unwrap(),
            "\"secret\""
        );
    }

    #[test]
    fn default_is_repository_source_not_public() {
        assert_eq!(DataClass::default(), DataClass::RepositorySource);
    }

    #[test]
    fn combine_takes_max_sensitivity() {
        assert_eq!(
            combine_data_classes([
                DataClass::Public,
                DataClass::RepositorySource,
                DataClass::Secret,
            ]),
            Some(DataClass::Secret)
        );
    }

    #[test]
    fn test_attempted_automatic_downgrade() {
        assert!(!DataClass::Secret.can_downgrade_to(&DataClass::Public));
        assert!(!DataClass::SensitiveSource.can_downgrade_to(&DataClass::RepositorySource));
        assert!(DataClass::Public.can_downgrade_to(&DataClass::Secret)); // Upgrade is fine
    }
}
