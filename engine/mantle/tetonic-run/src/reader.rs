//! Observation without execution-owner startup recovery.

use tetonic_domain::{ReplayGap, RunEventEnvelope, RunId, RunSnapshot, RunSupervisorError};
use tetonic_memory::SharedStore;

use crate::{DurableRunSupervisor, RunSupervisor};

/// Storage-backed observation only. Creating a reader never recovers or mutates runs.
/// The command interface is deliberately not exposed by this type.
pub struct DurableRunReader {
    supervisor: DurableRunSupervisor,
}

impl DurableRunReader {
    pub fn new(store: SharedStore) -> Self {
        Self {
            supervisor: DurableRunSupervisor::without_recovery(Some(store)),
        }
    }

    pub async fn snapshot(&self, run: RunId) -> Result<RunSnapshot, RunSupervisorError> {
        self.supervisor.snapshot(run).await
    }

    pub async fn resume_from_sequence(
        &self,
        run: RunId,
        after: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, RunSupervisorError> {
        self.supervisor
            .resume_from_sequence(run, after, limit)
            .await
    }
}
