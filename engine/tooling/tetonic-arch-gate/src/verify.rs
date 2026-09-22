//! Verification tiers: FAST / PACKAGE / FULL.

use std::path::Path;
use std::process::Command;

use crate::quality::run_quality;
use crate::report::Finding;
use crate::{engine_root, run_all};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyTier {
    Fast,
    Package,
    Full,
}

#[derive(Clone, Debug, Default)]
pub struct VerifyOpts {
    pub crate_name: Option<String>,
    pub skip_fmt: bool,
    pub skip_clippy: bool,
    pub skip_tests: bool,
}

pub struct VerifyReport {
    pub findings: Vec<Finding>,
    pub failed: bool,
}

fn cargo_status(engine: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(engine)
        .env("RUST_TEST_THREADS", "1")
        .args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    Err(format!("{stdout}{stderr}"))
}

fn cargo_finding(id: &str, engine: &Path, what: String, why: &str, how: &str) -> Finding {
    Finding::new(id, engine.to_path_buf(), what, why, how)
}

pub fn verify(tier: VerifyTier, opts: &VerifyOpts) -> VerifyReport {
    let engine = engine_root();
    let mut findings = Vec::new();

    if !opts.skip_fmt {
        eprintln!("== Formatting (QG-FMT-001) ==");
        if let Err(msg) = cargo_status(&engine, &["fmt", "--all", "--", "--check"]) {
            findings.push(cargo_finding(
                "QG-FMT-001",
                &engine,
                msg,
                "Unformatted Rust drifts under agent edits and hides real diffs.",
                "Run `cargo fmt --all` in engine/ and commit the result.",
            ));
        }
    }

    match tier {
        VerifyTier::Fast => {
            if let Some(name) = &opts.crate_name {
                eprintln!("== cargo check -p {name} (QG-CHECK-001) ==");
                if let Err(msg) = cargo_status(&engine, &["check", "-p", name, "--all-targets"]) {
                    findings.push(cargo_finding(
                        "QG-CHECK-001",
                        &engine,
                        msg,
                        "The affected crate must compile before IMPLEMENT continues.",
                        "Fix the compiler errors in the cited crate.",
                    ));
                }
            }
        }
        VerifyTier::Package | VerifyTier::Full => {
            if !opts.skip_clippy {
                eprintln!("== Clippy (QG-CLIPPY-001) ==");
                if let Err(msg) = cargo_status(
                    &engine,
                    &[
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--",
                        "-D",
                        "warnings",
                    ],
                ) {
                    findings.push(cargo_finding(
                        "QG-CLIPPY-001",
                        &engine,
                        msg,
                        "CI denies Clippy warnings (`-D warnings`). New warning debt is not allowed.",
                        "Fix the lint, or add a *localized* allow with a QUALITY-DEBT.md row (rule, location, reason, owner, expiry). Do not crate-level allow unwrap_used.",
                    ));
                }
            }
        }
    }

    eprintln!("== Architecture ==");
    findings.extend(run_all(&engine).into_iter().map(Finding::from_arch));

    eprintln!("== Quality static ==");
    findings.extend(run_quality(&engine));

    if tier == VerifyTier::Full && !opts.skip_tests {
        eprintln!("== Tests (QG-TEST-001) ==");
        if let Err(msg) = cargo_status(&engine, &["test", "--workspace"]) {
            findings.push(cargo_finding(
                "QG-TEST-001",
                &engine,
                msg,
                "FULL/CONVERGE requires workspace tests. Eval subsets remain CI jobs (lokai-eval), not this command, unless you run them separately.",
                "Fix failing tests. For eval: `cargo run -p lokai-eval -- run --subset quality --corpus corpus`.",
            ));
        }
    }

    VerifyReport {
        failed: !findings.is_empty(),
        findings,
    }
}

pub fn print_report(report: &VerifyReport) {
    let engine = engine_root();
    if report.findings.is_empty() {
        println!("engineering gate: OK ({})", engine.display());
        return;
    }
    eprintln!("engineering gate: {} finding(s)", report.findings.len());
    eprintln!();
    for f in &report.findings {
        f.print(&engine);
        eprintln!("----");
    }
}
