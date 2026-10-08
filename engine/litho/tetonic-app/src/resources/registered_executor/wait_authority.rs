//! Keep the root's pinned stop boundary in the existing runtime authorization chain.
use super::*;
use tetonic_domain::{AgentIdentity, AgentJobSpec, ExecutionScope};
use tetonic_run::managed::ExecutionAuthority;

pub(super) struct WaitAuthority {
    pub inner: Arc<dyn ExecutionAuthority>,
    pub store: SharedStore,
    pub org: String,
    pub team: String,
    pub work: String,
    pub stop_binding: String,
}

#[async_trait]
impl ExecutionAuthority for WaitAuthority {
    async fn authorize(
        &self,
        scope: &ExecutionScope,
        identity: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> Result<(), ()> {
        self.inner.authorize(scope, identity, job).await?;
        let (org, team, work, identity) = (
            self.org.clone(),
            self.team.clone(),
            self.work.clone(),
            identity.id.0.clone(),
        );
        if scope.organization_id != org {
            return Err(());
        }
        let current = self
            .store
            .read(move |db| db.work_human_stop_binding(&org, &team, &work, &identity))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        if current == self.stop_binding {
            Ok(())
        } else {
            Err(())
        }
    }

    async fn revoked_during_execution(
        &self,
        scope: &ExecutionScope,
        identity: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> bool {
        self.inner
            .revoked_during_execution(scope, identity, job)
            .await
            || self.authorize(scope, identity, job).await.is_err()
    }
}
