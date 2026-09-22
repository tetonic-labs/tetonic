use crate::fabric::NodeInfo;
use crate::InferenceProvider;
use crate::{
    ActiveJobRegistry, ChatRequest, ChatResponse, FabricCallMeta, InferenceError, TokenSink,
};
use async_trait::async_trait;
use lokai_fabric_protocol::{CancellationRequest, WorkerCapabilityAdvertisement};

#[async_trait]
pub trait FabricNodeProvider: InferenceProvider + Send + Sync {
    fn node_id(&self) -> &str;
    fn label(&self) -> &str;
    fn fabric_capabilities(&self) -> WorkerCapabilityAdvertisement;
    /// Reject in-flight results after worker removal or identity revocation.
    fn mark_fabric_revoked(&self) {}

    /// Coordinator-assigned trust tier for placement policy (M5-3).
    fn worker_trust(&self) -> lokai_domain::WorkerTrust {
        lokai_domain::WorkerTrust::OwnerControlledEstate
    }

    /// Apply a coordinator trust change immediately (M5-3).
    fn set_worker_trust(&self, _trust: lokai_domain::WorkerTrust) {}

    /// True after coordinator-assigned trust was stamped (`set_worker_trust`).
    /// Ctor default owner is not resolved (QA-011).
    fn worker_trust_resolved(&self) -> bool {
        true
    }

    /// Fire-and-forget typed `/v1/jobs/cancel` when the worker advertises cancellation (R7-1).
    fn request_jobs_cancel(&self, _cancel: CancellationRequest) {}

    async fn probe_node(&self) -> Option<NodeInfo>;
    #[allow(clippy::too_many_arguments)]
    async fn chat_on_fabric(
        &self,
        req: ChatRequest,
        job_id: &str,
        attempt_id: &str,
        fabric: Option<&FabricCallMeta>,
        turn_affinity: Option<&str>,
        registry: Option<&ActiveJobRegistry>,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError>;
}
