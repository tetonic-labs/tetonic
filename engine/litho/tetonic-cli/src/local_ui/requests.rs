//! Explicit HTTP request payloads and their authority boundary checks.
use serde::Deserialize;
use tetonic_app::local_workspace::WorkPurpose;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Submit {
    #[serde(default)]
    pub(super) work_team: Option<tetonic_app::local_workspace::WorkTeamSelection>,
    #[serde(default)]
    pub(super) purpose: WorkPurpose,
    #[serde(default)]
    pub(super) parent_id: Option<String>,
    pub(super) request_id: String,
    pub(super) input: String,
    #[serde(default)]
    pub(super) agent_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateWorkItemRequest {
    #[serde(default)]
    pub(super) notes: Option<Vec<String>>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) lead_id: Option<String>,
    #[serde(default)]
    pub(super) agent_ids: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_app::local_workspace::{
        BudgetSettingsRequest, CreateLocalAgent, PlanCommand, SaveWorkBrief,
    };
    #[test]
    fn budget_settings_cannot_supply_usage_or_execution_authority() {
        let command =
            serde_json::json!({"request_id":"id","expected_revision":0,"token_limit":1024});
        assert!(serde_json::from_value::<BudgetSettingsRequest>(command.clone()).is_ok());
        let mut reset = command.clone();
        reset["token_limit"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<BudgetSettingsRequest>(reset)
            .unwrap()
            .token_limit
            .is_none());
        let mut missing = command.clone();
        missing.as_object_mut().unwrap().remove("token_limit");
        assert!(serde_json::from_value::<BudgetSettingsRequest>(missing).is_err());
        for field in [
            "used_tokens",
            "released_tokens",
            "work_id",
            "team_id",
            "actor",
            "grant",
        ] {
            let mut invalid = command.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<BudgetSettingsRequest>(invalid).is_err());
        }
    }
    #[test]
    fn plan_commands_cannot_accept_execution_settings_or_dispatch_actions() {
        let command = serde_json::json!({"action":"generate","request_id":"id","expected_revision":0,"brief_revision":1});
        assert!(serde_json::from_value::<PlanCommand>(command.clone()).is_ok());
        for key in ["grant", "execute", "tools", "credential", "workspace_root"] {
            let mut bad = command.clone();
            bad[key] = serde_json::json!(true);
            assert!(serde_json::from_value::<PlanCommand>(bad).is_err());
        }
        assert!(serde_json::from_value::<PlanCommand>(
            serde_json::json!({"action":"dispatch","revision":1})
        )
        .is_err());
    }
    #[test]
    fn shaping_payloads_are_explicit_and_cannot_supply_execution_authority() {
        let mut value =
            serde_json::json!({"request_id":"id","input":"Explore this","agent_key":"The Guide"});
        assert_eq!(
            serde_json::from_value::<Submit>(value.clone())
                .unwrap()
                .purpose,
            WorkPurpose::Work
        );
        value["purpose"] = serde_json::json!("explore");
        assert_eq!(
            serde_json::from_value::<Submit>(value.clone())
                .unwrap()
                .purpose,
            WorkPurpose::Explore
        );
        value["purpose"] = serde_json::json!("anything");
        assert!(serde_json::from_value::<Submit>(value).is_err());
        let brief =
            serde_json::json!({"request_id":"id","expected_revision":0,"body":"Draft only"});
        assert!(serde_json::from_value::<SaveWorkBrief>(brief.clone()).is_ok());
        for field in ["dispatch", "grant", "team_id", "actor"] {
            let mut invalid = brief.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<SaveWorkBrief>(invalid).is_err());
        }
    }
    #[test]
    fn agent_requests_reject_unimplemented_authority_fields() {
        let value = serde_json::json!({"request_id":"id","name":"Analyst","purpose":"Think",
            "model":"installed","harness":"general","max_steps":2,"max_seconds":60,"max_tokens":1024});
        assert!(serde_json::from_value::<CreateLocalAgent>(value.clone()).is_ok());
        for field in [
            "tool_ids",
            "permissions",
            "workspace_path",
            "principal",
            "team_id",
        ] {
            let mut invalid = value.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<CreateLocalAgent>(invalid).is_err());
        }
    }
    #[test]
    fn blackboard_inspection_query_accepts_scope_but_not_caller_authority() {
        use tetonic_app::local_workspace::BlackboardQuery;
        let value = serde_json::json!({"offset":20,"work_ids":["work"]});
        let query = serde_json::from_value::<BlackboardQuery>(value.clone()).unwrap();
        assert!(query.validate().is_ok());
        for field in ["actor", "org", "team", "agent_id", "audience"] {
            let mut invalid = value.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<BlackboardQuery>(invalid).is_err());
        }
        let query = serde_json::from_value::<BlackboardQuery>(serde_json::json!({"offset":100001}))
            .unwrap();
        assert!(query.validate().is_err());
    }
}
