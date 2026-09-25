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
    child.wait_with_output().unwrap()
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
