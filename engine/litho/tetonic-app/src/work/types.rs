use super::*;
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
    pub work_teams: Vec<WorkTeam>,
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
    pub work_team: Option<WorkTeam>,
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
