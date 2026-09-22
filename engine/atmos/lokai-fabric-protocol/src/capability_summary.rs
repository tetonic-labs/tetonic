//! UI-safe worker capability summaries (M5-2).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::capability_document::WorkerCapabilities;
use crate::capability_validate::scheduling_eligible;
use crate::JobKind;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkerCapabilitySummary {
    pub worker_id: String,
    pub capability_revision: u64,
    pub boot_id: String,
    pub job_kinds: Vec<String>,
    pub model_count: usize,
    pub legacy_v1_chat_only: bool,
    pub draining: bool,
    pub load_score: f32,
    pub quarantined: bool,
    pub degraded: bool,
    pub schedulable: bool,
    pub software_version: String,
}

fn job_kind_label(kind: &JobKind) -> String {
    match kind {
        JobKind::Infer => "infer".into(),
        JobKind::Embed => "embed".into(),
        JobKind::AnalyzeCode => "analyze_code".into(),
        JobKind::IndexShard => "index_shard".into(),
        JobKind::TestShard => "test_shard".into(),
        JobKind::ReviewArtifact => "review_artifact".into(),
    }
}

impl WorkerCapabilities {
    pub fn ui_summary(
        &self,
        now: DateTime<Utc>,
        quarantined: bool,
        degraded: bool,
        require_fresh_dynamic: bool,
    ) -> WorkerCapabilitySummary {
        WorkerCapabilitySummary {
            worker_id: self.worker_id.0.clone(),
            capability_revision: self.capability_revision,
            boot_id: self.boot_id.clone(),
            job_kinds: self
                .supported_job_types
                .iter()
                .map(|j| job_kind_label(&j.job_kind))
                .collect(),
            model_count: self.model_inventory.len(),
            legacy_v1_chat_only: self
                .legacy_advertisement
                .as_ref()
                .is_some_and(|l| l.legacy_v1_chat_only),
            draining: self.runtime_capacity.draining,
            load_score: self.runtime_capacity.load_score,
            quarantined,
            degraded,
            schedulable: !quarantined && scheduling_eligible(self, now, require_fresh_dynamic),
            software_version: self.software_version.version.clone(),
        }
    }
}
