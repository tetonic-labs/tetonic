//! AUD-01 fabric-client defect characterization.
//! Passing these tests does not establish INV-V4-CMP-001.

fn result_accept_src() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/result_accept.rs"))
}

fn lib_src() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
}

fn production_src_has(needle: &str) -> bool {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let Ok(read) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let prod = text.split("#[cfg(test)]").next().unwrap_or(&text);
                if prod.contains(needle) {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn aud01_defect_bh_cmp_fabric_current() {
    // WORK-FIN-02 deleted construct. Not INV-V4-CMP-001 established.
    let src = result_accept_src();
    assert!(!src.contains("fn build_complete_attempt"));
    assert!(!src.contains("Ok(CompleteAttempt {"));
}

#[test]
fn aud01_defect_bh_cmp_patch_current() {
    // WORK-FIN-02 invert. GATE-01 moved the remnant out of production src/.
    let lib = lib_src();
    assert!(!lib.contains("pub mod patch_pipeline"));
    assert!(!lib.contains("pub use patch_pipeline"));
    assert!(!production_src_has("pub fn apply_verified_remote_patch"));
    assert!(!production_src_has("pub fn apply_authorized_remote_patch"));
}
