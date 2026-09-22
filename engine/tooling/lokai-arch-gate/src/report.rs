//! Actionable findings for humans and coding agents.

use std::path::{Path, PathBuf};

use crate::ids::{arch_how, arch_id, arch_why};
use crate::Violation;

#[derive(Debug, Clone)]
pub struct Finding {
    pub id: String,
    pub path: PathBuf,
    pub what: String,
    pub why: String,
    pub how: String,
}

impl Finding {
    pub fn new(
        id: impl Into<String>,
        path: PathBuf,
        what: impl Into<String>,
        why: impl Into<String>,
        how: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            path,
            what: what.into(),
            why: why.into(),
            how: how.into(),
        }
    }

    pub fn from_arch(v: Violation) -> Self {
        let id = arch_id(v.rule);
        Self {
            why: arch_why(v.rule).to_string(),
            how: arch_how(v.rule).to_string(),
            id: id.to_string(),
            path: v.path,
            what: v.detail,
        }
    }

    pub fn print(&self, engine_root: &Path) {
        let rel = self
            .path
            .strip_prefix(engine_root)
            .unwrap_or(&self.path)
            .display();
        eprintln!("{}: {}", self.id, short_title(&self.id));
        eprintln!();
        eprintln!("{rel}");
        eprintln!();
        eprintln!("{}", self.what);
        eprintln!();
        eprintln!("{}", self.why);
        eprintln!();
        eprintln!("{}", self.how);
        eprintln!();
        eprintln!(
            "See: docs/engineering/QUALITY-GATE.md#{}",
            self.id.to_lowercase()
        );
        eprintln!();
    }
}

fn short_title(id: &str) -> &'static str {
    match id {
        "QG-FMT-001" => "rustfmt check failed",
        "QG-CLIPPY-001" => "clippy -D warnings failed",
        "QG-CHECK-001" => "cargo check failed",
        "QG-TEST-001" => "cargo test --workspace failed",
        "QG-ANYHOW-001" => "direct anyhow dependency forbidden in this crate",
        "QG-MODEL-001" => "hardcoded model id in a routing/runtime crate",
        "QG-DOCS-001" => "forbidden top-level sprints/ directory",
        "ARCH-SIZE-001" => "source file exceeds 900 lines",
        "ARCH-PROC-001" => "Command::new outside process allowlist",
        _ => "engineering gate violation",
    }
}
