//! Current product owners, independent of the retained historical gate rules.
//! Syntax checks are tripwires, not proofs of runtime authorization or semantics.
use crate::{collect_rs_files, freeze::production_text, rel, Violation};
use std::path::Path;

const OWNERS: &[&str] = &[
    "litho/tetonic-cli/src/local_ui.rs",
    "litho/tetonic-cli/src/job.rs",
    "litho/tetonic-cli/src/control.rs",
    "litho/tetonic-app/src/host/mod.rs",
    "litho/tetonic-app/src/work/mod.rs",
    "litho/tetonic-app/src/workspace/mod.rs",
    "litho/tetonic-app/src/resources.rs",
    "litho/tetonic-app/src/team_work_controller.rs",
    "litho/tetonic-app/src/resources/registered/assembly.rs",
    "litho/tetonic-app/src/execution/mod.rs",
    "mantle/tetonic-run/src/managed/mod.rs",
    "strata/tetonic-memory/src/control/mod.rs",
    "strata/tetonic-memory/src/execution/run_store.rs",
    "strata/tetonic-memory/src/context/mod.rs",
    "strata/tetonic-memory/src/usage/mod.rs",
    "strata/tetonic-memory/src/artifacts/mod.rs",
    "litho/tetonic-tools/src/lib.rs",
];

pub(crate) fn check(root: &Path) -> Vec<Violation> {
    let mut findings = Vec::new();
    for owner in OWNERS {
        let path = root.join(owner);
        if std::fs::read_to_string(&path).is_err() {
            findings.push(Violation {
                rule: "current_owner_missing",
                path,
                detail: "current architectural owner is missing/unreadable; update its ownership map, imports and checks together".into(),
            });
        }
    }
    let lifecycle = regex::Regex::new(
        r"RunCommand\s*::\s*(CreateAttempt|StartAttempt|LeaseAttempt|ClaimExecution|ClaimFinalization|CompleteAttempt|FailAttempt|FinishRun|CancelRun|ResumeAttempt)\b|(?:DurableRunSupervisor|ManagedRunService)\s*::\s*new\s*\(",
    ).expect("lifecycle pattern");
    let agent = regex::Regex::new(r"Agent\s*::\s*new\s*\(").expect("agent pattern");
    let mut files = Vec::new();
    for area in [
        "litho/tetonic-cli/src",
        "litho/tetonic-app/src/work",
        "litho/tetonic-app/src/workspace",
        "litho/tetonic-app/src/resources",
    ] {
        files.extend(collect_rs_files(&root.join(area)));
    }
    files.push(root.join("litho/tetonic-app/src/team_work_controller.rs"));
    files.push(root.join("litho/tetonic-app/src/resources.rs"));
    for path in files {
        let relative = rel(root, &path);
        if relative.contains("/tests/") || relative.ends_with("_tests.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let source = production_text(&text);
        let composition_owner =
            relative == "litho/tetonic-app/src/resources/registered/assembly.rs";
        if lifecycle.is_match(&source) || (!composition_owner && agent.is_match(&source)) {
            findings.push(Violation {
                rule: "product_lifecycle_bypass",
                path,
                detail: "product use cases and transports must submit through registered execution; lifecycle belongs to tetonic-run and agent composition to resources/registered/assembly".into(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(root: &Path) {
        for file in OWNERS {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "// owner fixture\n").unwrap();
        }
    }

    #[test]
    fn deleting_any_current_owner_fails_the_check() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        assert!(check(dir.path()).is_empty());
        for owner in OWNERS {
            let path = dir.path().join(owner);
            std::fs::remove_file(&path).unwrap();
            let errors = check(dir.path());
            assert_eq!(errors.len(), 1, "{owner}");
            assert_eq!(errors[0].rule, "current_owner_missing");
            std::fs::write(path, "// restored fixture\n").unwrap();
        }
    }

    #[test]
    fn helpers_cannot_reintroduce_lifecycle_but_registered_composition_and_tests_can_construct_agents(
    ) {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let owner = dir
            .path()
            .join("litho/tetonic-app/src/resources/registered/assembly.rs");
        std::fs::write(&owner, "fn assemble() { Agent::new(inputs); }").unwrap();
        for relative in [
            "litho/tetonic-cli/src/helper.rs",
            "litho/tetonic-app/src/work/helper.rs",
            "litho/tetonic-app/src/workspace/helper.rs",
            "litho/tetonic-app/src/resources/helper.rs",
            "litho/tetonic-app/src/resources.rs",
            "litho/tetonic-app/src/team_work_controller.rs",
        ] {
            let helper = dir.path().join(relative);
            for source in [
                "fn bad() { RunCommand :: ClaimExecution(command); }",
                "fn bad() { Agent::new(inputs); }",
                "fn bad() { DurableRunSupervisor::new(store); }",
                "fn bad() { ManagedRunService::new(store); }",
            ] {
                std::fs::write(&helper, source).unwrap();
                let errors = check(dir.path());
                assert_eq!(errors.len(), 1, "{relative}: {source}");
                assert_eq!(errors[0].rule, "product_lifecycle_bypass");
            }
            std::fs::write(&helper, "// RunCommand::ClaimExecution example\n#[cfg(test)]\nmod tests { fn fixture() { Agent::new(inputs); } }\nfn submit() { resources.submit_registered_job(input); }").unwrap();
            assert!(check(dir.path()).is_empty());
        }
        std::fs::write(owner, "fn bad() { RunCommand::CompleteAttempt(command); }").unwrap();
        assert_eq!(check(dir.path())[0].rule, "product_lifecycle_bypass");
    }
}
