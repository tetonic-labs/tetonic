//! One local `AgentAttemptExecutor` (WORK-01). First placement; not a registry.

use async_trait::async_trait;
use lokai_core::{Agent, Conversation, Step};
use lokai_domain::{
    AgentAttemptExecutor, AgentInvocation, AttemptExecutionContext, CandidateOutcome,
};

/// Holds the live `Agent` and leftover Conversation. Does not construct a fresh
/// conversation (that would clone `Agent::run`).
pub struct LocalAgentAttemptExecutor<'a, F>
where
    F: FnMut(Step) + Send,
{
    agent: &'a mut Agent,
    conversation: &'a mut Conversation,
    on_step: F,
}

impl<'a, F> LocalAgentAttemptExecutor<'a, F>
where
    F: FnMut(Step) + Send,
{
    pub fn new(agent: &'a mut Agent, conversation: &'a mut Conversation, on_step: F) -> Self {
        Self {
            agent,
            conversation,
            on_step,
        }
    }
}

#[async_trait]
impl<F> AgentAttemptExecutor for LocalAgentAttemptExecutor<'_, F>
where
    F: FnMut(Step) + Send,
{
    async fn execute(
        &mut self,
        invocation: AgentInvocation,
        ctx: AttemptExecutionContext,
    ) -> CandidateOutcome {
        if let Some(existing) = self.agent.bound_attempt_id() {
            if existing != ctx.attempt_id.0 {
                return CandidateOutcome::Failed {
                    message: format!(
                        "Attempt binding mismatch: agent is bound to attempt '{existing}', cannot execute for '{}'",
                        ctx.attempt_id.0
                    ),
                };
            }
        } else {
            self.agent.stamp_attempt_id(&ctx.attempt_id.0);
        }
        self.agent
            .turn(self.conversation, invocation, &mut self.on_step)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_core::AgentConfig;
    use lokai_domain::{
        ActionKind, AttemptId, AuthorizedAction, ToolAdvertisement, ToolHost, ToolOutcome,
        ToolProposal,
    };
    use lokai_inference::{
        ChatRequest, ChatResponse, InferenceError, InferenceProvider, Message, TokenSink,
    };
    use serde_json::Value;
    use std::sync::Arc;

    struct DummyHost;

    impl ToolHost for DummyHost {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(DummyHost)
        }
        fn propose(&self, _name: &str, _args: &Value) -> Option<ToolProposal> {
            None
        }
        fn is_tool_allowed(&self, _name: &str) -> bool {
            true
        }
        fn is_read_only(&self, _name: &str) -> bool {
            true
        }
        fn advertisements(&self) -> Vec<ToolAdvertisement> {
            vec![]
        }
        fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
            Ok(())
        }
        fn execute_authorized(
            &self,
            _name: &str,
            _args: &Value,
            _auth: Option<&AuthorizedAction>,
            _cancel: &lokai_domain::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            let _ = ActionKind::ReadFile;
            ToolOutcome::ok("ok", "ok")
        }
    }

    struct NoChatProvider;

    #[async_trait]
    impl InferenceProvider for NoChatProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            panic!("empty-instruction turn must not chat");
        }
    }

    #[tokio::test]
    async fn work01_local_executor_delegates_to_turn() {
        let mut agent = Agent::new(Arc::new(NoChatProvider), DummyHost, AgentConfig::default());
        let mut conversation =
            Conversation::from_audit_messages(vec![Message::user("keep existing")]);
        let before = conversation.len();
        assert!(!conversation.is_empty());
        let mut executor = LocalAgentAttemptExecutor::new(&mut agent, &mut conversation, |_| {});
        let outcome = executor
            .execute(
                AgentInvocation {
                    instructions: String::new(),
                    user_input: "later".into(),
                    explain_turn: false,
                    empty_tool_nudge: false,
                    max_steps: 8,
                    completion_tool: "finish".into(),
                    discipline: lokai_domain::LoopDiscipline::default(),
                },
                AttemptExecutionContext {
                    attempt_id: AttemptId::new("att_work01"),
                },
            )
            .await;
        assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
        assert_eq!(conversation.len(), before);
        assert!(!conversation.is_empty());
    }

    #[tokio::test]
    async fn work05_executor_rejects_attempt_binding_mismatch() {
        let mut agent = Agent::new(Arc::new(NoChatProvider), DummyHost, AgentConfig::default());
        agent.stamp_attempt_id("att_bound_first");
        let mut conversation = Conversation::new();
        let mut executor = LocalAgentAttemptExecutor::new(&mut agent, &mut conversation, |_| {});
        let outcome = executor
            .execute(
                AgentInvocation {
                    instructions: String::new(),
                    user_input: "test".into(),
                    explain_turn: false,
                    empty_tool_nudge: false,
                    max_steps: 8,
                    completion_tool: "finish".into(),
                    discipline: lokai_domain::LoopDiscipline::default(),
                },
                AttemptExecutionContext {
                    attempt_id: AttemptId::new("att_different_second"),
                },
            )
            .await;
        match outcome {
            CandidateOutcome::Failed { message, .. } => {
                assert!(message.contains("Attempt binding mismatch"));
                assert!(message.contains("att_bound_first"));
                assert!(message.contains("att_different_second"));
            }
            other => panic!("expected failure on attempt mismatch, got {other:?}"),
        }
    }
}
