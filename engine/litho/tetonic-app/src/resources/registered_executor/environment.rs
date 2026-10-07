//! Durable approval binds a child's own provider and tools without exposing them
//! to its coordinator. Execution still checks capabilities, consent and lineage.
use super::*;
impl RegisteredExecutionSettings {
    pub(crate) fn environment_binding(&self, capabilities: &[String]) -> Result<String, AppError> {
        use sha2::Digest;
        let root = self
            .workspace_root
            .as_ref()
            .map(|path| {
                tetonic_tools::Workspace::new(path)
                    .map(|w| w.root().to_string_lossy().into_owned())
                    .map_err(|_| AppError::WorkspaceUnavailable)
            })
            .transpose()?;
        let mut tools = capabilities.to_vec();
        tools.sort();
        let value = serde_json::json!({"version":1,"workspace":root,"tools":tools,
            "model":self.model,"num_ctx":self.num_ctx,"data_class":self.data_class,
            "hosted":self.hosted.as_ref().map(|h| &h.binding),
            "seconds":self.max_elapsed_seconds,"tokens":self.reported_token_ceiling,
            "steps":self.limits.max_steps,"input":self.limits.max_input_bytes,
            "handoff":self.limits.human_handoff,"director":self.limits.work_director,"schema":self.response_schema});
        Ok(format!(
            "{:x}",
            sha2::Sha256::digest(value.to_string().as_bytes())
        ))
    }
}
