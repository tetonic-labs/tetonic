//! Host-bound execution authority checked at managed loop boundaries.
#[async_trait::async_trait]
pub trait ExecutionGate: Send + Sync {
    /// Deny on revoked/unavailable authority. Implementations return no secrets.
    async fn authorize(&self) -> Result<(), ()>;
}
