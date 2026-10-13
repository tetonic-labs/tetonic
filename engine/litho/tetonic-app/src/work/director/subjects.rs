//! Guide-specific composition over the context store. Subject selection is never
//! a dispatch target, a grant, or authority to replace another proposal.
use super::*;

#[derive(Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum GuideContextCommand {
    List {
        #[serde(default)]
        offset: Option<usize>,
    },
    Read {
        subject_id: String,
    },
    Save {
        subject: tetonic_memory::GuideSubjectDraft,
    },
    Focus {
        subject_id: Option<String>,
        expected_revision: i64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideContextMutation {
    pub request_id: String,
    pub command: GuideContextCommand,
}

impl WorkService {
    pub async fn guide_context_view(
        &self,
        conversation: &str,
    ) -> Result<serde_json::Value, AppError> {
        validate_request_id(conversation)?;
        let scope = self.services.authorized_scope().await?;
        let source = conversation.to_owned();
        self.services.local.store().read(move |db| {
            let focus=db.guide_focus(scope.principal(),scope.organization(),scope.team(),&source)?;
            let subject=focus.subject_id.as_ref().map(|id|db.guide_subject(scope.principal(),scope.organization(),scope.team(),id)).transpose()?.flatten();
            let subjects=db.guide_subjects(scope.principal(),scope.organization(),scope.team(),0)?;
            Ok::<_,tetonic_memory::StoreError>(serde_json::json!({"focus":focus,"subject":subject,"subjects":subjects.iter().map(|s|serde_json::json!({"id":s.id,"title":s.title,"revision":s.revision})).collect::<Vec<_>>(),"partial":subjects.len()==24}))
        }).await.map_err(|_|AppError::InferenceUnavailable)?.map_err(|e|resource(e.into()))
    }

    pub async fn update_guide_context(
        &self,
        conversation: &str,
        input: GuideContextMutation,
    ) -> Result<serde_json::Value, AppError> {
        let _admission = self.services.admission.lock().await;
        self.guide_context_command(conversation, input).await
    }

    pub(super) async fn guide_context_command(
        &self,
        conversation: &str,
        input: GuideContextMutation,
    ) -> Result<serde_json::Value, AppError> {
        validate_request_id(conversation)?;
        validate_request_id(&input.request_id)?;
        let scope = self.services.authorized_scope().await?;
        let source = conversation.to_owned();
        self.services.local.store().write(move |db| {
            db.require_guide_source(scope.principal(),scope.organization(),scope.team(),&source)?;
            match input.command {
                GuideContextCommand::List{offset} => {
                    let offset=offset.unwrap_or(0);
                    if offset>100_000 {return Err(tetonic_memory::StoreError::InvalidControlResource("subject page".into()));}
                    let rows=db.guide_subjects(scope.principal(),scope.organization(),scope.team(),offset)?;
                    Ok(serde_json::json!({"subjects":rows.iter().map(|s|serde_json::json!({"id":s.id,"title":s.title,"revision":s.revision,"work_ids":s.work_ids})).collect::<Vec<_>>(),"next_offset":(rows.len()==24).then_some(offset+24)}))
                },
                GuideContextCommand::Read{subject_id} => Ok(serde_json::json!({"subject":db.guide_subject(scope.principal(),scope.organization(),scope.team(),&subject_id)?})),
                GuideContextCommand::Save{subject} => Ok(serde_json::json!({"subject":db.save_guide_subject(scope.principal(),scope.organization(),scope.team(),&input.request_id,subject)?,"work_launched":false})),
                GuideContextCommand::Focus{subject_id,expected_revision} => Ok(serde_json::json!({"focus":db.set_guide_focus(scope.principal(),scope.organization(),scope.team(),&source,&input.request_id,tetonic_memory::GuideFocus{revision:expected_revision,subject_id})?,"work_launched":false,"authority":"Context selection only. No work moved, stopped, started or authorized. Proposal remains bound to the current conversation."})),
            }
        }).await.map_err(|_|AppError::InferenceUnavailable)?.map_err(|e|match e {
            tetonic_memory::StoreError::ControlResourceConflict => AppError::Conflict("This understanding or focus changed. Read it again before applying a correction; don't overwrite the owner's edit.".into()),
            other=>resource(other.into()),
        })
    }
}
