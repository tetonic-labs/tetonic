//! Local sandboxed process / test execution via shared ProcessBroker (M6-1).

use std::sync::Arc;

use async_trait::async_trait;
use tetonic_domain::sinks::{
    AuthorizedProcessRequest, ManagedProcessResult, ProcessBroker, ProcessBrokerError,
};

/// Adapter that refuses to bypass ProcessBroker / sandbox.
pub struct LocalProcessTargetAdapter {
    broker: Arc<dyn ProcessBroker>,
}

impl LocalProcessTargetAdapter {
    pub fn new(broker: Arc<dyn ProcessBroker>) -> Self {
        Self { broker }
    }

    pub async fn execute(
        &self,
        request: AuthorizedProcessRequest,
    ) -> Result<ManagedProcessResult, ProcessBrokerError> {
        self.broker.execute(request).await
    }
}

/// Marker trait for broker-owned local targets.
#[async_trait]
pub trait LocalComputeTarget: Send + Sync {
    async fn run_sandboxed_process(
        &self,
        request: AuthorizedProcessRequest,
    ) -> Result<ManagedProcessResult, ProcessBrokerError>;
}

#[async_trait]
impl LocalComputeTarget for LocalProcessTargetAdapter {
    async fn run_sandboxed_process(
        &self,
        request: AuthorizedProcessRequest,
    ) -> Result<ManagedProcessResult, ProcessBrokerError> {
        self.execute(request).await
    }
}
