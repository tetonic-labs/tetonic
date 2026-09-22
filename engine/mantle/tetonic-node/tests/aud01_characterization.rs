//! AUD-01 negative control: worker Infer never constructs Agent.

#[test]
fn aud01_bh_cmp_infer_never_constructs_agent() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/fabric_chat.rs"));
    assert!(
        !src.contains("tetonic_core::Agent"),
        "worker Infer must not construct Agent"
    );
    assert!(!src.contains("Agent::turn"));
    assert!(!src.contains("Agent::new"));
    assert!(!src.contains("Agent::run"));
}
