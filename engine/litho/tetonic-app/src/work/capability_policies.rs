use super::*;
pub use tetonic_memory::{CapabilityScope, SaveCapabilityPolicy, ScopedCapabilityPolicy};

impl WorkService {
    pub async fn capability_policies(&self) -> Result<Vec<ScopedCapabilityPolicy>, AppError> {
        let scope = self.services.authorized_scope().await?;
        self.services
            .local
            .resources()
            .capability_policies(
                &self.services.host.credential,
                scope.organization().into(),
                scope.team().into(),
            )
            .await
            .map_err(resource)
    }
    pub async fn save_capability_policy(
        &self,
        request: SaveCapabilityPolicy,
    ) -> Result<ScopedCapabilityPolicy, AppError> {
        let scope = self.services.authorized_scope().await?;
        validate_request_id(&request.request_id)?;
        self.services
            .local
            .resources()
            .save_capability_policy(
                &self.services.host.credential,
                scope.organization().into(),
                scope.team().into(),
                request,
            )
            .await
            .map_err(|error| match error {
                crate::resources::ResourceError::Conflict => {
                    AppError::Conflict("Permissions changed. Reload before saving again.".into())
                }
                other => resource(other),
            })
    }
}
