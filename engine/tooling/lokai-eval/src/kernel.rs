//! Production `submit_chat_turn` path with recorded (or live) inference.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use lokai_app::commands::{
    CancelRunCommand, InitializeCommand, RunTurnCommand, StartSessionCommand,
};
use lokai_app::events::{ApplicationEvent, ApplicationEventSink};
use lokai_app::redaction_audit::StoreRedactionSink;
use lokai_app::Application;
use lokai_domain::secrets::{OutboundRedactionSink, SecretScanner};
use lokai_inference::InferenceProvider;
use lokai_memory::SharedStore;
use lokai_secrets::scanner::ScannerEngine;

use crate::corpus::snapshot_files;
use crate::manifest::EvaluationManifest;
use crate::providers::{
    script_for_scenario, CaptureProvider, ScanProvider, ScriptTurn, ScriptedProvider,
    TokenBudgetProvider,
};
use crate::result::{ResourceUsage, RunStatus, TimingBreakdown};
use crate::traits::{AgentExecutionResult, AgentOrchestrator};

struct SilentSink;
impl ApplicationEventSink for SilentSink {
    fn send(&self, _event: ApplicationEvent) {}
}

#[derive(Clone, Copy, Debug)]
pub struct KernelOptions {
    /// When true, wrap the model with H1-1 `redact_outbound` (production function).
    pub scan: bool,
    /// R07: set cancel before `run_turn` (simulates interrupt / cancel).
    pub force_cancel: bool,
}

impl Default for KernelOptions {
    fn default() -> Self {
        Self {
            scan: true,
            force_cancel: false,
        }
    }
}

pub struct KernelOrchestrator {
    pub options: KernelOptions,
    pub script_override: Option<Vec<ScriptTurn>>,
}

impl KernelOrchestrator {
    pub fn recorded() -> Self {
        Self {
            options: KernelOptions::default(),
            script_override: None,
        }
    }

    pub fn without_scan() -> Self {
        Self {
            options: KernelOptions {
                scan: false,
                force_cancel: false,
            },
            script_override: None,
        }
    }

    pub fn force_cancel() -> Self {
        Self {
            options: KernelOptions {
                scan: true,
                force_cancel: true,
            },
            script_override: None,
        }
    }
}

#[async_trait(?Send)]
impl AgentOrchestrator for KernelOrchestrator {
    async fn run_task(
        &self,
        workspace: &Path,
        manifest: &EvaluationManifest,
    ) -> Result<AgentExecutionResult> {
        let scripts = match &self.script_override {
            Some(s) => s.clone(),
            None => script_for_scenario(&manifest.scenario_id)?,
        };
        run_kernel_turn(workspace, manifest, self.options, scripts).await
    }
}

async fn run_kernel_turn(
    workspace: &Path,
    manifest: &EvaluationManifest,
    options: KernelOptions,
    scripts: Vec<ScriptTurn>,
) -> Result<AgentExecutionResult> {
    let started = std::time::Instant::now();
    let before = snapshot_files(workspace)?;

    let db_path = workspace.join(".lokai").join("eval.db");
    std::fs::create_dir_all(db_path.parent().unwrap())?;
    let store: SharedStore =
        SharedStore::open(&db_path, 1).map_err(|e| anyhow::anyhow!("open eval store: {e}"))?;
    let sink: Arc<dyn ApplicationEventSink> = Arc::new(SilentSink);
    let (app, _init, bootstrap) = Application::bootstrap(
        InitializeCommand {
            workspace_root: workspace.display().to_string(),
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        },
        Some(store.clone()),
        sink,
        None,
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("bootstrap: {e}"))?;
    let app = Arc::new(app);

    let started_session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: bootstrap.workspace_root.clone(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: Some(manifest.task_prompt.clone()),
            data_class: None,
            verify_cmd: None,
            briefing: Some(true),
            orchestration: Some("single".into()),
            critic: Some(false),
            llm_router: Some(false),
            model_fast: Some(manifest.model_name.clone()),
            model_hard: Some(manifest.model_name.clone()),
            session_max_steps: Some(manifest.limits.max_turns.max(1) as usize),
            auto_grant_approvals: Some(true),
            ..Default::default()
        })
        .await
        .map_err(|e| anyhow::anyhow!("start_session: {e}"))?;

    let scripted: Arc<dyn InferenceProvider> = Arc::new(ScriptedProvider::new(scripts));
    let (capture, seen) = CaptureProvider::wrap(scripted);
    let provider: Arc<dyn InferenceProvider> = if options.scan {
        let scanner: Arc<dyn SecretScanner> = Arc::new(ScannerEngine::default_engine());
        let redaction_sink: Arc<dyn OutboundRedactionSink> =
            Arc::new(StoreRedactionSink::new(store.clone()));
        Arc::new(ScanProvider::new(scanner, redaction_sink, capture))
    } else {
        capture
    };

    let token_budget = manifest.limits.max_tokens.max(1);
    let wall =
        std::time::Duration::from_secs(manifest.limits.max_wall_clock_duration_secs.max(1) as u64);
    if manifest.limits.max_attempts == 0 {
        return Ok(AgentExecutionResult {
            patch_digest: None,
            output_digest: None,
            resource_usage: ResourceUsage {
                tool_call_count: 0,
                model_call_count: 0,
                token_usage_prompt: 0,
                token_usage_completion: 0,
            },
            timing: TimingBreakdown {
                wall_clock_duration_ms: started.elapsed().as_millis() as u64,
                model_inference_duration_ms: 0,
                tool_execution_duration_ms: 0,
            },
            touched_files: Vec::new(),
            outbound_texts: Vec::new(),
            found_secrets: false,
            run_status: RunStatus::Incomplete,
            failure_classification: Some("attempt_limit".into()),
        });
    }

    let live = app
        .sessions
        .live(&started_session.session_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let tokens_used = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let token_limit_exceeded = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let provider = TokenBudgetProvider::wrap(
        provider,
        token_budget,
        token_limit_exceeded.clone(),
        tokens_used.clone(),
        live.cancel.clone(),
    );
    app.bind_inference(provider, None);
    if options.force_cancel {
        app.sessions
            .cancel_session(CancelRunCommand {
                session_id: started_session.session_id.clone(),
                pooled_cancel: false,
            })
            .await
            .map_err(|e| anyhow::anyhow!("force cancel: {e}"))?;
    }

    let join = app.arm_turn_join(&started_session.session_id);
    app.submit_chat_turn(RunTurnCommand {
        session_id: started_session.session_id.clone(),
        user_input: manifest.task_prompt.clone(),
        verify_cmd: None,
        llm_router: Some(false),
    })
    .map_err(|e| anyhow::anyhow!("submit_chat_turn: {e}"))?;

    let mut join = join;
    let sleep = tokio::time::sleep(wall);
    tokio::pin!(sleep);
    let mut interrupt: Option<String> = None;
    let mut timed_out = false;
    loop {
        tokio::select! {
            biased;
            finish = &mut join => {
                let finish = finish.unwrap_or(lokai_app::TurnFinish {
                    ok: false,
                    canceled: true,
                    error: Some("join dropped".into()),
                });
                if interrupt.is_none() {
                    if options.force_cancel {
                        interrupt = Some("canceled".into());
                    } else if token_limit_exceeded.load(std::sync::atomic::Ordering::Relaxed) {
                        interrupt = Some("token_limit".into());
                    } else if let Some(err) = finish.error.as_deref() {
                        if err.contains("token budget") {
                            interrupt = Some("token_limit".into());
                        } else if err.contains("canceled") || options.force_cancel || finish.canceled
                        {
                            interrupt = Some("canceled".into());
                        } else if !finish.ok {
                            return Err(anyhow::anyhow!("run_turn: {err}"));
                        }
                    } else if finish.canceled || options.force_cancel {
                        interrupt = Some("canceled".into());
                    }
                }
                break;
            }
            _ = &mut sleep, if !timed_out => {
                timed_out = true;
                interrupt = Some("wall_clock_limit".into());
                let _ = app
                    .sessions
                    .cancel_session(CancelRunCommand {
                        session_id: started_session.session_id.clone(),
                        pooled_cancel: false,
                    })
                    .await;
            }
        }
    }

    let after = snapshot_files(workspace)?;
    let touched = diff_paths(&before, &after);
    let patch_digest = Some(crate::graders::patch_digest(&after, &touched)?);
    let outbound = seen.lock().unwrap().clone();
    let found_secrets = crate::graders::outbound_contains_secrets(&outbound);
    let prompt_tokens = tokens_used.load(std::sync::atomic::Ordering::Relaxed);

    let (run_status, failure_classification) = match &interrupt {
        Some(reason) => (RunStatus::Incomplete, Some(reason.clone())),
        None => (RunStatus::Completed, None),
    };

    Ok(AgentExecutionResult {
        patch_digest: patch_digest.clone(),
        output_digest: patch_digest,
        resource_usage: ResourceUsage {
            tool_call_count: 0,
            model_call_count: outbound.len() as u32,
            token_usage_prompt: prompt_tokens,
            token_usage_completion: 0,
        },
        timing: TimingBreakdown {
            wall_clock_duration_ms: started.elapsed().as_millis() as u64,
            model_inference_duration_ms: 0,
            tool_execution_duration_ms: 0,
        },
        touched_files: touched,
        outbound_texts: outbound,
        found_secrets,
        run_status,
        failure_classification,
    })
}

fn diff_paths(
    before: &std::collections::BTreeMap<String, Vec<u8>>,
    after: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (path, bytes) in after {
        if before.get(path) != Some(bytes) {
            out.push(path.clone());
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            out.push(path.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{resolve_corpus_root, FilesystemCorpus};
    use crate::traits::CorpusProvider;
    use std::path::PathBuf;

    fn block_local<F>(f: F) -> F::Output
    where
        F: std::future::Future,
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(f))
    }

    async fn mount(id: &str) -> (FilesystemCorpus, PathBuf) {
        let root = resolve_corpus_root(None).expect("corpus");
        let corpus = FilesystemCorpus::new(root);
        let ws = corpus.mount_snapshot(id).await.expect("mount");
        (corpus, ws)
    }

    #[test]
    fn wrong_edit_fails_file_boundary() {
        block_local(async {
            let (_c, ws) = mount("snap-01").await;
            let mut manifest = crate::corpus::load_manifest(
                &crate::corpus::resolve_corpus_root(None).unwrap(),
                "01-small-single-lang",
            )
            .unwrap();
            manifest.scenario_id = "01-wrong-edit".into();
            let orch = KernelOrchestrator {
                options: KernelOptions::default(),
                script_override: None,
            };
            let exec = orch.run_task(&ws, &manifest).await.expect("turn");
            let ctx = crate::graders::GraderContext {
                output_digest: exec.output_digest.clone(),
                touched_files: exec.touched_files.clone(),
                found_secrets: exec.found_secrets,
                workspace: ws.clone(),
            };
            let passed = crate::graders::evaluate_graders(&manifest.graders, &ctx)
                .await
                .unwrap();
            assert!(!passed, "writing src/main.rs must fail README FileBoundary");
        });
    }

    #[test]
    fn sensitive_fails_without_h11_scan_and_passes_with_it() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "07-synthetic-sensitive").unwrap();

            let (_c, ws_on) = mount("snap-07").await;
            let on = KernelOrchestrator::recorded()
                .run_task(&ws_on, &manifest)
                .await
                .expect("scanned turn");
            let ctx_on = crate::graders::GraderContext {
                output_digest: on.output_digest.clone(),
                touched_files: on.touched_files.clone(),
                found_secrets: on.found_secrets,
                workspace: ws_on,
            };
            assert!(
                crate::graders::evaluate_graders(&manifest.graders, &ctx_on)
                    .await
                    .unwrap(),
                "H1-1 scan must redact the synthetic key before the model"
            );

            let (_c2, ws_off) = mount("snap-07").await;
            let off = KernelOrchestrator::without_scan()
                .run_task(&ws_off, &manifest)
                .await
                .expect("unscanned turn");
            let ctx_off = crate::graders::GraderContext {
                output_digest: off.output_digest.clone(),
                touched_files: off.touched_files.clone(),
                found_secrets: off.found_secrets,
                workspace: ws_off,
            };
            assert!(
                !crate::graders::evaluate_graders(&manifest.graders, &ctx_off)
                    .await
                    .unwrap(),
                "disabling H1-1 redaction must fail the security grader"
            );
        });
    }

    #[test]
    fn same_commit_same_verdict() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "01-small-single-lang").unwrap();
            let orch = KernelOrchestrator::recorded();
            let mut verdicts = Vec::new();
            let mut last_touched = Vec::new();
            for _ in 0..2 {
                let (_c, ws) = mount("snap-01").await;
                let exec = orch.run_task(&ws, &manifest).await.expect("turn");
                last_touched = exec.touched_files.clone();
                let ctx = crate::graders::GraderContext {
                    output_digest: exec.output_digest.clone(),
                    touched_files: exec.touched_files.clone(),
                    found_secrets: exec.found_secrets,
                    workspace: ws,
                };
                verdicts.push(
                    crate::graders::evaluate_graders(&manifest.graders, &ctx)
                        .await
                        .unwrap(),
                );
            }
            assert_eq!(verdicts[0], verdicts[1]);
            assert!(
                verdicts[0],
                "recorded 01 fixture must pass FileBoundary, touched={last_touched:?}"
            );
        });
    }

    #[test]
    fn prompt_injection_fails_without_h11_scan() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "12-prompt-injection").unwrap();

            let (_c, ws_on) = mount("snap-12").await;
            let on = KernelOrchestrator::recorded()
                .run_task(&ws_on, &manifest)
                .await
                .expect("scanned 12");
            let ctx_on = crate::graders::GraderContext {
                output_digest: on.output_digest.clone(),
                touched_files: on.touched_files.clone(),
                found_secrets: on.found_secrets,
                workspace: ws_on,
            };
            assert!(
                crate::graders::evaluate_graders(&manifest.graders, &ctx_on)
                    .await
                    .unwrap(),
                "12 must pass with H1-1 scan and without writing secrets.yml"
            );

            let (_c2, ws_off) = mount("snap-12").await;
            let off = KernelOrchestrator::without_scan()
                .run_task(&ws_off, &manifest)
                .await
                .expect("unscanned 12");
            let ctx_off = crate::graders::GraderContext {
                output_digest: off.output_digest.clone(),
                touched_files: off.touched_files.clone(),
                found_secrets: off.found_secrets,
                workspace: ws_off,
            };
            assert!(
                !crate::graders::evaluate_graders(&manifest.graders, &ctx_off)
                    .await
                    .unwrap(),
                "12-prompt-injection must fail when H1-1 redaction is disabled"
            );
        });
    }

    #[test]
    fn recorded_batch_c_passes_independent_grading() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let orch = KernelOrchestrator::recorded();
            for (id, snap) in [
                ("09-test-creation", "snap-09"),
                ("10-misleading-implementation", "snap-10"),
                ("11-no-op-correct", "snap-11"),
            ] {
                let manifest = crate::corpus::load_manifest(&root, id).unwrap();
                let (_c, ws) = mount(snap).await;
                let exec = orch.run_task(&ws, &manifest).await.expect(id);
                assert_eq!(
                    exec.run_status,
                    RunStatus::Completed,
                    "{id} incomplete: {:?}",
                    exec.failure_classification
                );
                let ctx = crate::graders::GraderContext {
                    output_digest: exec.output_digest.clone(),
                    touched_files: exec.touched_files.clone(),
                    found_secrets: exec.found_secrets,
                    workspace: ws,
                };
                let failure = crate::graders::grade(&manifest.graders, &ctx)
                    .await
                    .unwrap();
                assert_eq!(failure, None, "{id}");
            }
        });
    }

    #[test]
    fn recorded_batch_b_requires_protected_grading() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let orch = KernelOrchestrator::recorded();
            for (id, snap) in [
                ("05-dirty-tree", "snap-05"),
                ("06-cross-file-refactor", "snap-06"),
                ("08-symbol-discovery", "snap-08"),
            ] {
                let manifest = crate::corpus::load_manifest(&root, id).unwrap();
                let (_c, ws) = mount(snap).await;
                let exec = orch.run_task(&ws, &manifest).await.expect(id);
                assert_eq!(
                    exec.run_status,
                    RunStatus::Completed,
                    "{id} incomplete: {:?}",
                    exec.failure_classification
                );
                let ctx = crate::graders::GraderContext {
                    output_digest: exec.output_digest.clone(),
                    touched_files: exec.touched_files.clone(),
                    found_secrets: exec.found_secrets,
                    workspace: ws,
                };
                let failure = crate::graders::grade(&manifest.graders, &ctx)
                    .await
                    .unwrap();
                assert_eq!(failure, None, "{id}");
            }
        });
    }

    #[test]
    fn recorded_batch_a_requires_independent_grading() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let orch = KernelOrchestrator::recorded();
            for (id, snap) in [
                ("02-large-monorepo", "snap-02"),
                ("03-multi-language", "snap-03"),
                ("04-failing-tests", "snap-04"),
            ] {
                let manifest = crate::corpus::load_manifest(&root, id).unwrap();
                let (_c, ws) = mount(snap).await;
                let exec = orch.run_task(&ws, &manifest).await.expect(id);
                assert_eq!(
                    exec.run_status,
                    RunStatus::Completed,
                    "{id} incomplete: {:?}",
                    exec.failure_classification
                );
                let ctx = crate::graders::GraderContext {
                    output_digest: exec.output_digest.clone(),
                    touched_files: exec.touched_files.clone(),
                    found_secrets: exec.found_secrets,
                    workspace: ws,
                };
                let failure = crate::graders::grade(&manifest.graders, &ctx)
                    .await
                    .unwrap();
                assert_eq!(failure, None, "{id}: {:?}", exec.touched_files);
            }
        });
    }

    #[test]
    fn parser_fixture_rejects_broken_implementation_and_replaced_test() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "03-multi-language").unwrap();
            let (_corpus, workspace) = mount("snap-03").await;
            let context = crate::graders::GraderContext {
                output_digest: None,
                touched_files: vec![],
                found_secrets: false,
                workspace: workspace.clone(),
            };
            assert_eq!(
                crate::graders::grade(&manifest.graders, &context)
                    .await
                    .unwrap(),
                Some("grader_command_failed")
            );
            std::fs::write(
                workspace.join("backend_tests.py"),
                "print('{\"lokai_test_report\":1,\"passed\":4,\"failed\":0,\"skipped\":0}')\n",
            )
            .unwrap();
            assert_eq!(
                crate::graders::grade(&manifest.graders, &context)
                    .await
                    .unwrap(),
                Some("grader_input_modified")
            );
        });
    }

    #[test]
    fn independent_rust_graders_reject_bugs_and_test_replacement() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            for (id, snapshot) in [
                ("04-failing-tests", "snap-04"),
                ("06-cross-file-refactor", "snap-06"),
                ("10-misleading-implementation", "snap-10"),
            ] {
                let manifest = crate::corpus::load_manifest(&root, id).unwrap();
                let (_corpus, workspace) = mount(snapshot).await;
                let context = crate::graders::GraderContext {
                    output_digest: None,
                    touched_files: vec![],
                    found_secrets: false,
                    workspace: workspace.clone(),
                };
                assert_eq!(
                    crate::graders::grade(&manifest.graders, &context)
                        .await
                        .unwrap(),
                    Some("grader_command_failed"),
                    "{id}"
                );
                std::fs::write(
                    workspace.join("tests/acceptance.rs"),
                    "#[test] fn always_passes() {}\n",
                )
                .unwrap();
                assert_eq!(
                    crate::graders::grade(&manifest.graders, &context)
                        .await
                        .unwrap(),
                    Some("grader_input_modified"),
                    "{id}"
                );
            }
        });
    }

    #[test]
    fn workspace_fixture_rejects_broken_logging_and_modified_grader() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "02-large-monorepo").unwrap();
            let (_corpus, workspace) = mount("snap-02").await;
            let context = crate::graders::GraderContext {
                output_digest: None,
                touched_files: vec![],
                found_secrets: false,
                workspace: workspace.clone(),
            };
            assert_eq!(
                crate::graders::grade(&manifest.graders, &context)
                    .await
                    .unwrap(),
                Some("grader_command_failed")
            );
            std::fs::write(
                workspace.join("core/tests/acceptance.rs"),
                "#[test] fn always_passes() {}\n",
            )
            .unwrap();
            assert_eq!(
                crate::graders::grade(&manifest.graders, &context)
                    .await
                    .unwrap(),
                Some("grader_input_modified")
            );
        });
    }

    #[test]
    fn mutation_grading_rejects_vacuous_broken_and_self_modifying_tests() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "09-test-creation").unwrap();
            for (test, expected) in [
                ("#[test] fn empty() {}", "mutation_survived"),
                ("#[test] fn ordinary_division() { assert_eq!(math::divide_numbers(4, 2), 2); }", "mutation_survived"),
                ("#[test] #[should_panic] fn unconditional() { panic!(\"always\"); }", "mutation_survived"),
                ("#[test] fn broken() { assert!(false); }", "grader_command_failed"),
                ("this is not Rust", "grader_command_failed"),
                ("#[test] fn changes_self() { std::fs::write(\"tests/divide_by_zero.rs\", \"changed\").unwrap(); }", "grader_input_modified"),
            ] {
                let (_corpus, workspace) = mount("snap-09").await;
                std::fs::create_dir(workspace.join("tests")).unwrap();
                std::fs::write(workspace.join("tests/divide_by_zero.rs"), test).unwrap();
                let original = std::fs::read(workspace.join("src/lib.rs")).unwrap();
                let context = crate::graders::GraderContext { output_digest: None, touched_files: vec!["tests/divide_by_zero.rs".into()], found_secrets: false, workspace: workspace.clone() };
                assert_eq!(crate::graders::grade(&manifest.graders, &context).await.unwrap(), Some(expected), "{test}");
                assert_eq!(std::fs::read(workspace.join("src/lib.rs")).unwrap(), original);
                assert_eq!(std::fs::read_to_string(workspace.join("tests/divide_by_zero.rs")).unwrap(), test);
                assert!(!workspace.join("target").exists(), "trials must build in private snapshots");
            }
        });
    }

    #[test]
    fn following_injection_fails_file_boundary() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let mut manifest = crate::corpus::load_manifest(&root, "12-prompt-injection").unwrap();
            manifest.scenario_id = "12-injection-follows".into();
            let (_c, ws) = mount("snap-12").await;
            let orch = KernelOrchestrator::recorded();
            let exec = orch.run_task(&ws, &manifest).await.expect("turn");
            let ctx = crate::graders::GraderContext {
                output_digest: exec.output_digest.clone(),
                touched_files: exec.touched_files.clone(),
                found_secrets: exec.found_secrets,
                workspace: ws,
            };
            assert!(
                !crate::graders::evaluate_graders(&manifest.graders, &ctx)
                    .await
                    .unwrap(),
                "writing secrets.yml must fail the injection FileBoundary"
            );
        });
    }

    #[test]
    fn force_cancel_marks_incomplete_not_pass() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let manifest = crate::corpus::load_manifest(&root, "01-small-single-lang").unwrap();
            let (_c, ws) = mount("snap-01").await;
            let exec = KernelOrchestrator::force_cancel()
                .run_task(&ws, &manifest)
                .await
                .expect("canceled turn");
            assert_eq!(exec.run_status, RunStatus::Incomplete);
            assert_eq!(exec.failure_classification.as_deref(), Some("canceled"));
            let result = crate::honesty::normalize_pass(crate::result::EvaluationResult {
                scenario_id: manifest.scenario_id.clone(),
                run_status: exec.run_status,
                timing: exec.timing,
                resource_usage: exec.resource_usage,
                patch_digest: exec.patch_digest,
                output_digest: exec.output_digest,
                trace_correlation_id: "t".into(),
                statistics: None,
                passed: true,
                failure_classification: exec.failure_classification,
            });
            assert!(!result.passed);
            assert!(!crate::honesty::scenario_counts_as_pass(&result));
        });
    }

    #[test]
    fn token_limit_marks_incomplete() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let mut manifest = crate::corpus::load_manifest(&root, "01-small-single-lang").unwrap();
            manifest.limits.max_tokens = 1;
            let (_c, ws) = mount("snap-01").await;
            let exec = KernelOrchestrator::recorded()
                .run_task(&ws, &manifest)
                .await
                .expect("token-limited turn");
            assert_eq!(exec.run_status, RunStatus::Incomplete);
            assert_eq!(exec.failure_classification.as_deref(), Some("token_limit"));
            assert!(!crate::honesty::scenario_counts_as_pass(
                &crate::honesty::normalize_pass(crate::result::EvaluationResult {
                    scenario_id: "01".into(),
                    run_status: exec.run_status,
                    timing: exec.timing,
                    resource_usage: exec.resource_usage,
                    patch_digest: None,
                    output_digest: None,
                    trace_correlation_id: "t".into(),
                    statistics: None,
                    passed: true,
                    failure_classification: exec.failure_classification,
                })
            ));
        });
    }

    #[test]
    fn attempt_limit_zero_is_incomplete() {
        block_local(async {
            let root = resolve_corpus_root(None).unwrap();
            let mut manifest = crate::corpus::load_manifest(&root, "01-small-single-lang").unwrap();
            manifest.limits.max_attempts = 0;
            let (_c, ws) = mount("snap-01").await;
            let exec = KernelOrchestrator::recorded()
                .run_task(&ws, &manifest)
                .await
                .expect("attempt limit");
            assert_eq!(exec.run_status, RunStatus::Incomplete);
            assert_eq!(
                exec.failure_classification.as_deref(),
                Some("attempt_limit")
            );
        });
    }

    #[test]
    fn wall_clock_timeout_in_deterministic_is_incomplete() {
        block_local(async {
            use crate::traits::AgentOrchestrator;
            struct SlowOrch;
            #[async_trait::async_trait(?Send)]
            impl AgentOrchestrator for SlowOrch {
                async fn run_task(
                    &self,
                    _workspace: &std::path::Path,
                    _manifest: &crate::manifest::EvaluationManifest,
                ) -> anyhow::Result<crate::traits::AgentExecutionResult> {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    Ok(crate::traits::AgentExecutionResult {
                        patch_digest: None,
                        output_digest: None,
                        resource_usage: ResourceUsage {
                            tool_call_count: 0,
                            model_call_count: 0,
                            token_usage_prompt: 0,
                            token_usage_completion: 0,
                        },
                        timing: TimingBreakdown {
                            wall_clock_duration_ms: 3000,
                            model_inference_duration_ms: 0,
                            tool_execution_duration_ms: 0,
                        },
                        touched_files: vec![],
                        outbound_texts: vec![],
                        found_secrets: false,
                        run_status: RunStatus::Completed,
                        failure_classification: None,
                    })
                }
            }
            let root = resolve_corpus_root(None).unwrap();
            let mut manifest = crate::corpus::load_manifest(&root, "01-small-single-lang").unwrap();
            manifest.limits.max_wall_clock_duration_secs = 1;
            struct NoopSandbox;
            #[async_trait::async_trait]
            impl crate::traits::SandboxProvider for NoopSandbox {
                async fn setup_sandbox(
                    &self,
                    _workspace: &std::path::Path,
                    _manifest: &crate::manifest::EvaluationManifest,
                ) -> anyhow::Result<()> {
                    Ok(())
                }
            }
            let corpus = FilesystemCorpus::new(root);
            let result = crate::deterministic::run(manifest, &corpus, &NoopSandbox, &SlowOrch)
                .await
                .expect("timeout path");
            assert_eq!(result.run_status, RunStatus::Incomplete);
            assert!(!result.passed);
            assert_eq!(
                result.failure_classification.as_deref(),
                Some("wall_clock_limit")
            );
        });
    }
}
