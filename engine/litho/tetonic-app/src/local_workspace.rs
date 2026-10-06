//! Single-owner, local UI composition. The HTTP adapter never supplies host settings,
//! identities, resource paths or grants. All work uses the existing managed runtime.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tetonic_domain::{DataClass, ExecutionScope, RunState, TaskState};

use crate::errors::AppError;
use crate::job_launch::{prepare_launch, PreparedLaunch, RegisteredLaunchHost};
use crate::resources::{
    HarnessPreparationLimits, LocalControl, RegisteredExecutionSettings, TeamWorkLaunch,
};

const OWNER: &str = "local-ui-owner";
const ORG: &str = "local-ui";
const TEAM: &str = "local-work";
const AGENT: &str = "Local assistant";
const AUDIENCE: &str = "tetonic-local-ui-v1";
pub const INPUT_LIMIT: usize = 12_000;
mod agents;
mod conversations;
mod plan_execution;
mod plan_human;
pub use plan_human::{AmendPlanAssignment, AnswerPlanQuestion};
mod plans;
pub use plan_execution::{PlanExecutionView, PlanTaskLink, StartPlan};
mod providers;
mod shaping;
mod workroom;
pub use agents::{CreateLocalAgent, LocalAgent, LocalAgentCatalog};
pub use plans::PlanCommand;
pub use providers::{LocalProvider, RemoveProviderKey, SaveProviderKey};
pub use shaping::{SaveWorkBrief, WorkPurpose};
pub use workroom::BudgetSettingsRequest;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocalWorkItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub agent_key: Option<String>,
    pub goal_id: Option<String>,
    pub run_id: Option<String>,
    pub request_id: String,
    pub version: i64,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub lead_id: Option<String>,
    #[serde(default)]
    pub agent_ids: Option<Vec<String>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CreateWorkItemRequest {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub agent_key: Option<String>,
    #[serde(default)]
    pub lead_id: Option<String>,
    #[serde(default)]
    pub agent_ids: Option<Vec<String>>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LocalApprovalsInspection {
    pub active_stops: Vec<tetonic_memory::ControlStop>,
    pub pending_approvals: Vec<tetonic_memory::EffectApproval>,
    pub effort: Vec<tetonic_memory::TeamEffortEntry>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ResolveApprovalRequest {
    pub proposal_digest: String,
    pub allow: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct LocalTeamInfo {
    pub id: String,
    pub name: String,
    pub org_id: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LocalDigestResponse {
    pub summary: String,
    pub total_work_items: usize,
    pub completed_items: usize,
    pub active_items: usize,
    pub pending_approvals_count: usize,
    pub highlights: Vec<String>,
}

#[derive(Serialize)]
pub struct LocalWorkspaceSnapshot {
    pub usage: Vec<tetonic_memory::WorkUsage>,
    pub budget_setting: tetonic_memory::TeamBudgetSetting,
    pub budget_max_tokens: u64,
    pub shaping_agent_key: String,
    pub organization: String,
    pub team_id: String,
    pub team_name: String,
    pub agent_id: String,
    pub agent_name: String,
    pub model: String,
    pub input_limit: usize,
    pub agents: Vec<LocalAgent>,
    pub tasks: Vec<LocalTask>,
    pub planning_tasks: Vec<LocalTask>,
}

#[derive(Serialize)]
pub struct LocalTask {
    pub human_questions: Vec<tetonic_memory::WorkHumanQuestion>,
    pub plan: Option<PlanTaskLink>,
    pub planning_for: Option<String>,
    pub purpose: WorkPurpose,
    pub parent_id: Option<String>,
    pub error: Option<String>,
    pub id: String,
    pub input: String,
    pub agent_key: String,
    pub agent_name: String,
    pub state: String,
    pub run_id: Option<String>,
    pub sequence: u64,
    pub messages: Vec<LocalMessage>,
}

#[derive(Serialize)]
pub struct LocalMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
}

pub struct LocalWorkspace {
    local: LocalControl,
    host: PreparedLaunch,
    context: String,
    // Only admission is serialized. Inference and reads never hold this lock.
    admission: tokio::sync::Mutex<()>,
    keys: std::sync::Arc<providers::ProviderKeys>,
    #[cfg(test)]
    hosted_transport: Option<std::sync::Arc<dyn tetonic_inference::hosted::HostedTransport>>,
}

fn resource(error: crate::resources::ResourceError) -> AppError {
    AppError::InvalidRequest(error.to_string())
}

impl LocalWorkspace {
    /// Startup is a local owner operation, never an unauthenticated API endpoint.
    /// A dedicated database is required; existing unrelated principals are not adopted.
    pub async fn open(database: PathBuf, model: String, ollama: String) -> Result<Self, AppError> {
        Self::open_with_workspace(database, model, ollama, None).await
    }

    pub async fn open_with_workspace(
        database: PathBuf,
        model: String,
        ollama: String,
        workspace_root: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        let local = LocalControl::open(database.clone(), AUDIENCE.into())
            .await
            .map_err(resource)?;
        match local
            .bootstrap(OWNER.into(), ORG.into(), "My workspace".into())
            .await
        {
            Ok(()) | Err(crate::resources::ResourceError::Conflict) => {}
            Err(error) => return Err(resource(error)),
        }
        let credential = local
            .credentials()
            .issue(OWNER.into(), 86_400)
            .await
            .map_err(resource)?;
        let secret = credential.expose_secret();
        let resources = local.resources();
        resources
            .create_team(secret, ORG.into(), TEAM.into(), "Personal".into())
            .await
            .map_err(resource)?;

        let has_workspace = workspace_root.is_some();
        let allowed_tools: std::collections::HashSet<String> = if has_workspace {
            [
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "edit_file",
                "write_file",
                "outline",
                "search_code",
                "finish",
            ]
            .into_iter()
            .map(String::from)
            .collect()
        } else {
            Default::default()
        };

        let requested_tools: Vec<String> = if has_workspace {
            vec![
                "read_file".into(),
                "list_dir".into(),
                "grep".into(),
                "glob".into(),
                "edit_file".into(),
                "write_file".into(),
            ]
        } else {
            vec![]
        };

        let default_instructions = if has_workspace {
            "You are a local engineering assistant with access to the workspace. Inspect files, search code, list directories, and make bounded modifications to solve the owner's request. Call finish with your complete answer and summary when done."
        } else {
            "Help the owner think through their request. Answer clearly and concisely. Call finish with your complete answer as the summary. You have no web, filesystem, shell or external tools; do not claim to have performed actions or research."
        };

        match resources
            .register_agent(
                secret,
                ORG.into(),
                AGENT.into(),
                "general".into(),
                serde_json::json!({
                    "instructions": default_instructions,
                    "requested_tools": requested_tools
                }),
            )
            .await
        {
            Ok(_) => {}
            Err(crate::resources::ResourceError::Conflict) => {
                let _ = resources
                    .publish_agent_revision(
                        secret,
                        ORG.into(),
                        AGENT.into(),
                        "general".into(),
                        serde_json::json!({
                            "instructions": default_instructions,
                            "requested_tools": requested_tools
                        }),
                    )
                    .await;
            }
            Err(e) => return Err(resource(e)),
        }
        let default_personas = [
            (shaping::GUIDE, shaping::GUIDE_INSTRUCTIONS),
            ("The Digest Assistant", "You are The Digest Assistant, an executive synthesis assistant. Provide high-level status standups, highlight completed milestones, and answer progress queries."),
            ("Researcher", "You are the Team Researcher. Investigate questions, synthesize facts, and structure comprehensive briefings using workspace inspection tools."),
            ("Analyst", "You are the Team Analyst. Evaluate plans, identify quantitative tradeoffs, critique proposals, and review execution quality."),
        ];
        for (name, instructions) in default_personas {
            let persona_tools = if name == shaping::GUIDE {
                vec![]
            } else {
                requested_tools.clone()
            };
            if resources
                .register_agent(
                    secret,
                    ORG.into(),
                    name.into(),
                    "general".into(),
                    serde_json::json!({
                        "instructions": instructions,
                        "requested_tools": persona_tools,
                    }),
                )
                .await
                .is_err()
            {
                let _ = resources
                    .publish_agent_revision(
                        secret,
                        ORG.into(),
                        name.into(),
                        "general".into(),
                        serde_json::json!({
                            "instructions": instructions,
                            "requested_tools": persona_tools,
                        }),
                    )
                    .await;
            }
        }
        let context = local
            .contexts()
            .team_participation_context(secret, ORG.into(), TEAM.into())
            .await
            .map_err(resource)?;
        let host = prepare_launch(
            secret,
            RegisteredLaunchHost {
                database,
                audience: AUDIENCE.into(),
                ollama,
                settings: RegisteredExecutionSettings {
                    plan_dispatch: None,
                    response_schema: None,
                    hosted: None,
                    max_elapsed_seconds: 120,
                    reported_token_ceiling: Some(4096),
                    workspace_root,
                    model,
                    num_ctx: 8192,
                    data_class: DataClass::Secret,
                    allowed_tools,
                    limits: HarnessPreparationLimits {
                        human_handoff: false,
                        max_steps: if has_workspace { 8 } else { 4 },
                        max_input_bytes: INPUT_LIMIT,
                    },
                },
            },
        )
        .await?;
        let keys = std::sync::Arc::new(providers::ProviderKeys {
            store: host
                .app
                .turn
                .store
                .clone()
                .ok_or(AppError::InferenceUnavailable)?,
            vault: std::sync::Arc::new(tetonic_secrets::key_storage::PlatformKeyStorage),
        });
        Ok(Self {
            local,
            host,
            context,
            admission: tokio::sync::Mutex::new(()),
            keys,
            #[cfg(test)]
            hosted_transport: None,
        })
    }

    pub async fn snapshot(&self) -> Result<LocalWorkspaceSnapshot, AppError> {
        let resources = self.local.resources();
        let team = resources
            .get_team(&self.host.credential, ORG.into(), TEAM.into())
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("local team unavailable".into()))?;
        let agent = resources
            .get_agent(&self.host.credential, ORG.into(), AGENT.into())
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("local agent unavailable".into()))?;
        let work = resources
            .list_team_work_items(&self.host.credential, ORG.into(), TEAM.into())
            .await
            .map_err(resource)?;
        let planning_ids = self.planning_ids().await?;
        let mut tasks = Vec::new();
        let mut planning_tasks = Vec::new();
        for item in work {
            // Planning runs belong in their huddle inspector, not as separate
            // top-level outcomes on the work map.
            if let Some(source) = planning_ids.get(&item.work_id) {
                let mut task = self.project_task(item).await?;
                task.planning_for = Some(source.clone());
                planning_tasks.push(task);
                continue;
            }
            tasks.push(self.project_task(item).await?);
        }
        Ok(LocalWorkspaceSnapshot {
            usage: self
                .local
                .resources()
                .team_work_usage(&self.host.credential, ORG.into(), TEAM.into())
                .await
                .map_err(resource)?,
            budget_setting: self
                .local
                .resources()
                .team_budget_setting(&self.host.credential, ORG.into(), TEAM.into())
                .await
                .map_err(resource)?,
            budget_max_tokens: self.host.settings.reported_token_ceiling.unwrap_or(4096),
            shaping_agent_key: shaping::GUIDE.into(),
            organization: "My workspace".into(),
            team_id: TEAM.into(),
            team_name: team.name,
            agent_id: agent.identity.identity_id,
            agent_name: AGENT.into(),
            model: self.host.settings.model.clone(),
            input_limit: INPUT_LIMIT,
            agents: self.agents().await?,
            tasks,
            planning_tasks,
        })
    }

    pub async fn task(&self, id: &str) -> Result<LocalTask, AppError> {
        validate_request_id(id)?;
        let work = self
            .local
            .resources()
            .get_team_work_item(&self.host.credential, ORG.into(), TEAM.into(), id.into())
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("task unavailable".into()))?;
        self.project_task(work).await
    }

    pub async fn submit(&self, id: String, input: String) -> Result<LocalTask, AppError> {
        self.submit_for_agent(id, input, AGENT.into()).await
    }

    pub async fn submit_for_agent(
        &self,
        id: String,
        input: String,
        agent_key: String,
    ) -> Result<LocalTask, AppError> {
        self.submit_in_conversation(id, input, agent_key, None)
            .await
    }

    pub async fn submit_in_conversation(
        &self,
        id: String,
        input: String,
        agent_key: String,
        parent_id: Option<String>,
    ) -> Result<LocalTask, AppError> {
        self.submit_with_purpose(id, input, agent_key, parent_id, WorkPurpose::Work)
            .await
    }

    pub async fn submit_with_purpose(
        &self,
        id: String,
        input: String,
        agent_key: String,
        parent_id: Option<String>,
        purpose: WorkPurpose,
    ) -> Result<LocalTask, AppError> {
        validate_request_id(&id)?;
        if agent_key == plan_execution::COORDINATOR {
            return Err(AppError::InvalidRequest(
                "Start an agreed plan to work with the team coordinator.".into(),
            ));
        }
        let input = input.trim().to_string();
        if input.is_empty() || input.len() > INPUT_LIMIT || input.contains('\0') {
            return Err(AppError::InvalidRequest(format!(
                "Enter a request of 1–{INPUT_LIMIT} UTF-8 bytes."
            )));
        }
        let _admission = self.admission.lock().await;
        let resources = self.local.resources();
        let registered = self.registered_agent(&agent_key).await?;
        let agent = self.agent_profile(agent_key.clone(), &registered)?;
        if purpose == WorkPurpose::Explore
            && (agent_key != shaping::GUIDE || !agent.tools.is_empty())
        {
            return Err(AppError::PolicyDenied(
                "Exploration requires the configured guide with no execution tools.".into(),
            ));
        }
        if let Some(parent) = &parent_id {
            if self.task(parent).await?.purpose != purpose {
                return Err(AppError::InvalidRequest("Keep exploration separate from execution. Start a new assignment when you are ready.".into()));
            }
        }
        let digest = registered.identity.bound_definition_digest;
        let request_id = if let Some(parent) = &parent_id {
            validate_request_id(parent)?;
            if parent == &id {
                return Err(AppError::InvalidRequest(
                    "A message cannot reply to itself.".into(),
                ));
            }
            format!("{id}@{agent_key}#{parent}")
        } else if agent_key == AGENT {
            id.clone()
        } else {
            format!("{id}@{agent_key}")
        };
        let title = if input.len() <= 120 && !input.contains('\n') {
            input.clone()
        } else {
            let first_line = input.lines().next().unwrap_or(&input).trim();
            let truncated = if first_line.len() > 100 {
                let mut end = 100;
                while !first_line.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &first_line[..end])
            } else {
                first_line.to_string()
            };
            if truncated.is_empty() {
                "Delegated Task".to_string()
            } else {
                truncated
            }
        };
        if let Some(existing) = resources
            .get_team_work_item(&self.host.credential, ORG.into(), TEAM.into(), id.clone())
            .await
            .map_err(resource)?
        {
            if existing.request_id != request_id
                || existing.purpose != purpose
                || existing.title != title
                || existing.input.as_deref() != Some(input.as_str())
            {
                return Err(AppError::InvalidRequest(
                    "This request ID already belongs to different input or a different agent."
                        .into(),
                ));
            }
            if existing.run_id.is_some() {
                return self.project_task(existing).await;
            }
        }
        let conversation_input = self
            .conversation_input(&id, &agent_key, parent_id.as_deref(), &input)
            .await?;
        let work = resources
            .create_team_work_item_for_purpose(
                &self.host.credential,
                ORG.into(),
                TEAM.into(),
                id.clone(),
                title,
                request_id,
                None,
                Some(input.clone()),
                purpose,
            )
            .await
            .map_err(resource)?;
        if work.run_id.is_some() {
            return self.project_task(work).await;
        }
        self.check_limits(agent.max_steps, agent.max_seconds, agent.max_tokens)?;
        let mut settings = self.host.settings.clone();
        if self.planning_ids().await?.contains_key(&id) {
            if purpose != WorkPurpose::Explore || agent_key != shaping::GUIDE || parent_id.is_some()
            {
                return Err(AppError::InvalidRequest("A planning request must use the configured Guide without conversation history.".into()));
            }
            settings.response_schema = Some(plans::response_schema());
        }
        if purpose == WorkPurpose::Explore {
            settings.allowed_tools.clear();
            settings.workspace_root = None;
        }
        if agent.provider == "ollama" {
            self.require_installed_model(&agent.model).await?;
        } else {
            settings.hosted = Some(self.hosted_binding(&agent).await?);
            settings.data_class = DataClass::SensitiveSource;
        }
        settings.model = agent.model;
        settings.max_elapsed_seconds = agent.max_seconds;
        settings.reported_token_ceiling = Some(agent.max_tokens);
        // New work inherits an explicit allowance from existing agent limits,
        // optionally narrowed by the owner's workspace default. Retries retain
        // the original funded amount even if defaults have since changed.
        let budget_id = id.clone();
        let budget = self
            .keys
            .store
            .read(move |db| db.work_budget_if_present(OWNER, ORG, TEAM, &budget_id))
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|e| resource(e.into()))?;
        if budget.is_none() {
            let setting = resources
                .team_budget_setting(&self.host.credential, ORG.into(), TEAM.into())
                .await
                .map_err(resource)?;
            let allowance = setting
                .token_limit
                .unwrap_or(agent.max_tokens as i64)
                .min(agent.max_tokens as i64);
            resources
                .authorize_work_budget(
                    &self.host.credential,
                    ORG.into(),
                    TEAM.into(),
                    id.clone(),
                    format!("allowance/{id}"),
                    allowance,
                )
                .await
                .map_err(resource)?;
        }
        if parent_id.is_some() {
            settings.limits.max_input_bytes = conversations::CONVERSATION_LIMIT;
        }
        let prepared = resources
            .prepare_general_revision(
                &self.host.credential,
                ORG.into(),
                agent_key.clone(),
                digest.clone(),
                conversation_input.clone(),
                settings.limits.clone(),
            )
            .await
            .map_err(resource)?;
        let grant_id = format!("local-ui-{id}");
        if resources
            .get_execution_grant(&self.host.credential, ORG.into(), grant_id.clone())
            .await
            .map_err(resource)?
            .is_none()
        {
            resources
                .issue_execution_grant(
                    &self.host.credential,
                    tetonic_memory::ExecutionGrant {
                        grant_id: grant_id.clone(),
                        scope: ExecutionScope {
                            principal_id: OWNER.into(),
                            organization_id: ORG.into(),
                            information_context_id: self.context.clone(),
                        },
                        job: prepared
                            .start_command(id.clone())
                            .map_err(resource)?
                            .job_spec,
                        expires_at: chrono::Utc::now().timestamp() + 3600,
                    },
                )
                .await
                .map_err(resource)?;
        }
        let (work, submission) = self
            .host
            .app
            .activate_team_work(
                &self.host.credential,
                self.host.verifier.clone(),
                TeamWorkLaunch {
                    organization_id: ORG.into(),
                    team_id: TEAM.into(),
                    work_id: id.clone(),
                    information_context_id: self.context.clone(),
                    agent_key,
                    definition_digest: digest,
                    execution_grant_id: grant_id,
                    input: Some(conversation_input),
                    recovery_id: id,
                },
                settings,
            )
            .await?;
        // The shared Application owns execution. A disconnected browser cannot cancel it.
        if let Some(execution) = submission.execution {
            tokio::task::spawn_local(async move {
                let _ = execution.completion.await;
            });
        }
        self.project_task(work).await
    }

    pub async fn cancel(&self, id: &str) -> Result<LocalTask, AppError> {
        let task = self.task(id).await?;
        if let Some(run_id) = task.run_id {
            self.host
                .app
                .run_manager
                .managed()
                .cancel_run(&tetonic_domain::RunId::new(run_id))
                .await
                .map_err(|_| {
                    AppError::InvalidRequest("Cancellation could not be confirmed.".into())
                })?;
        }
        self.task(id).await
    }

    async fn project_task(
        &self,
        work: tetonic_memory::TeamWorkItem,
    ) -> Result<LocalTask, AppError> {
        let (request_id, parent_id) = conversations::split_parent(&work.request_id);
        let plan = self.plan_link(&work.work_id).await?;
        let key = plan
            .as_ref()
            .map(|p| p.agent_key.as_str())
            .unwrap_or_else(|| request_id.split_once('@').map_or(AGENT, |(_, key)| key));
        let stored = if let Some(link) = &plan {
            self.local
                .resources()
                .get_agent_revision(
                    &self.host.credential,
                    ORG.into(),
                    key.into(),
                    link.definition_digest.clone(),
                )
                .await
                .map_err(resource)?
                .ok_or(AppError::InferenceUnavailable)?
        } else {
            self.registered_agent(key).await?
        };
        let agent = self.agent_profile(key.into(), &stored)?;
        let context = plan
            .as_ref()
            .map(|p| p.information_context_id.clone())
            .unwrap_or_else(|| self.context.clone());
        let bound_attempt = work.attempt_id.clone();
        let mut task = LocalTask {
            human_questions: vec![],
            plan,
            planning_for: None,
            purpose: work.purpose,
            parent_id: parent_id.map(str::to_owned),
            error: None,
            id: work.work_id,
            input: work.input.unwrap_or(work.title),
            agent_key: agent.key,
            agent_name: agent.name,
            state: "not_started".into(),
            run_id: work.run_id,
            sequence: 0,
            messages: vec![],
        };
        let Some(run) = task.run_id.clone() else {
            return Ok(task);
        };
        let contexts = self.local.contexts();
        let snapshot = contexts
            .inspect_run(&self.host.credential, ORG.into(), context.clone(), run)
            .await
            .map_err(resource)?;
        task.sequence = snapshot.sequence;
        // A child can finish while its parent is still active. Never substitute a
        // sibling's status, transcript or artifact for this work item's attempt.
        let attempt = bound_attempt.as_ref().and_then(|id| {
            snapshot
                .attempts
                .get(&tetonic_domain::AttemptId::new(id.clone()))
        });
        let bound_task = attempt.and_then(|a| snapshot.tasks.get(&a.task_id));
        task.state = match (&snapshot.state, bound_task.map(|t| &t.state)) {
            (_, Some(TaskState::Succeeded)) => "completed",
            (_, Some(TaskState::Failed | TaskState::Skipped)) => "failed",
            (_, Some(TaskState::Canceled)) => "canceled",
            (RunState::Canceled, _) => "canceled",
            (RunState::Canceling, _) => "canceling",
            (RunState::Failed, _) => "failed",
            (RunState::RecoveryRequired, _) => "recovery_required",
            (_, Some(TaskState::Running)) => "running",
            _ => "starting",
        }
        .into();
        if task.plan.is_some() {
            // Older finite-plan records used the generic retry policy but had
            // no retry owner. Show the terminal bound attempt, not false Starting.
            if task.state != "completed" {
                task.state = match attempt.map(|a| &a.state) {
                    Some(
                        tetonic_domain::AttemptState::TimedOut
                        | tetonic_domain::AttemptState::Failed,
                    ) => "failed".into(),
                    Some(tetonic_domain::AttemptState::Canceled) => "canceled".into(),
                    Some(
                        tetonic_domain::AttemptState::LeaseExpired
                        | tetonic_domain::AttemptState::Superseded,
                    ) => "recovery_required".into(),
                    _ => task.state,
                };
            }
            let work_id = task.id.clone();
            task.human_questions = self
                .keys
                .store
                .read(move |db| db.work_human_questions(OWNER, ORG, TEAM, &work_id))
                .await
                .map_err(|_| AppError::InferenceUnavailable)?
                .map_err(|e| resource(e.into()))?;
            if task.state == "running"
                && task.human_questions.iter().any(|q| {
                    q.answer.is_none()
                        && bound_attempt.as_deref() == Some(q.attempt_id.as_str())
                        && q.deadline > chrono::Utc::now().timestamp() as u64
                })
            {
                task.state = "waiting_human".into();
            }
        }
        if task.state == "failed" {
            task.error = Some(
                task_failure_message(attempt.and_then(|a| a.failure_reason.as_deref())).into(),
            );
        }
        if let Some(history) = bound_task
            .and_then(|t| {
                t.binding
                    .activation
                    .as_ref()
                    .or_else(|| t.binding.delegation.as_ref().map(|d| &d.activation))
            })
            .map(|a| a.audit_session_id.clone())
        {
            let history = contexts
                .transcript(&self.host.credential, context.clone(), history, 100)
                .await
                .map_err(resource)?;
            // Delegation work stores its title; the scoped audit contains the
            // exact admitted input, including dependency contributions.
            if task.plan.is_some() {
                if let Some((_, _, input)) = history.iter().find(|(_, role, _)| role == "user") {
                    task.input = input.clone();
                }
            }
            task.messages = history
                .into_iter()
                .filter(|(_, role, content)| {
                    (role == "assistant" || role == "tool") && !content.trim().is_empty()
                })
                .map(|(id, role, content)| LocalMessage { id, role, content })
                .collect();
        }
        // `finish` may contain the entire answer in its arguments, with no assistant
        // text in the transcript. The accepted artifact is the durable outcome source.
        if task.state == "completed" {
            if let Some(output) = bound_task.and_then(|t| t.accepted_artifact.as_ref()) {
                let artifacts = contexts
                    .bind_artifacts(
                        &self.host.credential,
                        context.clone(),
                        self.host.app.turn.runtime.artifact_store().clone(),
                    )
                    .await
                    .map_err(resource)?;
                let id = tetonic_domain::ArtifactId::new(output.artifact_id.clone());
                if let Ok(mut reader) = artifacts.open(&id).await {
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    let mut read_ok = true;
                    loop {
                        match reader.read_chunk(&mut buffer).await {
                            Ok(0) => break,
                            Ok(read) => {
                                if bytes.len() + read > 65_536 {
                                    read_ok = false;
                                    break;
                                }
                                bytes.extend_from_slice(&buffer[..read]);
                            }
                            Err(_) => {
                                read_ok = false;
                                break;
                            }
                        }
                    }
                    use sha2::Digest;
                    if read_ok
                        && format!("sha256:{:x}", sha2::Sha256::digest(&bytes)) == output.digest
                    {
                        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                            if value["v"] == 1 && value["outcome"] == "completed" {
                                if let Some(answer) =
                                    value["message"].as_str().filter(|s| !s.trim().is_empty())
                                {
                                    let has_assistant = task.messages.iter().any(|message| {
                                        message.role == "assistant" && message.content == answer
                                    });
                                    if !has_assistant {
                                        task.messages.push(LocalMessage {
                                            id: -1,
                                            role: "assistant".into(),
                                            content: answer.into(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(task)
    }
}

// Do not expose provider response bodies, tool output or internal store errors.
fn task_failure_message(reason: Option<&str>) -> &'static str {
    let reason = reason.unwrap_or_default();
    if reason.contains("token allowance") || reason.contains("complete token usage") {
        return "Work stopped at its token allowance, or usage could not be confirmed. Open Usage to review the recorded amount and any held allowance.";
    }
    if reason.contains("requested model residency unavailable") {
        "The local inference server could not confirm the requested model was loaded. Completed contributions are saved. Check the model server before starting more work."
    } else if reason.contains("hosted HTTP status 401") || reason.contains("hosted HTTP status 403")
    {
        "The provider rejected access. Check the saved API key and this model's permissions."
    } else if reason.contains("hosted HTTP status 429") {
        "The provider's rate or usage limit was reached. Check your account and try again later."
    } else if reason.contains("hosted HTTP status 400") || reason.contains("hosted HTTP status 404")
    {
        "The provider could not accept this model request. Check the model ID and tool-calling support."
    } else if reason == "execution deadline exceeded" {
        "The run reached its time limit. Narrow the request or review the agent's available run limits before trying again."
    } else if reason.contains("secret") || reason.contains("disclosure policy") {
        "The engine blocked this prompt's disclosure. Remove sensitive credentials before trying again."
    } else if reason.contains("credential unavailable") {
        "The provider key is unavailable. Save it again in agent setup."
    } else {
        "The run could not finish. Check the model connection and the agent's run limits before trying again."
    }
}

fn validate_request_id(id: &str) -> Result<(), AppError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(AppError::InvalidRequest("invalid request id".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
