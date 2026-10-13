//! Conversational understanding adapts the existing versioned brief; it is not a plan.
use super::*;
use crate::resources::work_director::DirectorBinding;

impl WorkService {
    pub(super) async fn remember_understanding(
        &self,
        binding: &DirectorBinding,
        direction: String,
    ) -> Result<serde_json::Value, AppError> {
        let scope = self.services.authorized_scope().await?;
        let brief = self.services.local.resources().save_guide_brief(
            &self.services.host.credential,
            crate::resources::SaveWorkBrief {
                org: scope.organization().into(), team: scope.team().into(),
                work: binding.source.clone(), request: control::operation_id(&binding.turn, "understanding"),
                expected: binding.brief_revision, body: direction,
            }, binding.turn.clone(), binding.revision,
        ).await.map_err(|error| match error {
            crate::resources::ResourceError::Conflict => AppError::Conflict(
                "The understanding or proposal changed, work started, or this reply already saved different notes. Inspect the current understanding; do not overwrite it or repeat the save. Continue the conversation.".into()),
            other => resource(other),
        })?;
        Ok(
            serde_json::json!({"saved":true,"understanding":brief,"work_launched":false,
            "authority":"Discussion notes only, not an approved plan, grant or change to running work. The owner can inspect and edit these notes in this conversation."}),
        )
    }

    pub(super) async fn proposal_brief_revision(
        &self,
        binding: &DirectorBinding,
    ) -> Result<i64, AppError> {
        let briefs = self.work_briefs(&binding.source).await?;
        // Only our own successful save can advance the turn's pinned base.
        // An intervening human edit still conflicts at proposal publication.
        Ok(briefs
            .first()
            .filter(|b| {
                b.request_id == control::operation_id(&binding.turn, "understanding")
                    && b.revision.checked_sub(1) == Some(binding.brief_revision)
            })
            .map_or(binding.brief_revision, |b| b.revision))
    }
}
