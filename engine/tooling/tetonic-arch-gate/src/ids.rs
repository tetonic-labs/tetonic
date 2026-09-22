//! Stable rule IDs for architecture checks (ARCH-*). Quality IDs live in `quality`.

/// Map the historical short rule name to a stable ARCH-* ID.
pub fn arch_id(rule: &str) -> &'static str {
    match rule {
        "subprocess_spawn" => "ARCH-PROC-001",
        "git_via_process_broker" => "ARCH-PROC-002",
        "lsp_via_process_broker" => "ARCH-PROC-003",
        "production_tools_sandboxed" => "ARCH-PROC-004",
        "reqwest_boundary" => "ARCH-NET-001",
        "engine_runtime" => "ARCH-RT-001",
        "dependency_direction" => "ARCH-DEP-001",
        "app_layer_deps" => "ARCH-DEP-002",
        "inference_no_enroll" => "ARCH-DEP-003",
        "fabric_protocol_isolation" => "ARCH-DEP-004",
        "schema_methods" => "ARCH-RPC-001",
        "file_size" => "ARCH-SIZE-001",
        "unguarded_remote_dispatch" => "ARCH-INFER-001",
        "compute_broker_wiring" => "ARCH-INFER-002",
        "lokaid_session_authority" => "ARCH-APP-001",
        "app_workflow_delegation" => "ARCH-APP-002",
        "cli_workflow_delegation" => "ARCH-APP-003",
        "lokaid_no_direct_orchestration" => "ARCH-APP-004",
        "cli_no_direct_orchestration" => "ARCH-APP-005",
        "lokaid_no_capacity_workflow" => "ARCH-APP-006",
        "no_gates_ok_turn_abort" => "ARCH-APP-007",
        "no_duplicate_resume_cap" => "ARCH-APP-008",
        "no_duplicate_enrollment_helpers" => "ARCH-APP-009",
        "cli_inspector_no_command" => "ARCH-APP-010",
        "cli_infra_leftovers" => "ARCH-APP-011",
        "app_layer_isolation" => "ARCH-APP-012",
        "app_layer_io" => "ARCH-APP-013",
        "app_door_new" => "ARCH-APP-014",
        "inspect_door_new" => "ARCH-APP-015",
        "product_boundary_new" => "ARCH-PROD-001",
        "workspace_mutations" | "workspace_mutation_bypass" => "ARCH-FS-001",
        "run_state_mutations" | "run_state_mutation_bypass" => "ARCH-RUN-001",
        "outbound_secret_scanner" => "ARCH-OUT-001",
        "fail_open_redact_new" => "ARCH-OUT-002",
        "check_tool_new_sites" => "ARCH-POL-001",
        "core_raw_read_new" => "ARCH-READ-001",
        "no_old_verify_flag" => "ARCH-PROC-005",
        "egress_hygiene_new" => "ARCH-EGRESS-001",
        "context_compiler_wired" => "ARCH-CTX-001",
        "async_sync_calls" => "ARCH-ASYNC-001",
        "no_mutex_store" => "ARCH-ASYNC-002",
        "fabric_client_uses_protocol" | "fabric_client_protocol" => "ARCH-FAB-001",
        other => {
            let _ = other;
            "ARCH-UNKNOWN"
        }
    }
}

pub fn arch_why(rule: &str) -> &'static str {
    match rule {
        "subprocess_spawn" | "git_via_process_broker" | "lsp_via_process_broker"
        | "production_tools_sandboxed" => {
            "Process spawn must go through the sandbox/process executor, not ad-hoc Command::new."
        }
        "check_tool_new_sites" => {
            "Production check_tool reopens dual policy (DEL-004). ARCH-POL-001 allowlist emptied at M5 CONVERGE."
        }
        "fail_open_redact_new" => {
            "Public fail-open redact copies INV-OUT-001. M4 CONVERGE removed the lib.rs allowlist; any plaintext Err / unwrap_or copy is a violation."
        }
        "core_raw_read_new" => {
            "Raw FS reads in lokai-core bypass jailed read_file (DEL-005). Prefetch allowlist removed at M3 CONVERGE."
        }
        "no_old_verify_flag" => {
            "A LOKAI_USE_OLD_VERIFY flag would freeze a dual verify path. M2 owns verify behavior."
        }
        "egress_hygiene_new" => {
            "Empty worker Infer EgressGuard or LOKAI_FABRIC_LEGACY_CHAT_ONLY reopens I14 / DEL-014. Coordinator new()+reload is out of this tripwire."
        }
        "app_door_new" => {
            "lokaid execute_turn, serve.py INSECURE, or production token-prefix eprintln reopens the M8 door mutants. Does not prove INV-APP-001 ESTABLISHED."
        }
        "inspect_door_new" => {
            "Daemon run handlers or eval recovery calling .supervisor.snapshot / resume_from_sequence skips the M10 inspect door. Does not prove INV-APP-001 ESTABLISHED. fabric_run_bridge snapshot is out of this tripwire."
        }
        "product_boundary_new" => {
            "core/runtime naming lokai-index/lokai-lsp or SpecialistRole as orchestrator identity reopens I11. Does not prove INV-PROD-001 ESTABLISHED. Bins may still Index::open (DEL-036 leftover)."
        }
        "reqwest_boundary" => {
            "HTTP clients must use lokai-egress so default-deny and allowlists stay one authority."
        }
        "file_size" => {
            "900-line limit is a maintainability guardrail so files stay reviewable. It is not proof of good architecture: do not split modules only to beat the counter."
        }
        "dependency_direction" | "app_layer_deps" | "inference_no_enroll" => {
            "Forbidden Cargo edges invert the intended layering (contracts vs inference vs app)."
        }
        "async_sync_calls" | "no_mutex_store" => {
            "Blocking the Tokio worker or reintroducing Mutex<Store> causes Infer/RPC stalls."
        }
        _ => "This check encodes a repository architecture invariant. See lokai-arch-gate README and docs/engineering/QUALITY-GATE.md.",
    }
}

pub fn arch_how(rule: &str) -> &'static str {
    match rule {
        "file_size" => {
            "Extract a focused submodule. If this file is already on the documented allowlist, do not add new oversized files; split instead of expanding the allowlist."
        }
        "subprocess_spawn" => {
            "Route through lokai-sandbox / ProcessExecutor. Do not add this file to the spawn allowlist without an architecture exception."
        }
        "check_tool_new_sites" => {
            "Route through evaluate_action / ActionBroker. Do not add check_tool sites."
        }
        "fail_open_redact_new" => {
            "Call redact_text_sync or refuse on scanner Err. Do not copy plaintext Err / unwrap_or arms. Definition stays in lokai-secrets/src/lib.rs only."
        }
        "core_raw_read_new" => {
            "Read through Workspace::resolve / jailed read_file. Do not add tokio::fs or std::fs::read in lokai-core."
        }
        "no_old_verify_flag" => {
            "Remove the flag. Verify behavior changes only in M2."
        }
        "egress_hygiene_new" => {
            "Pin worker Infer/capacity via EgressGuard::pinned_to_inference_url. Do not read LOKAI_FABRIC_LEGACY_CHAT_ONLY. Coordinator enroll may still new() then allow_node."
        }
        "app_door_new" => {
            "Call RunService::run_turn from lokaid handlers. Do not eprintln the RPC token prefix."
        }
        "inspect_door_new" => {
            "Call inspect_run / resume_events from handlers/run.rs and lokai-eval recovery.rs. Do not restore .supervisor.snapshot there. Do not ban snapshot workspace-wide."
        }
        "product_boundary_new" => {
            "Keep index/lsp out of lokai-core / lokai-runtime src and Cargo.toml. Role names live in the product pack (RoleId), not SpecialistRole in the orchestrator."
        }
        _ => "Fix the cited file or Cargo.toml so the invariant holds. See docs/engineering/QUALITY-GATE.md.",
    }
}
