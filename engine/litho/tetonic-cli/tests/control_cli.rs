//! Separate processes exercise real CLI routing, credential delivery and reopen.
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(db: &std::path::Path, args: &[&str], credential: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tetonic"))
        .args(["control", "--database"])
        .arg(db)
        .args(["--audience", "cli-test"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(value) = credential {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(value.as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let res = child.wait_with_output().unwrap();
    if !res.status.success() {
        eprintln!("CMD FAILED: args={:?}, stderr={}", args, String::from_utf8_lossy(&res.stderr));
    }
    res
}

#[test]
fn private_discussion_cli_preserves_history_and_denies_other_principals() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("discussion.db");
    assert!(run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "admin",
            "--org",
            "org",
            "--name",
            "Org"
        ],
        None
    )
    .status
    .success());
    assert!(
        run(&db, &["register-principal", "--principal", "alice"], None)
            .status
            .success()
    );
    let issue = |principal| {
        let output = run(&db, &["issue-credential", "--principal", principal], None);
        assert!(output.status.success());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["credential"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let admin = issue("admin");
    assert!(run(
        &db,
        &[
            "set-member",
            "--org",
            "org",
            "--principal",
            "alice",
            "--role",
            "member"
        ],
        Some(&admin)
    )
    .status
    .success());
    let alice = issue("alice");
    assert!(run(
        &db,
        &[
            "context",
            "private",
            "--org",
            "org",
            "--context",
            "private-a"
        ],
        Some(&alice)
    )
    .status
    .success());
    let open = run(
        &db,
        &[
            "context",
            "open",
            "--context",
            "private-a",
        ],
        Some(&alice),
    );
    assert!(open.status.success());
    let open_json: serde_json::Value = serde_json::from_slice(&open.stdout).unwrap();
    assert_eq!(
        open_json["agent_activated"],
        false
    );
    let session = open_json["session"].as_str().unwrap().to_owned();
    let reopen = run(
        &db,
        &[
            "context",
            "open",
            "--context",
            "private-a",
            "--session",
            &session,
        ],
        Some(&alice),
    );
    assert!(reopen.status.success());
    let file = dir.path().join("message.txt");
    std::fs::write(&file, "PRIVATECANARY — a personal thought").unwrap();
    let send = [
        "context",
        "send",
        "--context",
        "private-a",
        "--session",
        &session,
        "--request",
        "request-1",
        "--message-file",
        file.to_str().unwrap(),
    ];
    for _ in 0..2 {
        assert!(run(&db, &send, Some(&alice)).status.success());
    }
    let history = [
        "context",
        "history",
        "--context",
        "private-a",
        "--session",
        &session,
    ];
    let own = run(&db, &history, Some(&alice));
    assert!(own.status.success());
    let result: serde_json::Value = serde_json::from_slice(&own.stdout).unwrap();
    assert_eq!(result["messages"].as_array().unwrap().len(), 1);
    assert!(result["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("PRIVATECANARY"));
    for token in [Some(admin.as_str()), None] {
        let denied = run(&db, &history, token);
        assert!(!denied.status.success());
        for output in [&denied.stdout, &denied.stderr] {
            let text = String::from_utf8_lossy(output);
            assert!(!text.contains("PRIVATECANARY"));
            assert!(!text.contains(&alice));
            assert!(!text.contains(&admin));
        }
    }
    assert!(run(
        &db,
        &[
            "create-team",
            "--org",
            "org",
            "--team",
            "team",
            "--name",
            "Team"
        ],
        Some(&admin)
    )
    .status
    .success());
    let shared = [
        "context",
        "team",
        "--org",
        "org",
        "--team",
        "team",
        "--context",
        "shared",
    ];
    assert!(!run(&db, &shared, Some(&alice)).status.success());
    assert!(run(&db, &shared, Some(&admin)).status.success());
    assert!(run(
        &db,
        &[
            "add-team-member",
            "--org",
            "org",
            "--team",
            "team",
            "--principal",
            "alice"
        ],
        Some(&admin)
    )
    .status
    .success());
    assert!(run(&db, &shared, Some(&alice)).status.success());
    let search = [
        "context",
        "recall",
        "--context",
        "private-a",
        "--query",
        "PRIVATECANARY",
    ];
    let recalled = run(&db, &search, Some(&alice));
    assert!(recalled.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&recalled.stdout).unwrap()["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!run(&db, &search, Some(&admin)).status.success());
    let shared_search = run(
        &db,
        &[
            "context",
            "recall",
            "--context",
            "shared",
            "--query",
            "PRIVATECANARY",
        ],
        Some(&alice),
    );
    assert!(shared_search.status.success());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&shared_search.stdout).unwrap()["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let wrong_scope = [
        "context",
        "history",
        "--context",
        "shared",
        "--session",
        &session,
    ];
    assert!(!run(&db, &wrong_scope, Some(&alice)).status.success());
    let close = [
        "context",
        "close",
        "--context",
        "private-a",
        "--session",
        &session,
    ];
    assert!(!run(&db, &close, Some(&admin)).status.success());
    for _ in 0..2 {
        assert!(run(&db, &close, Some(&alice)).status.success());
    }
    assert!(run(&db, &history, Some(&alice)).status.success());
    assert!(!run(&db, &send, Some(&alice)).status.success());
    std::fs::write(&file, vec![b'x'; 65_537]).unwrap();
    assert!(!run(&db, &send, Some(&alice)).status.success());
    assert!(run(
        &db,
        &["remove-member", "--org", "org", "--principal", "alice"],
        Some(&admin)
    )
    .status
    .success());
    assert!(!run(&db, &history, Some(&alice)).status.success());
    assert!(!run(&db, &close, Some(&alice)).status.success());
}

#[test]
fn membership_grants_and_revocations_govern_new_processes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("members.db");
    assert!(run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "admin",
            "--org",
            "a",
            "--name",
            "A"
        ],
        None
    )
    .status
    .success());
    assert!(
        run(&db, &["register-principal", "--principal", "bob"], None)
            .status
            .success()
    );
    let issue = |principal| {
        let output = run(&db, &["issue-credential", "--principal", principal], None);
        assert!(output.status.success());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["credential"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let admin = issue("admin");
    let bob = issue("bob");
    let create = [
        "create-team",
        "--org",
        "a",
        "--team",
        "work",
        "--name",
        "Work",
    ];
    let grant = [
        "set-member",
        "--org",
        "a",
        "--principal",
        "bob",
        "--role",
        "team-creator",
    ];
    assert!(!run(&db, &create, Some(&bob)).status.success());
    assert!(!run(&db, &grant, Some(&bob)).status.success());
    assert!(run(&db, &grant, Some(&admin)).status.success());
    assert!(run(&db, &create, Some(&bob)).status.success());
    assert!(
        run(&db, &["register-principal", "--principal", "reader"], None)
            .status
            .success()
    );
    assert!(run(
        &db,
        &[
            "set-member",
            "--org",
            "a",
            "--principal",
            "reader",
            "--role",
            "member"
        ],
        Some(&admin)
    )
    .status
    .success());
    let reader = issue("reader");
    let inspect = ["get-team", "--org", "a", "--team", "work"];
    assert!(!run(&db, &inspect, Some(&reader)).status.success());
    let add = [
        "add-team-member",
        "--org",
        "a",
        "--team",
        "work",
        "--principal",
        "reader",
    ];
    assert!(!run(&db, &add, Some(&reader)).status.success());
    assert!(run(&db, &add, Some(&bob)).status.success());
    assert!(run(&db, &inspect, Some(&reader)).status.success());
    assert!(run(
        &db,
        &[
            "remove-team-member",
            "--org",
            "a",
            "--team",
            "work",
            "--principal",
            "reader"
        ],
        Some(&bob)
    )
    .status
    .success());
    assert!(!run(&db, &inspect, Some(&reader)).status.success());

    assert!(run(
        &db,
        &["remove-member", "--org", "a", "--principal", "bob"],
        Some(&admin)
    )
    .status
    .success());
    let denied = run(
        &db,
        &["get-team", "--org", "a", "--team", "work"],
        Some(&bob),
    );
    assert!(!denied.status.success());
    assert!(!String::from_utf8_lossy(&denied.stderr).contains(&bob));
    assert!(!run(
        &db,
        &["remove-member", "--org", "a", "--principal", "admin"],
        Some(&admin)
    )
    .status
    .success());
}

#[test]
fn bootstrap_create_reopen_and_revoke_via_cli() {
    let volatile = run(
        std::path::Path::new(":memory:"),
        &[
            "bootstrap",
            "--principal",
            "local/admin",
            "--org",
            "a",
            "--name",
            "A",
        ],
        None,
    );
    assert!(!volatile.status.success());
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("control.db");
    let initialized = run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "local/admin",
            "--org",
            "a",
            "--name",
            "Acme",
        ],
        None,
    );
    assert!(initialized.status.success(), "bootstrap failed");
    let takeover = run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "local/other",
            "--org",
            "b",
            "--name",
            "Other",
        ],
        None,
    );
    assert!(!takeover.status.success());
    let issued = run(
        &db,
        &["issue-credential", "--principal", "local/admin"],
        None,
    );
    assert!(issued.status.success(), "credential issuance failed");
    let key: serde_json::Value = serde_json::from_slice(&issued.stdout).unwrap();
    let secret = key["credential"].as_str().unwrap();
    let created = run(
        &db,
        &[
            "create-team",
            "--org",
            "a",
            "--team",
            "maintainers",
            "--name",
            "Maintainers",
        ],
        Some(secret),
    );
    assert!(created.status.success(), "team creation failed");
    let result: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(result["owner_principal_id"], "local/admin");
    let inspected = run(
        &db,
        &["get-team", "--org", "a", "--team", "maintainers"],
        Some(secret),
    );
    assert!(inspected.status.success(), "team inspection failed");
    let no_credential = run(
        &db,
        &["get-team", "--org", "a", "--team", "maintainers"],
        None,
    );
    assert!(!no_credential.status.success());
    let revoked = run(
        &db,
        &[
            "revoke-credential",
            "--credential-id",
            key["credential_id"].as_str().unwrap(),
        ],
        None,
    );
    assert!(revoked.status.success(), "revocation failed");
    let denied = run(
        &db,
        &["get-team", "--org", "a", "--team", "maintainers"],
        Some(secret),
    );
    assert!(!denied.status.success());
    assert!(!String::from_utf8_lossy(&denied.stderr).contains(secret));
    assert!(!String::from_utf8_lossy(&denied.stdout).contains(secret));
}

#[test]
fn registered_agent_cli_is_durable_idempotent_and_not_activation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("agents.db");
    assert!(run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "admin",
            "--org",
            "org",
            "--name",
            "Org"
        ],
        None
    )
    .status
    .success());
    let issued = run(&db, &["issue-credential", "--principal", "admin"], None);
    let issued: serde_json::Value = serde_json::from_slice(&issued.stdout).unwrap();
    let credential = issued["credential"].as_str().unwrap();
    let path = dir.path().join("agent.json");
    std::fs::write(
        &path,
        r#"{"instructions":"AGENTCONFIGCANARY","requested_tools":["read_file"]}"#,
    )
    .unwrap();
    let register = [
        "agent",
        "register",
        "--org",
        "org",
        "--agent",
        "researcher",
        "--harness",
        "general",
        "--config-file",
        path.to_str().unwrap(),
    ];
    let first = run(&db, &register, Some(credential));
    assert!(first.status.success());
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["agent_activated"], false);
    assert_eq!(first["privilege_class"], "unconfigured");
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend(std::fs::read(&path).unwrap());
    std::fs::write(&path, bom).unwrap();
    let retried = run(&db, &register, Some(credential));
    assert!(retried.status.success());
    assert_eq!(
        first,
        serde_json::from_slice::<serde_json::Value>(&retried.stdout).unwrap()
    );
    let get = ["agent", "get", "--org", "org", "--agent", "researcher"];
    assert_eq!(
        first,
        serde_json::from_slice::<serde_json::Value>(&run(&db, &get, Some(credential)).stdout)
            .unwrap()
    );
    std::fs::write(&path, r#"{"instructions":"changed"}"#).unwrap();
    assert!(!run(&db, &register, Some(credential)).status.success());
    assert_eq!(
        first,
        serde_json::from_slice::<serde_json::Value>(&run(&db, &get, Some(credential)).stdout)
            .unwrap()
    );
    let publish = [
        "agent",
        "publish",
        "--org",
        "org",
        "--agent",
        "researcher",
        "--harness",
        "general",
        "--config-file",
        path.to_str().unwrap(),
    ];
    let second = run(&db, &publish, Some(credential));
    assert!(second.status.success());
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(first["identity_id"], second["identity_id"]);
    assert_ne!(first["definition_digest"], second["definition_digest"]);
    assert_eq!(second["agent_activated"], false);
    assert_eq!(
        second,
        serde_json::from_slice::<serde_json::Value>(&run(&db, &publish, Some(credential)).stdout)
            .unwrap()
    );
    for expected in [&first, &second] {
        let selected = run(
            &db,
            &[
                "agent",
                "get",
                "--org",
                "org",
                "--agent",
                "researcher",
                "--revision",
                expected["definition_digest"].as_str().unwrap(),
            ],
            Some(credential),
        );
        assert!(selected.status.success());
        assert_eq!(
            *expected,
            serde_json::from_slice::<serde_json::Value>(&selected.stdout).unwrap()
        );
    }
    assert_eq!(
        first,
        serde_json::from_slice::<serde_json::Value>(&run(&db, &get, Some(credential)).stdout)
            .unwrap()
    );
    assert!(!run(
        &db,
        &[
            "agent",
            "get",
            "--org",
            "org",
            "--agent",
            "researcher",
            "--revision",
            "missing"
        ],
        Some(credential)
    )
    .status
    .success());
    for invalid in [b"[]".to_vec(), vec![b'x'; 65_537]] {
        std::fs::write(&path, invalid).unwrap();
        assert!(!run(&db, &register, Some(credential)).status.success());
    }
    assert!(!run(&db, &get, None).status.success());
    assert!(!run(
        &db,
        &["agent", "get", "--org", "foreign", "--agent", "researcher"],
        Some(credential)
    )
    .status
    .success());
    assert!(run(
        &db,
        &[
            "revoke-credential",
            "--credential-id",
            issued["credential_id"].as_str().unwrap()
        ],
        None
    )
    .status
    .success());
    let denied = run(&db, &get, Some(credential));
    assert!(!denied.status.success());
    for output in [&denied.stdout, &denied.stderr] {
        let text = String::from_utf8_lossy(output);
        assert!(!text.contains("AGENTCONFIGCANARY"));
        assert!(!text.contains(credential));
    }
}

#[test]
fn team_work_and_human_controls_cli_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("team_work.db");
    assert!(run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "admin",
            "--org",
            "org",
            "--name",
            "Org"
        ],
        None
    )
    .status
    .success());
    assert!(
        run(&db, &["register-principal", "--principal", "alice"], None)
            .status
            .success()
    );
    let issue = |principal| {
        let output = run(&db, &["issue-credential", "--principal", principal], None);
        assert!(output.status.success());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["credential"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let admin = issue("admin");
    assert!(run(
        &db,
        &[
            "set-member",
            "--org",
            "org",
            "--principal",
            "alice",
            "--role",
            "team-creator"
        ],
        Some(&admin)
    )
    .status
    .success());
    let alice = issue("alice");

    // Create team
    assert!(run(
        &db,
        &[
            "create-team",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--name",
            "Team Alpha"
        ],
        Some(&alice)
    )
    .status
    .success());

    // 1. Create a team goal
    let goal_out = run(
        &db,
        &[
            "work",
            "create-goal",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--goal",
            "goal-1",
            "--title",
            "Deliver Sprint 6",
        ],
        Some(&alice),
    );
    assert!(goal_out.status.success());
    let goal_json: serde_json::Value = serde_json::from_slice(&goal_out.stdout).unwrap();
    assert_eq!(goal_json["goal_id"], "goal-1");

    // 2. Accept a huddle proposal producing multiple work items
    let huddle_out = run(
        &db,
        &[
            "work",
            "accept-huddle",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--huddle",
            "huddle-1",
            "--request-id",
            "req-hud-1",
            "--title",
            "Task 1: Cutover Config",
            "--title",
            "Task 2: Retire Scaffolding",
        ],
        Some(&alice),
    );
    assert!(huddle_out.status.success());
    let huddle_json: serde_json::Value = serde_json::from_slice(&huddle_out.stdout).unwrap();
    assert_eq!(huddle_json.as_array().unwrap().len(), 2);

    // 3. Create a quick work item
    let quick_out = run(
        &db,
        &[
            "work",
            "create",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--work",
            "work-quick",
            "--title",
            "Task 3: Quick Task",
            "--request-id",
            "req-quick-1",
            "--goal",
            "goal-1",
        ],
        Some(&alice),
    );
    assert!(quick_out.status.success());

    // 4. List work items
    let list_out = run(
        &db,
        &["work", "list", "--org", "org", "--team", "team-alpha"],
        Some(&alice),
    );
    assert!(list_out.status.success());
    let list_json: serde_json::Value = serde_json::from_slice(&list_out.stdout).unwrap();
    assert_eq!(list_json.as_array().unwrap().len(), 3);

    // 5. Park and resume work
    let park_out = run(
        &db,
        &[
            "work",
            "park",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--work",
            "work-quick",
        ],
        Some(&alice),
    );
    assert!(park_out.status.success());
    let park_json: serde_json::Value = serde_json::from_slice(&park_out.stdout).unwrap();
    assert_eq!(park_json["status"], "parked");

    let resume_out = run(
        &db,
        &[
            "work",
            "resume",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--work",
            "work-quick",
        ],
        Some(&alice),
    );
    assert!(resume_out.status.success());
    let resume_json: serde_json::Value = serde_json::from_slice(&resume_out.stdout).unwrap();
    assert_eq!(resume_json["status"], "open");

    // 6. Propose an effect approval for work-quick
    let future_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let propose_out = run(
        &db,
        &[
            "work",
            "propose-approval",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--approval",
            "app-1",
            "--digest",
            "sha256:fedcba9876543210",
            "--request-id",
            "req-app-1",
            "--expires-at",
            &future_ts.to_string(),
            "--work",
            "work-quick",
        ],
        Some(&alice),
    );
    assert!(propose_out.status.success());

    // 7. Inspect team - verifies pending approvals exist and sibling tasks remain open
    let inspect_out = run(
        &db,
        &[
            "work",
            "inspect-team",
            "--org",
            "org",
            "--team",
            "team-alpha",
        ],
        Some(&alice),
    );
    assert!(inspect_out.status.success());
    let inspect_json: serde_json::Value = serde_json::from_slice(&inspect_out.stdout).unwrap();
    assert_eq!(inspect_json["pending_approvals"].as_array().unwrap().len(), 1);
    assert_eq!(
        inspect_json["pending_approvals"][0]["approval_id"],
        "app-1"
    );
    // Sibling work items remain open
    let items = inspect_json["work_items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().any(|i| i["title"] == "Task 1: Cutover Config" && i["status"] == "open"));

    // 8. Hierarchical stop & clear stop
    let stop_out = run(
        &db,
        &[
            "work",
            "stop",
            "--org",
            "org",
            "--scope",
            "team",
            "--id",
            "team-alpha",
            "--mode",
            "pause",
            "--reason",
            "Pre-flight verification pause",
        ],
        Some(&alice),
    );
    assert!(stop_out.status.success());

    let inspect_stopped = run(
        &db,
        &[
            "work",
            "inspect-team",
            "--org",
            "org",
            "--team",
            "team-alpha",
        ],
        Some(&alice),
    );
    assert!(inspect_stopped.status.success());
    let inspect_stopped_json: serde_json::Value =
        serde_json::from_slice(&inspect_stopped.stdout).unwrap();
    assert_eq!(inspect_stopped_json["active_stops"].as_array().unwrap().len(), 1);

    let clear_out = run(
        &db,
        &[
            "work",
            "clear-stop",
            "--org",
            "org",
            "--scope",
            "team",
            "--id",
            "team-alpha",
        ],
        Some(&alice),
    );
    assert!(clear_out.status.success());

    // 9. Resolve approval
    let resolve_out = run(
        &db,
        &[
            "work",
            "resolve-approval",
            "--org",
            "org",
            "--team",
            "team-alpha",
            "--approval",
            "app-1",
            "--digest",
            "sha256:fedcba9876543210",
            "--allow",
        ],
        Some(&alice),
    );
    assert!(resolve_out.status.success());

    // Final inspection: pending approvals now empty
    let inspect_final = run(
        &db,
        &[
            "work",
            "inspect-team",
            "--org",
            "org",
            "--team",
            "team-alpha",
        ],
        Some(&alice),
    );
    assert!(inspect_final.status.success());
    let inspect_final_json: serde_json::Value =
        serde_json::from_slice(&inspect_final.stdout).unwrap();
    assert!(inspect_final_json["pending_approvals"].as_array().unwrap().is_empty());
}

#[test]
fn workstation_placement_cli_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("workstations.db");
    assert!(run(
        &db,
        &[
            "bootstrap",
            "--principal",
            "admin",
            "--org",
            "org",
            "--name",
            "Org"
        ],
        None
    )
    .status
    .success());
    let issued = run(&db, &["issue-credential", "--principal", "admin"], None);
    let admin = serde_json::from_slice::<serde_json::Value>(&issued.stdout).unwrap()["credential"]
        .as_str()
        .unwrap()
        .to_owned();

    // 1. Enroll workstation
    let enroll_out = run(
        &db,
        &[
            "workstation",
            "enroll",
            "--org",
            "org",
            "--workstation",
            "ws-agent-1",
            "--label",
            "Linux Build Node",
            "--platform",
            "linux",
            "--device-secret",
            "device-secret-12345",
            "--shared-assignment",
        ],
        Some(&admin),
    );
    assert!(enroll_out.status.success());
    let enroll_json: serde_json::Value = serde_json::from_slice(&enroll_out.stdout).unwrap();
    assert_eq!(enroll_json["workstation_id"], "ws-agent-1");
    assert_eq!(enroll_json["platform"], "linux");
    assert_eq!(enroll_json["status"], "enrolled");

    // 2. Approve resource grant
    let grant_out = run(
        &db,
        &[
            "workstation",
            "approve-grant",
            "--org",
            "org",
            "--workstation",
            "ws-agent-1",
            "--grant",
            "grant-gpu",
            "--kind",
            "model_capacity",
            "--resource",
            "local/qwen2.5-coder",
        ],
        Some(&admin),
    );
    assert!(grant_out.status.success());

    // 3. Create team & work item to test pinning
    assert!(run(
        &db,
        &[
            "create-team",
            "--org",
            "org",
            "--team",
            "team-gpu",
            "--name",
            "GPU Team"
        ],
        Some(&admin)
    )
    .status
    .success());

    assert!(run(
        &db,
        &[
            "work",
            "create",
            "--org",
            "org",
            "--team",
            "team-gpu",
            "--work",
            "work-gpu-1",
            "--title",
            "Model fine-tuning task",
            "--request-id",
            "req-gpu-1",
        ],
        Some(&admin)
    )
    .status
    .success());

    // 4. Pin work to workstation
    let pin_out = run(
        &db,
        &[
            "workstation",
            "pin",
            "--org",
            "org",
            "--team",
            "team-gpu",
            "--work",
            "work-gpu-1",
            "--workstation",
            "ws-agent-1",
        ],
        Some(&admin),
    );
    assert!(pin_out.status.success());

    // 5. Offline and Reconnect
    let off_out = run(
        &db,
        &[
            "workstation",
            "offline",
            "--org",
            "org",
            "--workstation",
            "ws-agent-1",
        ],
        Some(&admin),
    );
    assert!(off_out.status.success());
    let off_json: serde_json::Value = serde_json::from_slice(&off_out.stdout).unwrap();
    assert_eq!(off_json["workstation"]["status"], "offline");

    let rec_out = run(
        &db,
        &[
            "workstation",
            "reconnect",
            "--org",
            "org",
            "--workstation",
            "ws-agent-1",
        ],
        Some(&admin),
    );
    assert!(rec_out.status.success());
    let rec_json: serde_json::Value = serde_json::from_slice(&rec_out.stdout).unwrap();
    assert_eq!(rec_json["status"], "enrolled");

    // 6. Drain workstation
    let drain_out = run(
        &db,
        &[
            "workstation",
            "drain",
            "--org",
            "org",
            "--workstation",
            "ws-agent-1",
        ],
        Some(&admin),
    );
    assert!(drain_out.status.success());
    let drain_json: serde_json::Value = serde_json::from_slice(&drain_out.stdout).unwrap();
    assert_eq!(drain_json["status"], "draining");

    // 7. Revoke workstation
    let revoke_out = run(
        &db,
        &[
            "workstation",
            "revoke",
            "--org",
            "org",
            "--workstation",
            "ws-agent-1",
        ],
        Some(&admin),
    );
    assert!(revoke_out.status.success());
    let revoke_json: serde_json::Value = serde_json::from_slice(&revoke_out.stdout).unwrap();
    assert_eq!(revoke_json["status"], "revoked");
}
