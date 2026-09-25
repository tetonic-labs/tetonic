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
