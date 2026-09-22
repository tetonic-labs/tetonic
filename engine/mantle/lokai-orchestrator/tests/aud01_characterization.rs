//! AUD-01 orchestrator characterization. Heuristic routing is not PRESERVE.

use lokai_core::AgentConfig;
use lokai_orchestrator::{
    parse_critic_verdict, specialist_agent_config, AgentBuildRequest, CriticOutcome, RoleId,
    SpecialistPack, ROOT_AGENT,
};

struct CodingPinPack;

impl SpecialistPack for CodingPinPack {
    fn parse(&self, s: &str) -> Option<RoleId> {
        match s {
            "planner" | "plan" => Some(RoleId::new("planner")),
            "debugger" | "debug" => Some(RoleId::new("debugger")),
            "critic" => Some(RoleId::new("critic")),
            "reviewer" | "review" => Some(RoleId::new("reviewer")),
            "coder" | "code" => Some(RoleId::new("coder")),
            _ => None,
        }
    }
    fn default_role(&self) -> RoleId {
        RoleId::new("coder")
    }
    fn critic_role(&self) -> RoleId {
        RoleId::new("critic")
    }
    fn revision_role(&self) -> RoleId {
        RoleId::new("coder")
    }
    fn overlay(&self, role: &RoleId) -> String {
        match role.as_str() {
            "planner" => "Do NOT edit files — call finish with the plan.".into(),
            "debugger" => "You are the debugger specialist.".into(),
            "reviewer" | "critic" => "Call finish with APPROVE or REVISE: <issues>.".into(),
            _ => String::new(),
        }
    }
    fn allowed_tools(&self, role: &RoleId) -> Option<Vec<String>> {
        match role.as_str() {
            "planner" | "reviewer" | "critic" => Some(vec![
                "read_file".into(),
                "search_code".into(),
                "finish".into(),
            ]),
            "debugger" => Some(vec!["edit_file".into(), "read_file".into()]),
            _ => None,
        }
    }
    fn spawn_allowed_tools(&self, role: &RoleId) -> Vec<String> {
        self.allowed_tools(role).unwrap_or_default()
    }
    fn should_run_critic(&self, role: &RoleId) -> bool {
        !matches!(role.as_str(), "planner" | "reviewer" | "critic")
    }
    fn max_steps(&self, _role: &RoleId, base: usize) -> usize {
        base
    }
    fn explain_turn(&self, role: &RoleId, base: bool) -> bool {
        match role.as_str() {
            "planner" | "reviewer" | "critic" => true,
            _ => base,
        }
    }

    fn root_explain_turn(&self, user_text: &str) -> bool {
        let _ = user_text;
        false
    }
}

fn turn_src() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/turn.rs"))
}

fn specialist_src() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/specialist.rs"))
}

#[test]
fn aud01_bh_resp_planner_explain_turn_via_pack() {
    let cfg = specialist_agent_config(
        &AgentConfig::default(),
        &CodingPinPack,
        &RoleId::new("planner"),
        "a0_s0",
    );
    assert!(cfg.explain_turn);
    assert!(cfg.system_overlay.unwrap().contains("Do NOT edit"));
}

#[test]
fn aud01_bh_plan_role_overlay_wiring() {
    let overlay = CodingPinPack.overlay(&RoleId::new("planner"));
    assert!(overlay.contains("Do NOT edit"));
    let tools = CodingPinPack
        .allowed_tools(&RoleId::new("planner"))
        .unwrap();
    assert!(!tools.contains(&"edit_file".into()));
    let src = specialist_src();
    assert!(src.contains("Do NOT edit files"));
}

#[test]
fn aud01_bh_invest_read_search_subset() {
    let tools = CodingPinPack
        .allowed_tools(&RoleId::new("planner"))
        .unwrap();
    assert!(tools.contains(&"search_code".into()));
    assert!(tools.contains(&"read_file".into()));
}

#[test]
fn aud01_bh_debug_debugger_role_mode() {
    assert_eq!(
        CodingPinPack.parse("debugger").unwrap(),
        RoleId::new("debugger")
    );
    let src = specialist_src();
    assert!(src.contains("\"debugger\""));
}

#[test]
fn aud01_bh_review_critic_verdict_approve_revise() {
    assert_eq!(parse_critic_verdict("APPROVE"), CriticOutcome::Approved);
    assert!(matches!(
        parse_critic_verdict("REVISE: missing edge case"),
        CriticOutcome::Revise(_)
    ));
}

#[test]
fn aud01_bh_orch_root_mode_agent_build_request_explain_turn_field() {
    // PRESERVE workflow. Direct Agent::turn owner is DEFECT (WORK-02).
    let req = AgentBuildRequest {
        agent_id: ROOT_AGENT.into(),
        role: None,
        dynamic_spec: None,
        use_hard_model: false,
        orchestration_tools: false,
        max_steps: None,
        explain_turn: true,
        spawned: false,
        inherited_workspace_version: None,
    };
    assert!(req.explain_turn);
    let src = turn_src();
    assert!(src.contains("agent"));
    assert!(src.contains("root_execute"));
    assert!(src.contains("ChildJob"));
}

#[test]
fn aud01_bh_orch_crit_mode_extra_turn() {
    // PRESERVE critic workflow. Extra Agent::turn owner is DEFECT (WORK-02).
    let src = turn_src();
    assert!(src.contains("critic_role()"));
    assert!(src.contains("admit_child"));
}

#[test]
fn aud01_bh_orch_rev_mode_revision_turn() {
    // PRESERVE revision workflow. Extra Agent::turn owner is DEFECT (WORK-02).
    let src = turn_src();
    assert!(src.contains("revision_ran"));
    assert!(src.contains("revision"));
}

#[test]
fn aud01_bh_orch_spawn_mode_nested_specialist() {
    // PRESERVE nested specialist workflow. Post-hoc AddTask / Agent::turn owner DEFECT.
    let src = turn_src();
    assert!(src.contains("pub async fn run_spawned_specialist"));
}

#[test]
fn aud01_current_heuristic_routes_plan_to_planner() {
    // Current classifier, not PRESERVE contract.
    let d = lokai_orchestrator::route_task(
        "Plan the refactor for the auth module",
        true,
        &CodingPinPack,
    );
    match d.mode {
        lokai_orchestrator::RouteMode::Specialist(role) => {
            assert_eq!(role, RoleId::new("planner"));
        }
        other => panic!("current heuristic routes plan text to planner, got {other:?}"),
    }
}
