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
    for (id, purpose) in [
        ("work", crate::WorkPurpose::Work),
        ("thought", crate::WorkPurpose::Explore),
    ] {
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
            purpose,
        )
        .unwrap();
    }
    db
}
fn signal(signature: &str) -> GuideSignal {
    GuideSignal {
        key: "connection/calendar".into(),
        work_id: "work".into(),
        signature: signature.into(),
        kind: "connection".into(),
        title: "Calendar connection".into(),
        detail: "Recorded failure; effect unknown.".into(),
    }
}
fn observe(db: &Store, signals: Vec<GuideSignal>) {
    db.observe_guide_signals("owner", "org", "team", signals)
        .unwrap();
}
fn enable(db: &Store) {
    db.save_guide_attention_settings(
        "owner",
        "org",
        "team",
        &uuid::Uuid::new_v4().to_string(),
        GuideAttentionSettings {
            revision: 0,
            enabled: true,
            daily_tokens: 2048,
            assessment_tokens: 1024,
        },
    )
    .unwrap();
}
fn claim(db: &Store) -> Option<GuideAttentionItem> {
    db.claim_guide_attention("owner", "org", "team").unwrap()
}
fn complete(db: &Store, id: &str, text: Option<&str>) {
    db.complete_guide_attention("owner", "org", "team", id, text.map(str::to_owned))
        .unwrap();
}

#[test]
fn repeated_incident_restart_and_read_do_not_multiply_assessments_or_resolve_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("attention.db");
    let db = seed(&path);
    observe(&db, vec![signal("failure-1")]);
    assert!(claim(&db).is_none(), "Default off means no model");
    enable(&db);
    let first = claim(&db).unwrap().assessment_id.unwrap();
    for _ in 0..50 {
        observe(&db, vec![signal("failure-1")]);
        assert!(claim(&db).is_none());
    }
    db.see_guide_attention("owner", "org", "team", "connection/calendar")
        .unwrap();
    assert_eq!(
        db.guide_attention_view("owner", "org", "team")
            .unwrap()
            .items[0]
            .state,
        "assessing"
    );
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(
        db.outstanding_guide_assessments("owner", "org", "team")
            .unwrap(),
        vec![first.clone()]
    );
    assert!(claim(&db).is_none());
    complete(
        &db,
        &first,
        Some("The calendar needs reconnecting; inspect the failed work."),
    );
    observe(&db, vec![signal("failure-1")]);
    assert!(claim(&db).is_none());
    let view = db.guide_attention_view("owner", "org", "team").unwrap();
    assert_eq!(view.reserved_today, 1024);
    assert!(view.items[0].seen);
    assert_eq!(view.items[0].state, "reviewed");
}

#[test]
fn superseded_recovered_and_late_assessments_cannot_publish_stale_results() {
    let db = seed(std::path::Path::new(":memory:"));
    enable(&db);
    observe(&db, vec![signal("failure-1")]);
    let old = claim(&db).unwrap().assessment_id.unwrap();
    observe(&db, vec![signal("failure-2")]);
    assert!(
        claim(&db).is_none(),
        "Old admitted work still holds the assessment slot"
    );
    complete(&db, &old, Some("STALE"));
    assert!(db
        .guide_attention_view("owner", "org", "team")
        .unwrap()
        .items[0]
        .update
        .is_none());
    let next = claim(&db).unwrap().assessment_id.unwrap();
    observe(&db, vec![]);
    complete(&db, &next, Some("LATE"));
    let view = db.guide_attention_view("owner", "org", "team").unwrap();
    assert_eq!(view.items[0].state, "resolved");
    assert!(view.items[0].update.is_none());
    observe(&db, vec![signal("failure-3")]);
    assert!(
        claim(&db).is_none(),
        "Full allowance stays reserved after interruption"
    );
    assert_eq!(
        db.guide_attention_view("owner", "org", "team")
            .unwrap()
            .reserved_today,
        2048
    );
}

#[test]
fn scope_replay_revisions_and_bounded_backlog_are_enforced() {
    let db = seed(std::path::Path::new(":memory:"));
    let request = uuid::Uuid::new_v4().to_string();
    let settings = GuideAttentionSettings {
        revision: 0,
        enabled: true,
        daily_tokens: 2048,
        assessment_tokens: 1024,
    };
    let saved = db
        .save_guide_attention_settings("owner", "org", "team", &request, settings.clone())
        .unwrap();
    assert_eq!(
        db.save_guide_attention_settings("owner", "org", "team", &request, settings.clone())
            .unwrap(),
        saved
    );
    assert!(db
        .save_guide_attention_settings(
            "owner",
            "org",
            "team",
            &uuid::Uuid::new_v4().to_string(),
            settings
        )
        .is_err());
    let mut bad = signal("bad");
    bad.work_id = "thought".into();
    assert!(db
        .observe_guide_signals("owner", "org", "team", vec![bad])
        .is_err());
    assert!(db
        .observe_guide_signals("stranger", "org", "team", vec![signal("bad")])
        .is_err());
    assert!(db.guide_attention_view("owner", "other", "team").is_err());
    observe(
        &db,
        (0..300)
            .map(|i| {
                let mut s = signal("one");
                s.key = format!("failure/{i:03}");
                s
            })
            .collect(),
    );
    let view = db.guide_attention_view("owner", "org", "team").unwrap();
    assert!(view.partial);
    assert_eq!(view.items.len(), 100);
    let pending: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM guide_attention WHERE state='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 256);
    let id = claim(&db).unwrap().assessment_id.unwrap();
    let settings = GuideAttentionSettings {
        revision: saved.revision,
        enabled: false,
        daily_tokens: 0,
        assessment_tokens: 0,
    };
    db.save_guide_attention_settings(
        "owner",
        "org",
        "team",
        &uuid::Uuid::new_v4().to_string(),
        settings,
    )
    .unwrap();
    complete(&db, &id, None);
    assert!(claim(&db).is_none());
}
