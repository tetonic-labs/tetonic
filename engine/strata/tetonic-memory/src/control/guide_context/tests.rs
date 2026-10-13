use super::*;

fn seed(path: &std::path::Path) -> Store {
    let db = Store::open(path).unwrap();
    db.bootstrap_control("owner", "org", "Org").unwrap();
    db.create_team(&crate::TeamRow {
        org_id: "org".into(),
        team_id: "team".into(),
        name: "Team".into(),
        owner_principal_id: "owner".into(),
    })
    .unwrap();
    for id in ["a", "b"] {
        db.create_team_work_item_for_purpose(
            crate::CreateTeamWorkItem {
                actor: "owner",
                org: "org",
                team: "team",
                work_id: id,
                title: id,
                request_id: id,
                goal_id: None,
            },
            Some(id),
            WorkPurpose::Explore,
        )
        .unwrap();
    }
    db
}
fn draft() -> GuideSubjectDraft {
    GuideSubjectDraft {
        id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        title: "A distinct concern".into(),
        understanding: "Option B is tentative. A was rejected.".into(),
        source_turns: vec!["a".into()],
        work_ids: vec![],
    }
}

#[test]
fn subjects_cross_conversations_without_moving_work_and_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guide.db");
    let db = seed(&path);
    let draft = draft();
    let request = uuid::Uuid::new_v4().to_string();
    let saved = db
        .save_guide_subject("owner", "org", "team", &request, draft.clone())
        .unwrap();
    assert_eq!(
        db.save_guide_subject("owner", "org", "team", &request, draft.clone())
            .unwrap(),
        saved
    );
    let mut changed = draft.clone();
    changed.understanding = "Different retry".into();
    assert!(db
        .save_guide_subject("owner", "org", "team", &request, changed.clone())
        .is_err());
    let selected = db
        .set_guide_focus(
            "owner",
            "org",
            "team",
            "b",
            &uuid::Uuid::new_v4().to_string(),
            GuideFocus {
                revision: 0,
                subject_id: Some(saved.id.clone()),
            },
        )
        .unwrap();
    assert_eq!(selected.revision, 1);
    assert!(db
        .get_team_work_item("org", "team", "b")
        .unwrap()
        .unwrap()
        .run_id
        .is_none());
    assert!(db
        .huddle_plans("owner", "org", "team", "b")
        .unwrap()
        .is_empty());
    changed.expected_revision = 1;
    changed.understanding = "Human correction: keep A instead.".into();
    changed.source_turns.push("b".into());
    db.save_guide_subject(
        "owner",
        "org",
        "team",
        &uuid::Uuid::new_v4().to_string(),
        changed.clone(),
    )
    .unwrap();
    assert!(db
        .save_guide_subject(
            "owner",
            "org",
            "team",
            &uuid::Uuid::new_v4().to_string(),
            draft
        )
        .is_err());
    assert!(db
        .set_guide_focus(
            "owner",
            "org",
            "team",
            "b",
            &uuid::Uuid::new_v4().to_string(),
            GuideFocus {
                revision: 0,
                subject_id: None
            }
        )
        .is_err());
    assert!(db
        .guide_subject("stranger", "org", "team", &saved.id)
        .is_err());
    assert!(db
        .guide_subject("owner", "other", "team", &saved.id)
        .is_err());
    drop(db);
    let reopened = Store::open(path).unwrap();
    assert_eq!(
        reopened.guide_focus("owner", "org", "team", "b").unwrap(),
        selected
    );
    assert_eq!(
        reopened
            .guide_subject("owner", "org", "team", &saved.id)
            .unwrap()
            .unwrap()
            .understanding,
        changed.understanding
    );
    assert_eq!(
        reopened
            .get_team_work_item("org", "team", "a")
            .unwrap()
            .unwrap()
            .input
            .as_deref(),
        Some("a")
    );
}

#[test]
fn source_and_work_ids_do_not_grant_cross_scope_access() {
    let db = seed(std::path::Path::new(":memory:"));
    let mut bad = draft();
    bad.source_turns = vec!["unknown".into()];
    assert!(db
        .save_guide_subject(
            "owner",
            "org",
            "team",
            &uuid::Uuid::new_v4().to_string(),
            bad
        )
        .is_err());
    let mut bad = draft();
    bad.work_ids = vec!["other-team-work".into()];
    assert!(db
        .save_guide_subject(
            "owner",
            "org",
            "team",
            &uuid::Uuid::new_v4().to_string(),
            bad
        )
        .is_err());
    assert!(db
        .guide_subjects("owner", "org", "team", 0)
        .unwrap()
        .is_empty());
}

#[test]
fn saving_understanding_after_more_than_64_ancestors_keeps_revision_fences() {
    let db = seed(std::path::Path::new(":memory:"));
    let mut parent = "a".to_string();
    for i in 0..80 {
        let id = format!("turn-{i}");
        let request = format!("{id}#{parent}");
        db.create_team_work_item_for_purpose(
            crate::CreateTeamWorkItem {
                actor: "owner",
                org: "org",
                team: "team",
                work_id: &id,
                title: "Thought",
                request_id: &request,
                goal_id: None,
            },
            Some("Detail"),
            WorkPurpose::Explore,
        )
        .unwrap();
        parent = id;
    }
    let saved = db
        .save_conversation_brief(
            crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "a",
                request: "long-turn",
                expected: 0,
                body: "Latest correction retained.",
            },
            &parent,
            0,
        )
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert!(db
        .save_conversation_brief(
            crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "a",
                request: "stale",
                expected: 0,
                body: "Stale notes"
            },
            &parent,
            0
        )
        .is_err());
}
