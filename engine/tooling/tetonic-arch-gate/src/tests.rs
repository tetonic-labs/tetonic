use super::*;

#[test]
fn compute_wiring_gate_follows_host_composition_and_rejects_missing_boundaries() {
    let source = engine_root();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for path in [
        "litho/tetonic-app/Cargo.toml",
        "litho/tetonic-app/src/compute_plane.rs",
        "litho/tetonic-app/src/job_launch.rs",
        "litho/tetonic-app/src/host/mod.rs",
        "mantle/tetonic-broker/src/lib.rs",
    ] {
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(source.join(path), target).unwrap();
    }
    assert!(check_compute_broker_wiring(root).is_empty());
    for (path, boundary) in [
        (
            "litho/tetonic-app/src/job_launch.rs",
            "ApplicationHost::open(",
        ),
        (
            "litho/tetonic-app/src/job_launch.rs",
            "ApplicationHost::from_control(",
        ),
        ("litho/tetonic-app/src/host/mod.rs", "build_compute_plane("),
        (
            "litho/tetonic-app/src/host/mod.rs",
            "install_compute_services(",
        ),
        ("litho/tetonic-app/src/host/mod.rs", "attach_egress("),
    ] {
        let target = root.join(path);
        let original = std::fs::read_to_string(&target).unwrap();
        std::fs::write(&target, original.replace(boundary, "missing_boundary(")).unwrap();
        assert!(
            !check_compute_broker_wiring(root).is_empty(),
            "missed {boundary}"
        );
        std::fs::write(target, original).unwrap();
    }
}

#[test]
fn gate_passes_on_engine_tree() {
    let root = engine_root();
    let violations = run_all(&root);
    assert!(
        violations.is_empty(),
        "architecture violations: {violations:#?}"
    );
}

#[test]
fn production_tools_are_sandboxed() {
    let root = engine_root();
    assert!(
        check_production_tools_sandboxed(&root).is_empty(),
        "production Tools/worktree/runtime must use Sandboxed: {:#?}",
        check_production_tools_sandboxed(&root)
    );
    assert!(
        check_git_via_process_broker(&root).is_empty(),
        "git must not use raw Command::new: {:#?}",
        check_git_via_process_broker(&root)
    );
}

#[test]
fn context_compiler_is_wired_in_assembly() {
    let root = engine_root();
    assert!(
        check_context_compiler_wired(&root).is_empty(),
        "R4-1 ContextCompiler wiring missing: {:#?}",
        check_context_compiler_wired(&root)
    );
}

#[test]
fn subprocess_check_catches_disallowed_file() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("evil.rs");
    std::fs::write(&bad, "fn f() { Command::new(\"sh\"); }").unwrap();
    let v = check_subprocess_spawn(dir.path());
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule, "subprocess_spawn");
}

#[test]
fn gates_ok_turn_abort_fails_gate() {
    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("litho/tetonic-cli/src/chat.rs");
    std::fs::create_dir_all(chat.parent().unwrap()).unwrap();
    std::fs::write(&chat, "if !doc.status.gates_ok { abort_turn(); }\n").unwrap();
    let v = check_no_gates_ok_turn_abort(dir.path());
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule, "no_gates_ok_turn_abort");
}

#[test]
fn gates_ok_display_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let cap = dir.path().join("litho/tetonic-cli/src/capacity.rs");
    std::fs::create_dir_all(cap.parent().unwrap()).unwrap();
    std::fs::write(&cap, "println!(\"gates_ok={}\", wire.gates_ok);\n").unwrap();
    let v = check_no_gates_ok_turn_abort(dir.path());
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn duplicate_resume_cap_in_bins_fails_gate() {
    let dir = tempfile::tempdir().unwrap();
    let resume = dir.path().join("litho/tetonicd/src/daemon/resume.rs");
    std::fs::create_dir_all(resume.parent().unwrap()).unwrap();
    std::fs::write(&resume, "pub const RESUME_MESSAGE_CAP: u32 = 200;\n").unwrap();
    let v = check_no_duplicate_resume_cap(dir.path());
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule, "no_duplicate_resume_cap");
}

#[test]
fn duplicate_enrollment_helper_in_bins_fails_gate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("litho/tetonic-cli/src/estate.rs");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
            &path,
            "pub fn reload_enrollment_egress(store: &Store, guard: &EgressGuard) -> Result<()> { Ok(()) }\n",
        )
        .unwrap();
    let v = check_no_duplicate_enrollment_helpers(dir.path());
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule, "no_duplicate_enrollment_helpers");
}

#[test]
fn workspace_mutations_catches_cli_raw_write() {
    let dir = tempfile::tempdir().unwrap();
    let offline = dir.path().join("litho/tetonic-cli/src/offline.rs");
    std::fs::create_dir_all(offline.parent().unwrap()).unwrap();
    std::fs::write(
        &offline,
        "fn restore() { std::fs::write(path, text).unwrap(); }\n",
    )
    .unwrap();
    let v = check_workspace_mutations(dir.path());
    assert_eq!(v.len(), 1, "{v:#?}");
    assert_eq!(v[0].rule, "workspace_mutation_bypass");
}

#[test]
fn workspace_mutations_allows_cli_jailed_write() {
    let dir = tempfile::tempdir().unwrap();
    let offline = dir.path().join("litho/tetonic-cli/src/offline.rs");
    std::fs::create_dir_all(offline.parent().unwrap()).unwrap();
    std::fs::write(
        &offline,
        "fn restore() { tetonic_tools::write_bytes_nofollow(&path, text).unwrap(); }\n",
    )
    .unwrap();
    let v = check_workspace_mutations(dir.path());
    assert!(v.is_empty(), "{v:#?}");
}
