use super::*;
use serde_json::{json, Value};
use tetonic_domain::work_scope::CancellationSignal;
use tetonic_domain::{AuthorizedAction, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal};

#[derive(Clone)]
pub(crate) struct SkillToolHost {
    pub inner: Box<dyn ToolHost>,
    pub library: Option<Arc<SkillLibrary>>,
    pub selected: HashSet<String>,
}
impl ToolHost for SkillToolHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn checkpoint_ready(&self) -> bool {
        self.inner.checkpoint_ready()
            && self
                .selected
                .iter()
                .filter(|id| id.starts_with("skill_"))
                .all(|id| self.is_tool_allowed(id))
    }
    fn propose(&self, name: &str, args: &Value) -> Option<ToolProposal> {
        if name.starts_with("skill_") {
            None
        } else {
            self.inner.propose(name, args)
        }
    }
    fn is_tool_allowed(&self, name: &str) -> bool {
        if name.starts_with("skill_") {
            self.selected.contains(name) && self.library.as_ref().is_some_and(|l| l.contains(name))
        } else {
            self.inner.is_tool_allowed(name)
        }
    }
    fn is_read_only(&self, name: &str) -> bool {
        if name.starts_with("skill_") {
            self.is_tool_allowed(name)
        } else {
            self.inner.is_read_only(name)
        }
    }
    fn requires_action_broker(&self, name: &str) -> bool {
        !name.starts_with("skill_") && self.inner.requires_action_broker(name)
    }
    fn requires_user_approval(&self, name: &str) -> bool {
        !name.starts_with("skill_") && self.inner.requires_user_approval(name)
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        let mut ads = self.inner.advertisements();
        if let Some(library) = &self.library {
            for skill in library
                .list()
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s.enabled && self.selected.contains(&s.id))
            {
                ads.push(ToolAdvertisement { name: skill.id, description: format!("Load the granted {} skill instructions when relevant: {}. This does not grant additional tools or permissions.", skill.name, skill.description), parameters: json!({"type":"object","properties":{},"additionalProperties":false}) });
            }
        }
        ads
    }
    fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String> {
        if !name.starts_with("skill_") {
            return self.inner.validate_tool_args(name, args);
        }
        if !self.is_tool_allowed(name) || !args.as_object().is_some_and(|o| o.is_empty()) {
            return Err(
                "Skill is not granted, has been revoked, or arguments are invalid. Use {}.".into(),
            );
        }
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&AuthorizedAction>,
        cancel: &CancellationSignal,
    ) -> ToolOutcome {
        if !name.starts_with("skill_") {
            return self.inner.execute_authorized(name, args, auth, cancel);
        }
        if cancel.is_canceled() || self.validate_tool_args(name, args).is_err() {
            return ToolOutcome::fail("Skill is unavailable or not granted.", "denied");
        }
        match self
            .library
            .as_ref()
            .and_then(|l| l.content(name, true).ok().flatten())
        {
            Some(content) => ToolOutcome::ok("Loaded skill instructions", content),
            None => ToolOutcome::fail(
                "Skill access was revoked or the workspace could not be read.",
                "denied",
            ),
        }
    }
}

pub(crate) struct SkillAuthority {
    pub inner: Arc<dyn tetonic_run::managed::ExecutionAuthority>,
    pub library: Arc<SkillLibrary>,
}
#[async_trait::async_trait]
impl tetonic_run::managed::ExecutionAuthority for SkillAuthority {
    async fn authorize(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> Result<(), ()> {
        self.inner.authorize(scope, identity, job).await?;
        if self
            .library
            .authorized(scope, &job.capability_bindings.iter().cloned().collect())
        {
            Ok(())
        } else {
            Err(())
        }
    }
    async fn revoked_during_execution(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> bool {
        !self
            .library
            .authorized(scope, &job.capability_bindings.iter().cloned().collect())
            || self
                .inner
                .revoked_during_execution(scope, identity, job)
                .await
    }
}
