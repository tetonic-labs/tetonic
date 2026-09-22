//! M0 freeze: trip new dual-authority mutants. Owner packages empty their allowlists at CONVERGE.
//!
//! Scans skip `tooling/lokai-arch-gate/` so this file may name the needles.

use std::path::{Path, PathBuf};

use crate::{collect_rs_files, rel, Violation};

const SECRETS_REDACT_DEF: &str = "core/lokai-secrets/src/lib.rs";

/// Concatenated so freeze source is not a production hit if skip ever regresses.
fn old_verify_needle() -> String {
    format!("{}{}", "LOKAI_USE_OLD", "_VERIFY")
}

fn norm(rel_path: &str) -> String {
    rel_path.replace('\\', "/")
}

fn skip_freeze_rel(rel_path: &str) -> bool {
    let n = norm(rel_path);
    n.contains("tooling/lokai-arch-gate/")
        || n.contains("/tests/")
        || n.ends_with("_tests.rs")
        || n.contains("/benches/")
}

fn is_cfg_test_attr(trimmed: &str) -> bool {
    trimmed == "#[cfg(test)]"
        || trimmed.starts_with("#[cfg(test),")
        || trimmed.starts_with("#[cfg(all(test")
        || trimmed.starts_with("#[cfg(any(test")
}

fn brace_count(line: &str, ch: char) -> i32 {
    line.chars().filter(|&c| c == ch).count() as i32
}

/// Drop `#[cfg(test)]` items/modules (same approach as quality.rs).
fn strip_cfg_test_items(text: &str) -> String {
    let mut out = String::new();
    let mut skip_depth: Option<i32> = None;
    let mut awaiting_item = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if skip_depth.is_none() && !awaiting_item && is_cfg_test_attr(trimmed) {
            awaiting_item = true;
            continue;
        }
        if awaiting_item && skip_depth.is_none() {
            if trimmed.starts_with("#[") {
                continue;
            }
            awaiting_item = false;
            let opens = brace_count(line, '{');
            let closes = brace_count(line, '}');
            if opens == 0 && closes == 0 {
                continue;
            }
            let depth = opens - closes;
            if depth > 0 {
                skip_depth = Some(depth);
            }
            continue;
        }
        if let Some(depth) = skip_depth {
            let depth = depth + brace_count(line, '{') - brace_count(line, '}');
            skip_depth = if depth <= 0 { None } else { Some(depth) };
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub(crate) fn production_text(text: &str) -> String {
    let stripped = strip_cfg_test_items(text);
    let mut out = String::new();
    for line in stripped.lines() {
        if line.trim().starts_with("//") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn ends_with_norm(rel_path: &str, suffix: &str) -> bool {
    norm(rel_path).ends_with(suffix)
}

fn walk_ext(dir: &Path, out: &mut Vec<PathBuf>, exts: &[&str]) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == ".git" || name == "fixtures" {
                continue;
            }
            walk_ext(&path, out, exts);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            out.push(path);
        }
    }
}

pub fn run_freeze(root: &Path) -> Vec<Violation> {
    let mut v = Vec::new();
    v.extend(check_tool_new_sites(root));
    v.extend(fail_open_redact_new(root));
    v.extend(core_raw_read_new(root));
    v.extend(no_old_verify_flag(root));
    v.extend(egress_hygiene_new(root));
    v.extend(app_door_new(root));
    v.extend(product_boundary_new(root));
    v.extend(inspect_door_new(root));
    v
}

pub fn check_tool_new_sites(root: &Path) -> Vec<Violation> {
    let ident = regex::Regex::new(r"\bcheck_tool\b").expect("regex");
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) {
            continue;
        }
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if ident.is_match(&text) {
            out.push(Violation {
                rule: "check_tool_new_sites",
                path: path.clone(),
                detail: "production check_tool site; dual policy deleted (DEL-004 / M5)".into(),
            });
        }
    }
    out
}

pub fn fail_open_redact_new(root: &Path) -> Vec<Violation> {
    let defn = regex::Regex::new(r"\bfn\s+redact_text_sync\b").expect("regex");
    let arm_tuple =
        regex::Regex::new(r"Err\s*\(\s*_\s*\)\s*=>\s*\(\s*text\.to_string\(\)\s*,\s*false\s*\)")
            .expect("regex");
    let arm_plain =
        regex::Regex::new(r"Err\s*\(\s*_\s*\)\s*=>\s*text\.to_string\(\)").expect("regex");
    let unwrap_tuple = regex::Regex::new(
        r"unwrap_or(?:_else\s*\(\s*\|_?\s*\|)?\s*\(\s*\(?\s*text\.to_string\(\)\s*,\s*false\s*\)",
    )
    .expect("regex");
    let unwrap_plain = regex::Regex::new(
        r"unwrap_or_else\s*\(\s*\|_?\s*\|\s*text\.to_string\(\)\s*\)|unwrap_or\s*\(\s*text\.to_string\(\)\s*\)",
    )
    .expect("regex");
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) {
            continue;
        }
        let def_ok = ends_with_norm(&rel_path, SECRETS_REDACT_DEF);
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if defn.is_match(&text) && !def_ok {
            out.push(Violation {
                rule: "fail_open_redact_new",
                path: path.clone(),
                detail: "fn redact_text_sync may be defined only in lokai-secrets/src/lib.rs"
                    .into(),
            });
        }
        if arm_tuple.is_match(&text)
            || arm_plain.is_match(&text)
            || unwrap_tuple.is_match(&text)
            || unwrap_plain.is_match(&text)
        {
            out.push(Violation {
                rule: "fail_open_redact_new",
                path: path.clone(),
                detail: "fail-open plaintext redact copy (DEL-019 / M4); lib.rs is not exempt"
                    .into(),
            });
        }
    }
    out
}

pub fn core_raw_read_new(root: &Path) -> Vec<Violation> {
    let tokio_fs = regex::Regex::new(r"tokio::fs::").expect("regex");
    let std_read = regex::Regex::new(r"std::fs::read\s*\(").expect("regex");
    let std_rts = regex::Regex::new(r"std::fs::read_to_string\s*\(").expect("regex");
    let std_rte = regex::Regex::new(r"std::fs::read_to_end\s*\(").expect("regex");
    let mut out = Vec::new();
    let core_src = root.join("core/lokai-core/src");
    for path in collect_rs_files(&core_src) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) {
            continue;
        }
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if tokio_fs.is_match(&text)
            || std_read.is_match(&text)
            || std_rts.is_match(&text)
            || std_rte.is_match(&text)
        {
            out.push(Violation {
                rule: "core_raw_read_new",
                path: path.clone(),
                detail: "raw FS read in lokai-core bypasses jailed read_file (DEL-005 / M3)".into(),
            });
        }
    }
    out
}

pub fn no_old_verify_flag(root: &Path) -> Vec<Violation> {
    let needle = old_verify_needle();
    let mut files = Vec::new();
    walk_ext(root, &mut files, &["rs", "toml", "yml", "yaml"]);
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        // .rs: skip comments / cfg(test); toml/yml: whole file.
        let hay = if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            production_text(&text)
        } else {
            text
        };
        if hay.contains(&needle) {
            out.push(Violation {
                rule: "no_old_verify_flag",
                path,
                detail: "do not add a verify dual-path env flag; M2 owns verify behavior".into(),
            });
        }
    }
    out
}

fn legacy_chat_env_needle() -> String {
    format!("{}{}", "LOKAI_FABRIC_LEGACY", "_CHAT_ONLY")
}

fn is_worker_infer_assembly(rel_path: &str) -> bool {
    let n = norm(rel_path);
    n.ends_with("mantle/lokai-node/src/fabric.rs") || n.ends_with("litho/lokaid/src/node.rs")
}

/// M7: no empty worker Infer/capacity `EgressGuard::new()`, no legacy chat env.
pub fn egress_hygiene_new(root: &Path) -> Vec<Violation> {
    let env_needle = legacy_chat_env_needle();
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) {
            continue;
        }
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if text.contains(&env_needle) {
            out.push(Violation {
                rule: "egress_hygiene_new",
                path: path.clone(),
                detail: "LOKAI_FABRIC_LEGACY_CHAT_ONLY production read (DEL-014 / M7)".into(),
            });
        }
        if is_worker_infer_assembly(&rel_path) && text.contains("EgressGuard::new()") {
            out.push(Violation {
                rule: "egress_hygiene_new",
                path,
                detail: "worker Infer/capacity client must pin via pinned_to_inference_url, not EgressGuard::new() (M7 / I14)".into(),
            });
        }
    }
    out
}

fn token_prefix_needle() -> String {
    format!("{}{}", "LOKAI_RPC_TOKEN", "=")
}

fn is_lokaid_src(rel_path: &str) -> bool {
    norm(rel_path).contains("litho/lokaid/")
}

/// M8: lokaid handlers do not skip to execute_turn;
/// production lokaid does not eprintln the token prefix.
pub fn app_door_new(root: &Path) -> Vec<Violation> {
    let token = token_prefix_needle();
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) || !is_lokaid_src(&rel_path) {
            continue;
        }
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if text.contains("turn_execution::execute_turn") {
            out.push(Violation {
                rule: "app_door_new",
                path: path.clone(),
                detail:
                    "lokaid must call RunService::run_turn, not turn_execution::execute_turn (M8)"
                        .into(),
            });
        }
        if text.contains(&token) {
            out.push(Violation {
                rule: "app_door_new",
                path: path.clone(),
                detail: "production lokaid must not eprintln the RPC token prefix (M8)".into(),
            });
        }
    }
    out
}

fn toml_has_crate_dep(text: &str, name: &str) -> bool {
    let eq = format!("{name} =");
    let eq_tight = format!("{name}=");
    text.lines().any(|l| {
        let t = l.trim();
        t.starts_with(&eq) || t.starts_with(&eq_tight)
    })
}

fn is_core_or_runtime_src(rel_path: &str) -> bool {
    let n = norm(rel_path);
    n.contains("core/lokai-core/src/") || n.contains("core/lokai-runtime/src/")
}

fn is_orchestrator_src(rel_path: &str) -> bool {
    norm(rel_path).contains("mantle/lokai-orchestrator/src/")
}

/// M9: generic loop must not name coding crates; coder/critic is not orchestrator identity.
pub fn product_boundary_new(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) {
            continue;
        }
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if is_core_or_runtime_src(&rel_path)
            && (text.contains("lokai_index::") || text.contains("lokai_lsp::"))
        {
            out.push(Violation {
                rule: "product_boundary_new",
                path: path.clone(),
                detail:
                    "lokai-core / lokai-runtime must not name lokai_index:: or lokai_lsp:: (M9 I11)"
                        .into(),
            });
        }
        if is_orchestrator_src(&rel_path) && text.contains("SpecialistRole") {
            out.push(Violation {
                rule: "product_boundary_new",
                path: path.clone(),
                detail: "SpecialistRole must not be orchestrator identity; use RoleId + pack (M9)"
                    .into(),
            });
        }
    }
    for rel_toml in [
        "core/lokai-core/Cargo.toml",
        "core/lokai-runtime/Cargo.toml",
    ] {
        let path = root.join(rel_toml);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if toml_has_crate_dep(&text, "lokai-index") || toml_has_crate_dep(&text, "lokai-lsp") {
            out.push(Violation {
                rule: "product_boundary_new",
                path,
                detail: "lokai-core / lokai-runtime Cargo.toml must not depend on lokai-index or lokai-lsp (M9)"
                    .into(),
            });
        }
    }
    out
}

const INSPECT_RUN_HANDLER: &str = "litho/lokaid/src/daemon/handlers/run.rs";
const INSPECT_EVAL_RECOVERY: &str = "tooling/lokai-eval/src/recovery.rs";

fn is_inspect_client_file(rel_path: &str) -> bool {
    ends_with_norm(rel_path, INSPECT_RUN_HANDLER) || ends_with_norm(rel_path, INSPECT_EVAL_RECOVERY)
}

/// M10: daemon run handlers and eval recovery inspect via Application, not pub supervisor.
/// Do not scan workspace-wide — FSM / fabric_run_bridge still call snapshot.
pub fn inspect_door_new(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if skip_freeze_rel(&rel_path) || !is_inspect_client_file(&rel_path) {
            continue;
        }
        let text = production_text(&std::fs::read_to_string(&path).unwrap_or_default());
        if text.contains(".supervisor.snapshot") {
            out.push(Violation {
                rule: "inspect_door_new",
                path: path.clone(),
                detail: "client inspect must use inspect_run, not .supervisor.snapshot (M10)"
                    .into(),
            });
        }
        if text.contains(".supervisor.resume_from_sequence") {
            out.push(Violation {
                rule: "inspect_door_new",
                path: path.clone(),
                detail:
                    "client resume must use resume_events, not .supervisor.resume_from_sequence (M10)"
                        .into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_all;
    use std::fs;

    fn write(path: &Path, body: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn run_all_reports_planted_check_tool() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            "fn f(policy: P) { policy.check_tool(&ctx, name, args); }\n",
        );
        let v = run_all(dir.path());
        assert!(
            v.iter().any(|x| x.rule == "check_tool_new_sites"),
            "run_all must include freeze; got {v:#?}"
        );
    }

    #[test]
    fn check_tool_new_sites_catches_foreign_crate() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            "fn f(policy: P) { policy.check_tool(&ctx, name, args); }\n",
        );
        let v = check_tool_new_sites(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "check_tool_new_sites");
    }

    #[test]
    fn check_tool_cap_on_agent() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-core/src/agent.rs"),
            "fn a() { p.check_tool(&c, n, a); }\n",
        );
        let v = check_tool_new_sites(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "check_tool_new_sites");
    }

    #[test]
    fn fail_open_redact_catches_new_fn() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            "pub fn redact_text_sync(s: &S, text: &str) -> (String, bool) { (text.into(), false) }\n",
        );
        let v = fail_open_redact_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "fail_open_redact_new");
    }

    #[test]
    fn fail_open_redact_catches_err_arm() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            "fn f(text: &str) { let _ = match r { Ok(x) => x, Err(_) => (text.to_string(), false) }; }\n",
        );
        let v = fail_open_redact_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "fail_open_redact_new");
    }

    #[test]
    fn fail_open_redact_catches_lib_rs_plaintext_arm() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-secrets/src/lib.rs"),
            "fn f(text: &str) { let _ = match r { Ok(x) => x, Err(_) => (text.to_string(), false) }; }\n",
        );
        let v = fail_open_redact_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "fail_open_redact_new");
    }

    #[test]
    fn fail_open_redact_catches_unwrap_or_tuple() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            "fn f(text: &str) { let _ = r.unwrap_or((text.to_string(), false)); }\n",
        );
        let v = fail_open_redact_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "fail_open_redact_new");
    }

    #[test]
    fn fail_open_redact_catches_unwrap_or_else_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            "fn f(text: &str) { let _ = r.unwrap_or_else(|_| text.to_string()); }\n",
        );
        let v = fail_open_redact_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "fail_open_redact_new");
    }

    #[test]
    fn core_raw_read_catches_new_file() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-core/src/evil.rs"),
            "async fn f(p: P) { let _ = tokio::fs::read_to_string(p).await; }\n",
        );
        let v = core_raw_read_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "core_raw_read_new");
    }

    #[test]
    fn core_raw_read_ignores_metadata() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-core/src/agent.rs"),
            "fn f(p: &P) { let _ = std::fs::metadata(p); }\n",
        );
        let v = core_raw_read_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn core_raw_read_catches_agent_prefetch() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-core/src/agent.rs"),
            "async fn f(p: P) { let _ = tokio::fs::read_to_string(p).await; }\n",
        );
        let v = core_raw_read_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "core_raw_read_new");
    }

    #[test]
    fn no_old_verify_flag_catches_env() {
        let dir = tempfile::tempdir().unwrap();
        let flag = old_verify_needle();
        write(
            &dir.path().join("litho/lokai-app/src/x.rs"),
            &format!("const F: &str = \"{flag}\";\n"),
        );
        let v = no_old_verify_flag(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "no_old_verify_flag");
    }

    #[test]
    fn egress_hygiene_catches_empty_default_ollama() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("mantle/lokai-node/src/fabric.rs"),
            "pub fn default_ollama(base: &str) { EgressGuard::new(); }\n",
        );
        let v = egress_hygiene_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "egress_hygiene_new");
    }

    #[test]
    fn egress_hygiene_catches_legacy_chat_env() {
        let dir = tempfile::tempdir().unwrap();
        let flag = legacy_chat_env_needle();
        write(
            &dir.path().join("mantle/lokai-node/src/fabric.rs"),
            &format!("let force = std::env::var(\"{flag}\");\n"),
        );
        let v = egress_hygiene_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "egress_hygiene_new");
    }

    #[test]
    fn egress_hygiene_allows_coordinator_new() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path()
                .join("litho/lokaid/src/daemon/handlers/initialize.rs"),
            "let guard = Arc::new(EgressGuard::new());\n",
        );
        let v = egress_hygiene_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn egress_hygiene_allows_pinned_worker() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("mantle/lokai-node/src/fabric.rs"),
            "Arc::new(EgressGuard::pinned_to_inference_url(base))\n",
        );
        let v = egress_hygiene_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn app_door_catches_execute_turn_in_lokaid() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokaid/src/daemon/handlers/chat.rs"),
            "app.turn_execution::execute_turn(req).await\n",
        );
        let v = app_door_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "app_door_new");
    }

    #[test]
    fn app_door_catches_token_prefix_in_lokaid() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokaid/src/main.rs"),
            &format!(
                "eprintln!(\"lokaid: {}{{}}\", token);\n",
                token_prefix_needle()
            ),
        );
        let v = app_door_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "app_door_new");
    }

    #[test]
    fn app_door_allows_inner_execute_turn() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/run_service.rs"),
            "turn_execution::execute_turn(req).await\n",
        );
        let v = app_door_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn app_door_allows_token_env_read_without_prefix() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokaid/src/daemon/config.rs"),
            "std::env::var(\"LOKAI_RPC_TOKEN\")\n",
        );
        let v = app_door_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn app_door_skips_lokaid_tests() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokaid/src/daemon/tests/cases.rs"),
            "turn_execution::execute_turn\n",
        );
        let v = app_door_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn product_boundary_catches_index_in_core_src() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-core/src/agent.rs"),
            "let _ = lokai_index::Index::open(p);\n",
        );
        let v = product_boundary_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "product_boundary_new");
    }

    #[test]
    fn product_boundary_catches_runtime_cargo_dep() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("core/lokai-runtime/Cargo.toml"),
            "lokai-index = { path = \"../lokai-index\" }\n",
        );
        let v = product_boundary_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "product_boundary_new");
    }

    #[test]
    fn product_boundary_catches_specialist_role_enum() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path()
                .join("mantle/lokai-orchestrator/src/specialist.rs"),
            "pub enum SpecialistRole { Coder, Critic }\n",
        );
        let v = product_boundary_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "product_boundary_new");
    }

    #[test]
    fn product_boundary_allows_app_index_and_runtime_tests() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/coding_pack.rs"),
            "let _ = lokai_index::FilesystemCodeIndex;\n",
        );
        write(
            &dir.path()
                .join("core/lokai-runtime/tests/metadata_graph.rs"),
            "if *name == \"lokai-index\" {}\n",
        );
        write(
            &dir.path().join("core/lokai-core/Cargo.toml"),
            "lokai-tools = { path = \"../lokai-tools\" }\n",
        );
        let v = product_boundary_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn inspect_door_catches_snapshot_in_run_handler() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokaid/src/daemon/handlers/run.rs"),
            "app.supervisor.snapshot(id).await\n",
        );
        let v = inspect_door_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "inspect_door_new");
    }

    #[test]
    fn inspect_door_catches_resume_in_eval_recovery() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("tooling/lokai-eval/src/recovery.rs"),
            "app.supervisor.resume_from_sequence(id, 0, 8).await\n",
        );
        let v = inspect_door_new(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "inspect_door_new");
    }

    #[test]
    fn inspect_door_allows_fabric_bridge_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("litho/lokai-app/src/fabric_run_bridge.rs"),
            "self.supervisor.snapshot(RunId::new(run_id)).await\n",
        );
        let v = inspect_door_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn inspect_door_skips_protocol_golden_canary() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path()
                .join("litho/lokaid/src/daemon/tests/protocol_golden.rs"),
            "!src.contains(\".supervisor.snapshot\")\n",
        );
        let v = inspect_door_new(dir.path());
        assert!(v.is_empty(), "{v:#?}");
    }
}
