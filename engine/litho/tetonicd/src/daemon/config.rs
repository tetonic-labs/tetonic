//! RPC auth and environment flags.

pub use tetonic_app::generate_rpc_token;

/// Parent-provisioned token, or a generated one. Empty/whitespace env is unset.
pub fn rpc_token_from_env_or_generate() -> String {
    match std::env::var("LOKAI_RPC_TOKEN") {
        Ok(t) => {
            let t = t.trim();
            if t.is_empty() {
                generate_rpc_token()
            } else {
                t.to_string()
            }
        }
        Err(_) => generate_rpc_token(),
    }
}

pub fn rpc_auth_disabled() -> bool {
    std::env::var("LOKAI_RPC_INSECURE")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Whether stdio RPC runs in strict mode (blocks control-plane mutations).
/// Default **on** unless `LOKAI_STRICT_RPC=0` or `false`.
pub fn rpc_strict_mode() -> bool {
    !matches!(
        std::env::var("LOKAI_STRICT_RPC"),
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false")
    )
}

/// Whether stdio RPC may mutate egress allow rules (default deny; AR1-1).
pub fn rpc_egress_mutations_allowed() -> bool {
    if rpc_strict_mode() {
        return false;
    }
    std::env::var("LOKAI_ALLOW_RPC_EGRESS")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Whether stdio RPC may mutate policy/capacity control plane (SEC2-E2-006).
pub fn rpc_control_plane_mutations_allowed() -> bool {
    !rpc_strict_mode()
}
