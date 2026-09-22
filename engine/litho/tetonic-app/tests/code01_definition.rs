//! CODE-01 public definition / identity-policy / inventory pins.

use std::fs;
use std::path::{Path, PathBuf};

use tetonic_app::coding_pack::CodingPack;
use tetonic_app::definition::{CodingAgentDefinition, CodingIdentityPolicy, ContextBinding};
use tetonic_orchestrator::{RoleId, SpecialistPack};

const ROLES: &[&str] = &["planner", "coder", "debugger", "reviewer", "critic"];
const ALIASES: &[&str] = &[
    "planner", "plan", "coder", "code", "debugger", "debug", "reviewer", "review", "critic",
];

fn production() -> CodingAgentDefinition {
    CodingAgentDefinition::production()
}

#[test]
fn code01_pack_is_facade_over_production() {
    let def = production();
    let pack = CodingPack;
    for role_name in ROLES {
        let role = RoleId::new(*role_name);
        assert_eq!(pack.overlay(&role), def.overlay(&role));
        assert_eq!(pack.allowed_tools(&role), def.allowed_tools(&role));
        assert_eq!(
            pack.spawn_allowed_tools(&role),
            def.spawn_allowed_tools(&role)
        );
        assert_eq!(pack.should_run_critic(&role), def.should_run_critic(&role));
        assert_eq!(pack.max_steps(&role, 16), def.max_steps(&role, 16));
        assert_eq!(
            pack.explain_turn(&role, false),
            def.explain_turn(&role, false)
        );
        assert_eq!(
            pack.explain_turn(&role, true),
            def.explain_turn(&role, true)
        );
    }
    for alias in ALIASES {
        assert_eq!(pack.parse(alias), def.parse(alias));
    }
    assert_eq!(pack.parse("nope"), def.parse("nope"));
    assert_eq!(pack.default_role(), def.default_role());
    assert_eq!(pack.critic_role(), def.critic_role());
    assert_eq!(pack.revision_role(), def.revision_role());
}

#[test]
fn code01_daemon_parse_uses_production() {
    let def = production();
    for alias in ALIASES {
        assert_eq!(
            tetonic_app::coding_pack::CodingPack.parse(alias),
            def.parse(alias)
        );
    }
    assert_eq!(CodingPack.parse("unknown-role"), None);
    assert_eq!(def.parse("unknown-role"), None);
}

#[test]
fn code01_identity_policy_is_not_session_and_is_not_persisted() {
    let def = production();
    let policy = def.identity_policy();
    assert_eq!(policy.actors(), ROLES);
    assert_eq!(policy.privilege_class(), "default");
    assert_eq!(
        policy.bindings(),
        &[
            ContextBinding::Briefing,
            ContextBinding::ProjectContext,
            ContextBinding::Memory
        ]
    );
    for (i, actor) in policy.actors().iter().enumerate() {
        let role = RoleId::new(actor.clone());
        assert_eq!(policy.toolsets()[i].actor, *actor);
        assert_eq!(policy.toolsets()[i].allowed_tools, def.allowed_tools(&role));
        assert_eq!(
            policy.toolsets()[i].spawn_allowed_tools,
            def.spawn_allowed_tools(&role)
        );
    }
    let derived = CodingIdentityPolicy::from_definition(&def);
    assert_eq!(derived, policy);

    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let domain_identity =
        fs::read_to_string(engine_root.join("core/tetonic-domain/src/identity.rs"))
            .expect("lokai-domain identity.rs");
    assert!(
        domain_identity.contains("struct AgentIdentity"),
        "WORK-01 production identity lives in lokai-domain"
    );
    assert!(
        domain_identity.contains("struct AgentJobSpec"),
        "WORK-01 production job spec lives in lokai-domain"
    );
    for (layer, name) in [
        ("mantle", "tetonic-run"),
        ("core", "tetonic-core"),
        ("mantle", "tetonic-node"),
    ] {
        assert!(
            !source_tree_mentions(
                &engine_root.join(layer).join(name).join("src"),
                "CodingAgentDefinition"
            ),
            "{name} must not import CodingAgentDefinition"
        );
    }
}

#[test]
fn code01_prompt_finish_remain_kernel_overlay() {
    let agent_rs =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/tetonic-core/src/agent.rs");
    let agent_src = fs::read_to_string(&agent_rs).expect("agent.rs");
    assert!(
        !agent_src.contains("fn system_prompt"),
        "kernel overlay must be gone after SUB-01 IMPLEMENT"
    );

    let def_src =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/definition.rs"))
            .expect("definition.rs");
    assert!(
        def_src.contains("Investigate before editing"),
        "definition is the live instruction renderer"
    );
    assert!(
        !def_src.contains("completion_policy"),
        "no unused completion_policy field"
    );

    let overlay = production().overlay(&RoleId::new("planner"));
    assert!(overlay.contains("**planner**"));
    assert!(!overlay.contains("You are operating inside the user's workspace"));
}

#[test]
fn code01_app001_inventory_not_established() {
    let _ = production();
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (layer, name) in [
        ("core", "tetonic-core"),
        ("core", "tetonic-runtime"),
        ("atmos", "tetonic-fabric-client"),
        ("mantle", "tetonic-node"),
    ] {
        let src = engine_root.join(layer).join(name).join("src");
        assert!(
            !source_tree_mentions(&src, "CodingAgentDefinition"),
            "{name} must not import CodingAgentDefinition (inventory only; not ESTABLISHED)"
        );
    }
}

fn source_tree_mentions(root: &Path, needle: &str) -> bool {
    walk_rs(root, &mut |path| {
        fs::read_to_string(path)
            .map(|src| {
                src.lines().any(|line| {
                    let trimmed = line.trim_start();
                    !trimmed.starts_with("//") && line.contains(needle)
                })
            })
            .unwrap_or(false)
    })
}

fn walk_rs(root: &Path, pred: &mut dyn FnMut(&Path) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if walk_rs(&path, pred) {
                return true;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") && pred(&path) {
            return true;
        }
    }
    false
}
