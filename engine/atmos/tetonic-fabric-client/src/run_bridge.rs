//! Optional RunSupervisor bridge for task-scoped remote result acceptance (M5-4).

use async_trait::async_trait;
use tetonic_domain::{CommandEnvelope, LeaseProof, RunSnapshot};

/// Supplies live run/attempt authority so fabric acceptance cannot bypass
/// RunSupervisor winner / cancel / lease checks.
#[async_trait]
pub trait RemoteResultRunBridge: Send + Sync {
    async fn load_snapshot(&self, run_id: &str) -> Result<Option<RunSnapshot>, String>;

    async fn lease_proof_for_attempt(
        &self,
        run_id: &str,
        attempt_id: &str,
    ) -> Result<Option<(LeaseProof, CommandEnvelope)>, String>;
}
