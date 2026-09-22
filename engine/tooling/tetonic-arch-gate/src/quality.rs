//! Engineering quality static checks (QG-*). Does not duplicate architecture spawn/FS rules.

use std::path::{Path, PathBuf};

use crate::report::Finding;

/// Library crates that currently declare `anyhow` in `[dependencies]`: debt, not a license to add more.
pub const GRANDFATHER_ANYHOW: &[&str] = &[
    "lokai-core",
    "tetonic-core",
    "lokai-app",
    "tetonic-app",
    "lokai-artifact",
    "tetonic-artifact",
    "lokai-node",
    "tetonic-node",
];

/// Bins (and only these) may take a direct `anyhow` dependency.
pub const ALLOW_ANYHOW: &[&str] = &[
    "lokai-cli",
    "lokaid",
    "lokai-eval",
    "tetonic-eval",
    "lokai-bench",
    "tetonic-bench",
];

/// Crates whose *production* sources must not embed model product identifiers.
const MODEL_FORBIDDEN_PREFIXES: &[&str] = &[
    "core/lokai-core/src/",
    "core/tetonic-core/src/",
    "core/lokai-runtime/src/",
    "core/tetonic-runtime/src/",
    "mantle/lokai-orchestrator/src/",
    "mantle/tetonic-orchestrator/src/",
    "litho/lokai-app/src/",
    "litho/tetonic-app/src/",
];

pub fn repo_root(engine_root: &Path) -> PathBuf {
    engine_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| engine_root.to_path_buf())
}

pub fn run_quality(engine_root: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    out.extend(check_anyhow_direct(engine_root));
    out.extend(check_hardcoded_models(engine_root));
    out.extend(check_no_root_sprints(&repo_root(engine_root)));
    out
}

fn package_name_from_toml(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("name") {
            let rest = rest.trim().trim_start_matches('=').trim();
            let name = rest.trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        if t.starts_with('[') && t != "[package]" {
            break;
        }
    }
    None
}

fn production_dep_section(toml: &str) -> &str {
    let cut_dev = toml.split("[dev-dependencies]").next().unwrap_or(toml);
    cut_dev
        .split("[build-dependencies]")
        .next()
        .unwrap_or(cut_dev)
}

fn has_direct_anyhow(toml: &str) -> bool {
    production_dep_section(toml).lines().any(|line| {
        let t = line.trim();
        t.starts_with("anyhow ") || t.starts_with("anyhow=") || t == "anyhow"
    })
}

pub fn check_anyhow_direct(engine_root: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    for group in ["core", "mantle", "strata", "litho", "atmos", "tooling"] {
        let dir = engine_root.join(group);
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let cargo = entry.path().join("Cargo.toml");
            if !cargo.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&cargo).unwrap_or_default();
            let Some(name) = package_name_from_toml(&text) else {
                continue;
            };
            if !has_direct_anyhow(&text) {
                continue;
            }
            if ALLOW_ANYHOW.contains(&name.as_str()) || GRANDFATHER_ANYHOW.contains(&name.as_str())
            {
                continue;
            }
            out.push(Finding::new(
                "QG-ANYHOW-001",
                cargo,
                format!("package `{name}` declares a direct `anyhow` dependency"),
                "Library/runtime crates must use typed errors (`thiserror`). `anyhow` is for binary top-level context only.",
                "Remove `anyhow` from this Cargo.toml. Convert call sites to `thiserror`. Bins lokai-cli, lokaid, lokai-eval, tetonic-eval, lokai-bench, tetonic-bench may keep anyhow. Existing grandfathered crates are listed in docs/engineering/QUALITY-DEBT.md: do not copy that pattern.",
            ));
        }
    }
    out
}

fn is_model_forbidden_path(rel: &str) -> bool {
    if rel.contains("/tests/") || rel.ends_with("_tests.rs") || rel.contains("/benches/") {
        return false;
    }
    MODEL_FORBIDDEN_PREFIXES
        .iter()
        .any(|p| rel.replace('\\', "/").starts_with(p))
}

pub fn check_hardcoded_models(engine_root: &Path) -> Vec<Finding> {
    let re = regex::Regex::new(
        r#"(?i)"((?:llama-?3|gpt-4|gpt-3\.5|claude-[0-9]|mistral[-:]|qwen[0-9]|qwen:))"#,
    )
    .expect("regex");
    let mut out = Vec::new();
    for path in crate::collect_rs_files(engine_root) {
        let rel = path
            .strip_prefix(engine_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if !is_model_forbidden_path(&rel) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let production = strip_cfg_test_items(&text);
        if re.is_match(&production) {
            out.push(Finding::new(
                "QG-MODEL-001",
                path,
                "production source contains a literal model product identifier",
                "Model selection must come from RuntimeProfile, InferenceDefaults, recipes, or request input: not hardcoded in core/runtime/orchestrator/app routing.",
                "Take the model id from config or the request. Fixtures belong under #[cfg(test)]. Capacity recipes and eval corpus are allowed data layers (this rule does not scan them).",
            ));
        }
    }
    out
}

/// Drop `#[cfg(test)]` items/modules so a test-only helper earlier in a file
/// does not hide later production code. Does not interpret `cfg` on expressions.
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

fn is_cfg_test_attr(trimmed: &str) -> bool {
    trimmed == "#[cfg(test)]"
        || trimmed.starts_with("#[cfg(test),")
        || trimmed.starts_with("#[cfg(all(test")
        || trimmed.starts_with("#[cfg(any(test")
}

fn brace_count(line: &str, ch: char) -> i32 {
    line.chars().filter(|&c| c == ch).count() as i32
}

pub fn check_no_root_sprints(repo_root: &Path) -> Vec<Finding> {
    let sprints = repo_root.join("sprints");
    if sprints.is_dir() {
        vec![Finding::new(
            "QG-DOCS-001",
            sprints,
            "repository root contains a sprints/ directory",
            "Sprint tickets live under docs/epics/<epic>/sprints/, not a top-level sprints folder.",
            "Move the tree into docs/epics/<epic>/sprints/ and delete the root sprints/ directory.",
        )]
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn anyhow_forbidden_on_library_crate() {
        let dir = tempfile::tempdir().unwrap();
        let crate_dir = dir.path().join("core/lokai-domain");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"lokai-domain\"\n[dependencies]\nanyhow = { workspace = true }\n",
        )
        .unwrap();
        let v = check_anyhow_direct(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "QG-ANYHOW-001");
    }

    #[test]
    fn anyhow_allowed_on_cli() {
        let dir = tempfile::tempdir().unwrap();
        let crate_dir = dir.path().join("litho/lokai-cli");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"lokai-cli\"\n[dependencies]\nanyhow = \"1\"\n",
        )
        .unwrap();
        assert!(check_anyhow_direct(dir.path()).is_empty());
    }

    #[test]
    fn anyhow_grandfathered_core() {
        let dir = tempfile::tempdir().unwrap();
        let crate_dir = dir.path().join("core/lokai-core");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"lokai-core\"\n[dependencies]\nanyhow = { workspace = true }\n",
        )
        .unwrap();
        assert!(check_anyhow_direct(dir.path()).is_empty());
    }

    #[test]
    fn model_literal_in_core_production_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("core/lokai-core/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("route.rs"),
            "fn f() { let _ = \"qwen3.5:latest\"; }\n",
        )
        .unwrap();
        let v = check_hardcoded_models(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "QG-MODEL-001");
    }

    #[test]
    fn model_literal_in_core_tests_ok() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("core/lokai-core/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("route.rs"),
            "fn f() {}\n#[cfg(test)]\nmod tests { fn t() { let _ = \"qwen3.5:latest\"; } }\n",
        )
        .unwrap();
        assert!(check_hardcoded_models(dir.path()).is_empty());
    }

    #[test]
    fn cfg_test_helper_does_not_hide_later_production_literal() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("core/lokai-core/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("route.rs"),
            "#[cfg(test)]\nfn helper() {}\nfn prod() { let _ = \"qwen3:latest\"; }\n",
        )
        .unwrap();
        let v = check_hardcoded_models(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "QG-MODEL-001");
    }

    #[test]
    fn quality_passes_on_engine_tree() {
        let root = crate::engine_root();
        let v = run_quality(&root);
        assert!(v.is_empty(), "quality violations: {v:#?}");
    }

    #[test]
    fn root_sprints_fails() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sprints")).unwrap();
        let v = check_no_root_sprints(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "QG-DOCS-001");
    }

    #[test]
    fn oversized_source_fails_arch_size() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("crates/foo/src");
        fs::create_dir_all(&src).unwrap();
        let body: String = (0..901).map(|_| "x\n").collect();
        fs::write(src.join("lib.rs"), body).unwrap();
        let v = crate::check_file_sizes(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(crate::arch_id(v[0].rule), "ARCH-SIZE-001");
        assert!(v[0].detail.contains("901"));
    }

    #[test]
    fn command_new_outside_allowlist_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("core/lokai-core/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("x.rs"),
            "fn f() { let _ = std::process::Command::new(\"echo\"); }\n",
        )
        .unwrap();
        let v = crate::check_subprocess_spawn(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(crate::arch_id(v[0].rule), "ARCH-PROC-001");
    }
}
