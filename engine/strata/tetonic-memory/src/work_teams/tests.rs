use super::*;

fn seed(path: &std::path::Path) -> Store {
    let db = Store::open(path).unwrap();
    db.bootstrap_control("owner", "org", "Org").unwrap();
    for team in ["space", "other"] {
        db.create_team(&crate::TeamRow {
            org_id: "org".into(),
            team_id: team.into(),
            name: team.into(),
            owner_principal_id: "owner".into(),
        })
        .unwrap();
    }
    for key in ["Mira", "Jun", "Sage"] {
        db.register_organization_agent(
            "owner",
            "org",
            key,
            "general",
            &serde_json::json!({"tools":["finish"],"model":"saved-model"}),
        )
        .unwrap();
    }
    db
}
fn request() -> SaveWorkTeam {
    SaveWorkTeam {
        id: "research".into(),
        request_id: "create".into(),
        expected_revision: 0,
        name: "Research".into(),
        purpose: "Investigate evidence".into(),
        agent_keys: vec!["Mira".into(), "Jun".into()],
    }
}
fn work(
    db: &Store,
    id: &str,
    selection: Option<&WorkTeamSelection>,
    parent: Option<&str>,
) -> Result<crate::TeamWorkItem> {
    db.create_rostered_work(
        crate::CreateTeamWorkItem {
            actor: "owner",
            org: "org",
            team: "space",
            work_id: id,
            title: "Explore",
            request_id: id,
            goal_id: None,
        },
        Some("Explore"),
        crate::WorkPurpose::Explore,
        selection,
        parent,
    )
}

#[test]
fn durable_rosters_pin_work_reject_stale_saves_and_never_change_agent_definitions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("teams.db");
    let db = seed(&path);
    let before = db
        .get_organization_agent("owner", "org", "Mira")
        .unwrap()
        .unwrap();
    let initial = db
        .save_work_team("owner", "org", "space", &request())
        .unwrap();
    let selection = WorkTeamSelection {
        id: initial.id.clone(),
        revision: initial.revision,
    };
    let first = work(&db, "first", Some(&selection), None).unwrap();
    let mut edit = request();
    edit.request_id = "edit".into();
    edit.expected_revision = 1;
    edit.agent_keys = vec!["Sage".into()];
    edit.name = "Evidence".into();
    db.save_work_team("owner", "org", "space", &edit).unwrap();
    assert_eq!(work(&db, "first", Some(&selection), None).unwrap(), first);
    assert!(work(&db, "new", Some(&selection), None).is_err());
    assert!(db
        .get_team_work_item("org", "space", "new")
        .unwrap()
        .is_none());
    assert_eq!(
        db.save_work_team("owner", "org", "space", &request())
            .unwrap(),
        initial
    );
    edit.request_id = "stale".into();
    assert!(db.save_work_team("owner", "org", "space", &edit).is_err());
    assert!(work(&db, "first", None, None).is_err());
    assert!(work(
        &db,
        "first",
        Some(&WorkTeamSelection {
            id: selection.id.clone(),
            revision: 2
        }),
        None
    )
    .is_err());
    work(&db, "reply", None, Some("first")).unwrap();
    assert_eq!(
        db.work_team_for_work("owner", "org", "space", "reply")
            .unwrap(),
        Some(initial.clone())
    );
    assert_eq!(
        db.get_organization_agent("owner", "org", "Mira")
            .unwrap()
            .unwrap()
            .definition_json,
        before.definition_json
    );
    assert!(db.work_teams("owner", "org", "other").unwrap().is_empty());
    db.register_control_principal("outsider").unwrap();
    assert!(db.work_teams("outsider", "org", "space").is_err());
    assert!(db
        .save_work_team("outsider", "org", "space", &request())
        .is_err());
    drop(db);
    let reopened = Store::open(path).unwrap();
    assert_eq!(
        reopened.work_teams("owner", "org", "space").unwrap()[0].revision,
        2
    );
    assert_eq!(
        reopened
            .work_team_for_work("owner", "org", "space", "first")
            .unwrap(),
        Some(initial)
    );
}

#[test]
fn out_of_roster_plans_and_unknown_agents_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let db = seed(&dir.path().join("teams.db"));
    let mut invalid = request();
    invalid.agent_keys.push("unknown".into());
    assert!(db
        .save_work_team("owner", "org", "space", &invalid)
        .is_err());
    db.save_work_team("owner", "org", "space", &request())
        .unwrap();
    work(
        &db,
        "shape",
        Some(&WorkTeamSelection {
            id: "research".into(),
            revision: 1,
        }),
        None,
    )
    .unwrap();
    db.save_work_brief(crate::SaveWorkBrief {
        actor: "owner",
        org: "org",
        team: "space",
        work: "shape",
        request: "brief",
        expected: 0,
        body: "Compare sources",
    })
    .unwrap();
    let mut content = crate::PlanContent {
        title: "Compare".into(),
        summary: "Evidence".into(),
        token_budget: 2000,
        open_questions: vec![],
        assignments: vec![crate::PlanAssignment {
            key: "research".into(),
            title: "Research".into(),
            instructions: "Read".into(),
            agent_key: "Sage".into(),
            depends_on: vec![],
            tools: vec![],
            deliverable: "Sources".into(),
            token_budget: 1000,
        }],
    };
    let save = |content: &crate::PlanContent| {
        db.save_huddle_plan(crate::SaveHuddlePlan {
            actor: "owner",
            org: "org",
            team: "space",
            work: "shape",
            request: "plan",
            expected: 0,
            brief_revision: 1,
            generation_id: "gen",
            generation_input: "brief",
            content: Some(content),
        })
    };
    assert!(save(&content).is_err());
    content.assignments[0].agent_key = "Mira".into();
    assert!(save(&content).is_ok());
}
