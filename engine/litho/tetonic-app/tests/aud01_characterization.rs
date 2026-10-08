//! AUD-01 app-door characterization (public APIs only).

use tetonic_app::coding_pack::CodingPack;
use tetonic_orchestrator::{RoleId, SpecialistPack};

#[test]
fn aud01_bh_resp_explain_turn_app_pack() {
    let pack = CodingPack;
    assert!(pack.explain_turn(&RoleId::new("planner"), false));
    assert!(pack.explain_turn(&RoleId::new("reviewer"), false));
    assert!(pack.explain_turn(&RoleId::new("critic"), false));
    assert!(!pack.explain_turn(&RoleId::new("coder"), false));
}

#[test]
fn aud01_bh_invest_app_pack_read_subset() {
    let tools = CodingPack
        .allowed_tools(&RoleId::new("planner"))
        .expect("read subset");
    assert!(tools.contains(&"search_code".into()));
    assert!(tools.contains(&"read_file".into()));
    assert!(!tools.contains(&"edit_file".into()));
}

#[test]
fn aud01_bh_plan_app_pack_no_edit() {
    let overlay = CodingPack.overlay(&RoleId::new("planner"));
    assert!(overlay.contains("Do NOT edit"));
    let tools = CodingPack.allowed_tools(&RoleId::new("planner")).unwrap();
    assert!(!tools.contains(&"run_shell".into()));
    assert!(!tools.contains(&"write_file".into()));
}

#[test]
fn aud01_bh_edit_mode_exists_without_commit_contract() {
    // PRESERVE mode only. Commit-before-winner is BH-FIN-COMMIT DEFECT, not this row.
    assert!(CodingPack.allowed_tools(&RoleId::new("coder")).is_none());
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/definition.rs"));
    assert!(src.contains("edit_file"));
    assert!(!src.contains("commit_staged_if_any"));
}
