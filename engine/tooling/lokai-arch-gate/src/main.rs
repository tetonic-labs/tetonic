//! Architecture + engineering quality gate CLI.

use clap::{Parser, Subcommand, ValueEnum};
use lokai_arch_gate::report::Finding;
use lokai_arch_gate::verify::{print_report, verify, VerifyOpts, VerifyTier};
use lokai_arch_gate::{engine_root, run_all};

#[derive(Parser)]
#[command(name = "lokai-arch-gate")]
#[command(about = "Lokai architecture and engineering quality gate")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Architecture invariants only (historical default).
    Arch,
    /// Quality static checks (anyhow, model literals, docs layout). Does not run rustfmt/clippy.
    Quality,
    /// Combined verification tier.
    Verify {
        #[arg(value_enum)]
        tier: Tier,
        /// FAST: also `cargo check -p <crate>`
        #[arg(long = "crate")]
        crate_name: Option<String>,
        #[arg(long)]
        skip_fmt: bool,
        #[arg(long)]
        skip_clippy: bool,
        #[arg(long)]
        skip_tests: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Tier {
    Fast,
    Package,
    Full,
}

fn main() {
    let cli = Cli::parse();
    let root = engine_root();
    match cli.command {
        None | Some(Command::Arch) => {
            let findings: Vec<Finding> =
                run_all(&root).into_iter().map(Finding::from_arch).collect();
            print_findings(&findings, &root);
        }
        Some(Command::Quality) => {
            let findings = lokai_arch_gate::quality::run_quality(&root);
            print_findings(&findings, &root);
        }
        Some(Command::Verify {
            tier,
            crate_name,
            skip_fmt,
            skip_clippy,
            skip_tests,
        }) => {
            let tier = match tier {
                Tier::Fast => VerifyTier::Fast,
                Tier::Package => VerifyTier::Package,
                Tier::Full => VerifyTier::Full,
            };
            let report = verify(
                tier,
                &VerifyOpts {
                    crate_name,
                    skip_fmt,
                    skip_clippy,
                    skip_tests,
                },
            );
            print_report(&report);
            if report.failed {
                std::process::exit(1);
            }
        }
    }
}

fn print_findings(findings: &[Finding], root: &std::path::Path) {
    if findings.is_empty() {
        println!("architecture/quality gate: OK ({})", root.display());
        return;
    }
    eprintln!("engineering gate: {} finding(s)", findings.len());
    eprintln!();
    for f in findings {
        f.print(root);
        eprintln!("----");
    }
    std::process::exit(1);
}
