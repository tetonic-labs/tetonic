use clap::{Parser, Subcommand};
use std::path::PathBuf;

use tetonic_eval::{
    compare, corpus, deterministic, gate, kernel, manifest, statistical, suite, traits,
};

#[derive(Parser)]
#[command(
    name = "tetonic-eval",
    version = "0.1.0",
    author,
    about = "Tetonic Evaluation Harness"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run an evaluation scenario or a named CI subset
    Run {
        #[arg(long)]
        mode: Option<String>,
        /// Path to one evaluation manifest JSON
        #[arg(short, long)]
        manifest: Option<PathBuf>,
        /// Corpus scenario id (`01-small-single-lang`, …)
        #[arg(long)]
        corpus_id: Option<String>,
        /// Named subset: `pr`, `quality`, or `security`
        #[arg(long)]
        subset: Option<String>,
        /// Corpus root (default: `engine/corpus` or `$LOKAI_CORPUS`)
        #[arg(long)]
        corpus: Option<PathBuf>,
        /// Write the JSON result to this path
        #[arg(long)]
        out: Option<PathBuf>,
        /// Explicitly overwrite a baseline file with this run (never silent)
        #[arg(long)]
        write_baseline: Option<PathBuf>,
        /// Disable H1-1 outbound scanning (negative tests only)
        #[arg(long)]
        no_scan: bool,
    },
    /// Compare two evaluation results and report regressions
    Compare {
        #[arg(short, long)]
        baseline: PathBuf,
        #[arg(short, long)]
        candidate: PathBuf,
    },
    /// Verify `corpus/fixtures/*` against `corpus/digests.json`
    Integrity {
        #[arg(long)]
        corpus: Option<PathBuf>,
        /// Rewrite digests.json from the current fixtures
        #[arg(long)]
        write: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = tetonic_telemetry::sanitization::init_subscriber(
        tetonic_telemetry::sanitization::DiagnosticMode::Safe,
    );
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    let cli = Cli::parse();
    let root_ctx = tetonic_telemetry::TraceContext::default();
    let span = tracing::info_span!("lokai_eval_root");
    let _enter = span.enter();
    tetonic_telemetry::inject_context(root_ctx);

    let local = tokio::task::LocalSet::new();
    local.run_until(async move { dispatch(cli).await }).await
}

async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Run {
            mode,
            manifest,
            corpus_id,
            subset,
            corpus,
            out,
            write_baseline,
            no_scan,
        } => {
            let corpus_root = corpus::resolve_corpus_root(corpus.as_deref())?;
            let fs = corpus::FilesystemCorpus::new(corpus_root.clone());
            let orchestrator: Box<dyn traits::AgentOrchestrator> = if no_scan {
                Box::new(kernel::KernelOrchestrator::without_scan())
            } else {
                Box::new(kernel::KernelOrchestrator::recorded())
            };

            let run_mode = parse_mode(mode.as_deref())?;
            let json = if let Some(name) = subset {
                let ids = suite::parse_subset(&name)?;
                let mode = run_mode.unwrap_or(manifest::EvaluationMode::Deterministic);
                let result = suite::run_ids(
                    &corpus_root,
                    ids,
                    &name,
                    mode,
                    &fs,
                    &fs,
                    orchestrator.as_ref(),
                )
                .await?;
                if !result.passed {
                    let text = serde_json::to_string_pretty(&result)?;
                    emit(&text, out.as_deref(), write_baseline.as_deref())?;
                    anyhow::bail!(
                        "subset {} failed pass-rate floor {:.2} (got {:.2})",
                        name,
                        gate::PASS_RATE_FLOOR,
                        result.pass_rate
                    );
                }
                serde_json::to_string_pretty(&result)?
            } else {
                let eval_manifest =
                    load_one(&corpus_root, manifest.as_deref(), corpus_id.as_deref())?;
                let inferred = run_mode.unwrap_or_else(|| eval_manifest.mode.clone());
                let result = match inferred {
                    manifest::EvaluationMode::Deterministic => {
                        eval_manifest.validate_deterministic()?;
                        deterministic::run(eval_manifest, &fs, &fs, orchestrator.as_ref()).await?
                    }
                    manifest::EvaluationMode::Statistical => {
                        statistical::run(eval_manifest, &fs, &fs, orchestrator.as_ref()).await?
                    }
                };
                let text = serde_json::to_string_pretty(&result)?;
                if !result.passed {
                    emit(&text, out.as_deref(), write_baseline.as_deref())?;
                    std::process::exit(1);
                }
                text
            };

            emit(&json, out.as_deref(), write_baseline.as_deref())?;
            println!("{json}");
        }
        Commands::Compare {
            baseline,
            candidate,
        } => {
            compare::run(baseline, candidate).await?;
        }
        Commands::Integrity { corpus, write } => {
            let root = corpus::resolve_corpus_root(corpus.as_deref())?;
            if write {
                let map = corpus::write_digests(&root)?;
                println!("{}", serde_json::to_string_pretty(&map)?);
            }
            corpus::check_integrity(&root)?;
            println!("INTEGRITY SUCCESS");
        }
    }

    Ok(())
}

fn parse_mode(mode: Option<&str>) -> anyhow::Result<Option<manifest::EvaluationMode>> {
    Ok(match mode {
        None => None,
        Some(m) => Some(match m.to_lowercase().as_str() {
            "deterministic" => manifest::EvaluationMode::Deterministic,
            "statistical" => manifest::EvaluationMode::Statistical,
            _ => anyhow::bail!("Invalid mode. Use 'deterministic' or 'statistical'"),
        }),
    })
}

fn load_one(
    corpus_root: &std::path::Path,
    manifest: Option<&std::path::Path>,
    corpus_id: Option<&str>,
) -> anyhow::Result<manifest::EvaluationManifest> {
    if let Some(id) = corpus_id {
        return corpus::load_manifest(corpus_root, id);
    }
    let Some(path) = manifest else {
        anyhow::bail!("pass --manifest, --corpus-id, or --subset");
    };
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn emit(
    json: &str,
    out: Option<&std::path::Path>,
    write_baseline: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
    }
    if let Some(path) = write_baseline {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        eprintln!("wrote baseline {}", path.display());
    }
    Ok(())
}
