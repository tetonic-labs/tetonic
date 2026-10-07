//! Durable host handoff. It never replays an ordinary tool or a model request.
use super::*;

impl Agent {
    /// Enable checkpointing for a human handoff in the managed general harness.
    /// The host hook must be idempotent by pending call ID: restoration can call
    /// it again to retrieve the same saved question/answer. Ordinary effects and
    /// delegated dispatch are not replayable handoffs.
    pub fn with_durable_waits(mut self) -> Self {
        self.durable_waits = true;
        self
    }

    pub(crate) fn wait_harness_binding(&self) -> serde_json::Value {
        let mut config = self.config.clone();
        // These are stamped by the managed owner after conformance validation.
        config.run_id = None;
        config.task_id = None;
        config.attempt_id = None;
        serde_json::json!({"config": config, "tools": self.inference_schemas(),
            "checkpoint_ready": self.tools.checkpoint_ready(),
            "durable_waits": self.durable_waits})
    }

    pub(super) async fn host_wait(
        &self,
        convo: &mut Conversation,
        checkpoint: crate::WaitCheckpoint,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> Result<(), CandidateOutcome> {
        let fail = || CandidateOutcome::Failed {
            message: "The saved handoff could not be safely continued.".into(),
        };
        checkpoint.validate().map_err(|_| fail())?;
        if !checkpoint.matches_agent(self) {
            return Err(fail());
        }
        // Compiled coding contexts have additional transient state. They are not
        // supported by this registered general-harness checkpoint contract.
        if !self.durable_waits
            || self.context_compiler.is_some()
            || self.turn_instructions.is_some()
            || !self.tools.checkpoint_ready()
        {
            return Err(fail());
        }
        let gate = self.execution_gate.as_ref().ok_or_else(fail)?;
        let hook = self.spawn.as_ref().ok_or_else(fail)?;
        let pending = checkpoint.pending.clone();
        let reason = if checkpoint.invocation.discipline.handoff_tool.as_deref()
            == Some(&pending.tool_name)
        {
            tetonic_domain::SuspensionReason::HumanInput
        } else {
            return Err(fail());
        };
        gate.suspend(&checkpoint, reason)
            .await
            .map_err(|_| fail())?;
        let outcome = hook(pending.clone(), convo).await;
        // Capacity and current authority are reacquired before leaving the saved boundary.
        gate.resume().await.map_err(|_| fail())?;
        let result = outcome.to_model_string();
        if let Some(audit) = self.audit() {
            audit.tool_call(
                &pending.call_id,
                &pending.tool_name,
                &pending.arguments.to_string(),
                outcome.ok,
                &outcome.summary,
                outcome.error_kind.as_deref(),
            );
            audit.tool_message(&pending.tool_name, &pending.call_id, &result);
        }
        on_step(Step::ToolResult {
            call_id: pending.call_id.clone(),
            name: pending.tool_name.clone(),
            ok: outcome.ok,
            summary: outcome.summary,
        });
        convo
            .messages
            .push(Message::tool(pending.tool_name, result).with_tool_call_id(pending.call_id));
        Ok(())
    }
}
