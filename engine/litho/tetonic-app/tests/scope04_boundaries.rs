//! Keep adapter defaults and work orchestration from growing back together.
use std::path::Path;

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

#[test]
fn local_workspace_is_only_bootstrap_and_compatibility() {
    let adapter = read("src/local_workspace.rs");
    assert!(adapter.contains("WorkService as LocalWorkspace"));
    for forbidden in [
        "activate_team_work(",
        "create_work_with_roster(",
        "impl LocalWorkspace",
        "async fn",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "adapter acquired behavior: {forbidden}"
        );
    }
    assert!(read("src/work/submission.rs").contains("activate_team_work("));
    assert!(
        read("src/work/plan_execution/controller.rs").contains("impl TeamWorkHost for WorkService")
    );
}

#[test]
fn service_sources_do_not_embed_local_authorization_identities() {
    fn check(path: &Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                check(&path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for literal in ["\"local-ui-owner\"", "\"local-ui\"", "\"local-work\""] {
                    assert!(
                        !text.contains(literal),
                        "{} embeds {literal}",
                        path.display()
                    );
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    check(&root.join("src/work"));
    check(&root.join("src/workspace"));
}
