//! Host-bound execution authority checked at managed loop boundaries.
use tetonic_domain::work_scope::WorkScope;

#[async_trait::async_trait]
pub trait ExecutionGate: Send + Sync {
    /// Deny on revoked/unavailable authority. Implementations return no secrets.
    async fn authorize(&self) -> Result<(), ()>;
}

/// Denies further effects after the attempt scope is canceled.
pub struct ScopeCancellationGate {
    scope: WorkScope,
}

impl ScopeCancellationGate {
    pub fn new(scope: WorkScope) -> Self {
        Self { scope }
    }
}

#[async_trait::async_trait]
impl ExecutionGate for ScopeCancellationGate {
    async fn authorize(&self) -> Result<(), ()> {
        if self.scope.is_canceled() {
            Err(())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_denies_the_next_effect() {
        let scope = WorkScope::default();
        let gate = ScopeCancellationGate::new(scope.clone());
        assert!(gate.authorize().await.is_ok());
        scope.cancel();
        assert!(gate.authorize().await.is_err());
    }
}
