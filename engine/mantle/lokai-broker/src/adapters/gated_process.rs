//! ProcessBroker wrapper. Production execute is identity over the inner
//! sandbox broker (M2 D-2). ComputeBroker `dispatch_sandboxed` remains for
//! IndexShard / TestShard jobs, not this path.

use std::sync::Arc;

use async_trait::async_trait;
use lokai_domain::sinks::{
    AuthorizedProcessRequest, AuthorizedServiceRequest, ManagedProcessHandle, ManagedProcessResult,
    ProcessBroker, ProcessBrokerError,
};

use crate::broker::DefaultComputeBroker;

/// Production ProcessBroker: execute via inner ProcessBroker (no TestShard mapping).
pub struct BrokerGatedProcessBroker {
    _compute: Arc<DefaultComputeBroker>,
    inner: Arc<dyn ProcessBroker>,
}

impl BrokerGatedProcessBroker {
    pub fn new(compute: Arc<DefaultComputeBroker>, inner: Arc<dyn ProcessBroker>) -> Self {
        Self {
            _compute: compute,
            inner,
        }
    }
}

#[async_trait]
impl ProcessBroker for BrokerGatedProcessBroker {
    async fn execute(
        &self,
        request: AuthorizedProcessRequest,
    ) -> Result<ManagedProcessResult, ProcessBrokerError> {
        self.inner.execute(request).await
    }

    async fn start_service(
        &self,
        request: AuthorizedServiceRequest,
    ) -> Result<Box<dyn ManagedProcessHandle>, ProcessBrokerError> {
        self.inner.start_service(request).await
    }
}
