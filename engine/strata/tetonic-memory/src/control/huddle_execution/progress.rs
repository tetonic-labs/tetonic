//! A consistent read of existing work/run truth for the team controller.
use super::*;
use tetonic_domain::{AttemptState, RunSnapshot, RunState, TaskState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentState {
    NotStarted,
    Executing,
    WaitingHuman,
    Suspended,
    Completed,
    NeedsAttention,
}

#[derive(Clone, Debug)]
pub struct AssignmentProgress {
    pub pin: PlanAgentPin,
    pub state: AssignmentState,
    /// The existing registered-child admission projection, not a second capacity calculation.
    pub holds_capacity: bool,
}

#[derive(Clone, Debug)]
pub struct HuddleProgress {
    pub receipt: HuddleExecution,
    pub run_id: Option<String>,
    pub assignments: Vec<AssignmentProgress>,
}

impl Store {
    pub fn huddle_progress(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
        now: u64,
    ) -> Result<HuddleProgress> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let receipt = self
            .huddle_execution(actor, org, team, source)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let root = self
            .get_team_work_item(org, team, &receipt.root_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let run = root
            .run_id
            .as_deref()
            .map(|id| self.load_run_snapshot(id))
            .transpose()?
            .flatten();
        let mut capacity = std::collections::HashMap::new();
        if let Some(run) = &run {
            let mut query = self.conn.prepare(
                "SELECT task_id,execution_held FROM registered_child_capacity WHERE run_id=?1",
            )?;
            for row in query.query_map([&run.run_id.0], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            })? {
                let (task, held) = row?;
                capacity.insert(task, held);
            }
        }
        let mut assignments = Vec::new();
        for pin in &receipt.assignments {
            let work = self
                .get_team_work_item(org, team, &pin.work_id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            let mut state = assignment_state(&work, run.as_ref(), pin, root.attempt_id.as_deref());
            let mut holds_capacity = false;
            if let Some(run) = &run {
                if let Some(task) = work
                    .attempt_id
                    .as_ref()
                    .and_then(|id| run.attempts.get(&tetonic_domain::AttemptId::new(id)))
                    .and_then(|a| run.tasks.get(&a.task_id))
                {
                    holds_capacity = *capacity
                        .get(&task.task_id.0)
                        .ok_or(StoreError::ControlAccessDenied)?;
                    let scope = task
                        .binding
                        .execution_scope
                        .as_ref()
                        .ok_or(StoreError::ControlAccessDenied)?;
                    if scope.organization_id != org
                        || !self.context_access_in_organization(
                            actor,
                            &scope.information_context_id,
                            org,
                        )?
                    {
                        return Err(StoreError::ControlAccessDenied);
                    }
                }
            }
            if matches!(
                state,
                AssignmentState::Executing | AssignmentState::Suspended
            ) {
                let waiting = self
                    .work_human_questions(actor, org, team, &pin.work_id)?
                    .iter()
                    .any(|q| {
                        q.answer.is_none() && self.validate_human_wait(org, team, q, now).is_ok()
                    });
                if waiting {
                    state = AssignmentState::WaitingHuman;
                }
            }
            assignments.push(AssignmentProgress {
                pin: pin.clone(),
                state,
                holds_capacity,
            });
        }
        let progress = HuddleProgress {
            receipt,
            run_id: root.run_id,
            assignments,
        };
        tx.commit()?;
        Ok(progress)
    }
}

pub(super) fn assignment_state(
    work: &crate::TeamWorkItem,
    run: Option<&RunSnapshot>,
    pin: &PlanAgentPin,
    parent_attempt: Option<&str>,
) -> AssignmentState {
    use AssignmentState::*;
    if work.run_id.is_none() && work.attempt_id.is_none() {
        return if work.status == "open" {
            NotStarted
        } else {
            NeedsAttention
        };
    }
    let Some(run) = run.filter(|run| work.run_id.as_deref() == Some(run.run_id.0.as_str())) else {
        return NeedsAttention;
    };
    let Some(a) = work
        .attempt_id
        .as_ref()
        .and_then(|id| run.attempts.get(&tetonic_domain::AttemptId::new(id)))
    else {
        return NeedsAttention;
    };
    let Some(t) = run.tasks.get(&a.task_id) else {
        return NeedsAttention;
    };
    if t.binding.delegation.as_ref().map_or(true, |d| {
        d.activation.request_id != crate::work_activation_request_id(&work.request_id)
            || Some(d.parent_attempt.0.as_str()) != parent_attempt
    }) || t.active_attempt.as_ref() != Some(&a.attempt_id)
        || t.binding.task_definition_version != a.task_version
        || t.binding
            .job_spec
            .as_ref()
            .map(|j| j.definition_digest.as_str())
            != Some(&pin.definition_digest)
    {
        return NeedsAttention;
    }
    if t.state == TaskState::Succeeded && a.state == AttemptState::Succeeded {
        Completed
    } else if run.state != RunState::Active || run.cancellation.run_canceled {
        NeedsAttention
    } else {
        match (&t.state, &a.state) {
            // Candidate success precedes verification/commit and artifact acceptance.
            // It remains executing until the task, not just its attempt, succeeds.
            (TaskState::Running, AttemptState::Running | AttemptState::Succeeded)
            | (TaskState::Leased, AttemptState::Leased | AttemptState::Starting) => Executing,
            (TaskState::Parked, AttemptState::Suspended)
                if a.execution_quiesced && a.suspension.is_some() =>
            {
                Suspended
            }
            _ => NeedsAttention,
        }
    }
}
