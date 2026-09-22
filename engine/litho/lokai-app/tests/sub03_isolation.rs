//! SUB-03 kernel Cargo isolation pins. Absence/inventory only; not ESTABLISHED. Not GATE-001.

use std::fs;
use std::path::{Path, PathBuf};

fn engine_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("read")
}

fn cargo_section(toml: &str, header: &str) -> String {
    let marker = format!("[{header}]");
    let start = toml
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let rest = &toml[start + marker.len()..];
    match rest.find("\n[") {
        Some(end) => rest[..end].to_string(),
        None => rest.to_string(),
    }
}

fn production_core_src(rel: &str) -> String {
    let src = read(engine_root().join("core/lokai-core/src").join(rel));
    let mut out = String::new();
    for line in src.lines() {
        if line.trim().starts_with("#[cfg(test)]") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn lists_crate(section: &str, name: &str) -> bool {
    section.lines().any(|line| {
        let t = line.trim();
        if t.starts_with('#') {
            return false;
        }
        t.starts_with(name) && t[name.len()..].starts_with([' ', '='])
    })
}

#[test]
fn sub03_lokai_core_production_toml_has_no_tools_or_transaction() {
    let toml = read(engine_root().join("core/lokai-core/Cargo.toml"));
    let deps = cargo_section(&toml, "dependencies");
    let dev = cargo_section(&toml, "dev-dependencies");
    assert!(
        !lists_crate(&deps, "lokai-tools"),
        "[dependencies] must not list lokai-tools: {deps}"
    );
    assert!(
        !lists_crate(&deps, "lokai-transaction"),
        "[dependencies] must not list lokai-transaction: {deps}"
    );
    // Distinctive: a [dev-dependencies] listing does not satisfy a [dependencies] pin.
    assert!(
        lists_crate(&dev, "lokai-tools") || lists_crate(&dev, "lokai-transaction"),
        "[dev-dependencies] may still list those crates; the pin is the [dependencies] section"
    );
    assert!(
        toml.contains("[dev-dependencies]"),
        "test must parse sections; whole-file grep is not enough"
    );
}

#[test]
fn sub03_production_agent_rs_has_no_tools_or_transaction_imports() {
    let agent = production_core_src("agent.rs");
    assert!(
        !agent.contains("lokai_tools::") && !agent.contains("use lokai_tools"),
        "production agent.rs must not import lokai_tools"
    );
    assert!(
        !agent.contains("lokai_transaction::") && !agent.contains("use lokai_transaction"),
        "production agent.rs must not import lokai_transaction"
    );
}

#[test]
fn sub03_arch_v4_sub_002_fixture_planted() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/lokai-arch-gate/fixtures/v4/ARCH-V4-SUB-002.v4fix");
    let text = read(fixture);
    assert!(text.contains("SUB-03"));
    assert!(text.contains("production-failing detector live"));
}
