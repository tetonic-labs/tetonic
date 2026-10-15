//! Let the current attempt repair known blockers before it leaves the loop.
//! Readiness is not approval or publication; those remain finalization's job.
use super::*;

impl Agent {
    pub(super) async fn completion_ready(
        &self,
        convo: &mut Conversation,
        monitor: &mut HeuristicMonitor,
        call: Option<(&str, &str, &str)>,
        on_step: &mut (impl FnMut(Step) + Send),
    ) -> Result<bool, CandidateOutcome> {
        self.authorize_completion(convo).await?;
        let cancel = self.work_scope.cancellation_signal();
        let result = self
            .run_tools_blocking(move |tools| tools.check_completion(&cancel))
            .await;
        // Revocation/cancellation during a host check wins over a ready result.
        self.authorize_completion(convo).await?;
        let result = result.map_err(|_| CandidateOutcome::Failed {
            message: "Could not confirm completion readiness".into(),
        })?;
        let Err(blocker) = result else {
            return Ok(true);
        };
        let feedback = format!("Completion blocked: {}", blocker.message);
        if let Some((name, id, args)) = call {
            if let Some(audit) = self.audit() {
                audit.tool_call(id, name, args, false, &blocker.message, Some(&blocker.kind));
                audit.tool_message(name, id, &feedback);
            }
            on_step(Step::ToolResult {
                call_id: id.into(),
                name: name.into(),
                ok: false,
                summary: blocker.message,
            });
            convo
                .messages
                .push(Message::tool(name, feedback).with_tool_call_id(id));
        } else {
            if let Some(audit) = self.audit() {
                audit.message("user", &feedback, None);
            }
            on_step(Step::Note(blocker.message));
            convo.messages.push(Message::user(feedback));
        }
        if monitor.record_control_result(false) {
            let note = monitor.stuck_reason();
            if let Some(audit) = self.audit() {
                audit.note(note);
            }
            on_step(Step::Stopped(note.into()));
            return Err(CandidateOutcome::Limited {
                kind: LimitKind::NoProgress,
                message: note.into(),
            });
        }
        Ok(false)
    }

    async fn authorize_completion(&self, convo: &Conversation) -> Result<(), CandidateOutcome> {
        if convo.is_canceled() || self.work_scope.is_canceled() {
            return Err(CandidateOutcome::Canceled {
                reason: "canceled".into(),
            });
        }
        if !self.execution_authorized().await {
            return Err(CandidateOutcome::Failed {
                message: "execution authorization denied".into(),
            });
        }
        // The authorization gate itself can await external state.
        if convo.is_canceled() || self.work_scope.is_canceled() {
            return Err(CandidateOutcome::Canceled {
                reason: "canceled".into(),
            });
        }
        Ok(())
    }
}
