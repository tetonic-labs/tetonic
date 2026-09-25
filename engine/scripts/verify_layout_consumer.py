#!/usr/bin/env python3
"""
LAYOUT-CONSUMER-01 Proof Runner (V4 Architecture Layout Verification)

This script validates that the core runtime and manager (lokai-run, lokai-runtime,
lokai-core, lokai-sandbox) can be consumed by an isolated non-coding consumer
with ZERO coding crates (no lokai-app, lokai-orchestrator, lokai-tools,
lokai-transaction, lokai-context, lokai-index, lokai-lsp) in the resolved build closure.

Execution:
    python engine/scripts/verify_layout_consumer.py
"""

import os
import sys
import shutil
import tempfile
import subprocess
import tomllib
import re
from pathlib import Path

FORBIDDEN_CRATES = {
    "lokai-app",
    "lokai-orchestrator",
    "lokai-tools",
    "lokai-transaction",
    "lokai-context",
    "lokai-index",
    "lokai-lsp",
    "tetonic-cli",
    "tetonicd",
    "lokai-arch-gate",
    "lokai-eval",
    "lokai-bench",
    "tetonic-arch-gate",
    "tetonic-eval",
    "tetonic-bench",
}

ALLOWED_PACKAGES = {
    "lokai-domain": "core/lokai-domain",
    "lokai-core": "core/lokai-core",
    "lokai-runtime": "core/lokai-runtime",
    "lokai-run": "mantle/lokai-run",
    "lokai-inference": "atmos/lokai-inference",
    "lokai-sandbox": "core/lokai-sandbox",
    "lokai-secrets": "core/lokai-secrets",
    "lokai-memory": "strata/lokai-memory",
    "lokai-artifact": "strata/lokai-artifact",
    "lokai-policy": "core/lokai-policy",
    "lokai-telemetry": "core/lokai-telemetry",
    "lokai-egress": "atmos/lokai-egress",
    "lokai-fabric-protocol": "atmos/lokai-fabric-protocol",
}

CONSUMER_CARGO_TOML = """[package]
name = "layout-consumer"
version = "0.1.0"
edition = "2021"

[dependencies]
lokai-domain = { path = "../core/lokai-domain" }
lokai-core = { path = "../core/lokai-core" }
lokai-runtime = { path = "../core/lokai-runtime" }
lokai-run = { path = "../mantle/lokai-run" }
lokai-inference = { path = "../atmos/lokai-inference" }
lokai-sandbox = { path = "../core/lokai-sandbox" }
lokai-secrets = { path = "../core/lokai-secrets" }
lokai-memory = { path = "../strata/lokai-memory" }
lokai-artifact = { path = "../strata/lokai-artifact" }
lokai-policy = { path = "../core/lokai-policy" }
tokio = { workspace = true, features = ["full"] }
serde_json = { workspace = true }
async-trait = { workspace = true }
tracing = { workspace = true }
"""

CONSUMER_TEST_RS = r"""//! LAYOUT-CONSUMER-01 managed process test fixture.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use lokai_domain::execution::{
    ActionKind, AuthorizedAction, ExecutionOutcome,
};
use lokai_domain::ids::{AttemptId, IdentityId};
use lokai_domain::tool_host::{ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal};
use lokai_domain::work_scope::CancellationSignal;
use lokai_domain::{
    AgentIdentity, AgentInvocation, AgentJobSpec, CandidateOutcome, RunState,
};

use lokai_core::{
    Agent, AgentConfig, AuditSink,
};
use lokai_inference::{
    ChatRequest, ChatResponse, FunctionCall, GenUsage, InferenceError,
    InferenceProvider, InferenceProvenance, Message, TokenSink, ToolCall,
};
use lokai_runtime::{
    AgentAssemblyParts, AssemblyMode, EngineRuntime, ProductionApproval,
};
use lokai_run::managed::{
    ManagedBinding, ManagedRunHooks, ManagedRunService, StartIdentityJobCommand,
    StartIdentityJobResult,
};
use lokai_run::DurableRunSupervisor;
use lokai_sandbox::{
    EnforcementLevel, NonCodingProcessValidator, ProcessExecutor, ProcessResourceValidator,
};

/// Narrow consumer tool host advertising only run_shell backed by ProcessExecutor.
struct ConsumerToolHost {
    executor: Arc<ProcessExecutor>,
    tool_calls_observed: Arc<AtomicUsize>,
    successful_output: Arc<Mutex<Option<String>>>,
}

impl ToolHost for ConsumerToolHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(ConsumerToolHost {
            executor: self.executor.clone(),
            tool_calls_observed: self.tool_calls_observed.clone(),
            successful_output: self.successful_output.clone(),
        })
    }

    fn propose(&self, name: &str, _args: &Value) -> Option<ToolProposal> {
        if name == "run_shell" {
            Some(ToolProposal {
                kind: ActionKind::ExecuteShell,
                resolved_path: None,
            })
        } else {
            None
        }
    }

    fn is_tool_allowed(&self, name: &str) -> bool {
        name == "run_shell" || name == "finish"
    }

    fn is_read_only(&self, _name: &str) -> bool {
        false
    }

    fn requires_action_broker(&self, name: &str) -> bool {
        name == "run_shell"
    }

    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        vec![ToolAdvertisement {
            name: "run_shell".into(),
            description: "Run a shell command in working directory".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        }]
    }

    fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String> {
        if name == "run_shell" {
            if args.get("command").and_then(|v| v.as_str()).is_some() {
                Ok(())
            } else {
                Err("missing command argument".into())
            }
        } else if name == "finish" {
            Ok(())
        } else {
            Err(format!("unknown tool {name}"))
        }
    }

    fn execute_authorized(
        &self,
        name: &str,
        _args: &Value,
        auth: Option<&AuthorizedAction>,
        _cancel: &CancellationSignal,
    ) -> ToolOutcome {
        if name != "run_shell" {
            return ToolOutcome::fail("unknown tool", "unknown_tool");
        }
        let Some(authorized) = auth else {
            return ToolOutcome::fail("missing authorization", "denied");
        };

        self.tool_calls_observed.fetch_add(1, Ordering::SeqCst);
        let outcome = self.executor.run_process_sync(authorized);
        match outcome {
            ExecutionOutcome::Completed { ok, summary, .. } => {
                if ok {
                    *self.successful_output.lock().unwrap() = Some(summary.clone());
                    ToolOutcome::ok(summary.clone(), summary)
                } else {
                    ToolOutcome::fail(summary, "shell_error")
                }
            }
            ExecutionOutcome::Failed { reason, .. } => {
                ToolOutcome::fail(reason, "execution_failed")
            }
            other => ToolOutcome::fail(format!("{other:?}"), "unexpected_outcome"),
        }
    }
}

/// Deterministic 2-turn inference provider:
/// Turn 1: tool call to run_shell with echo marker
/// Turn 2: tool call to finish with completion summary
struct DeterministicProvider {
    turn: Arc<AtomicUsize>,
    marker: String,
}

#[async_trait]
impl InferenceProvider for DeterministicProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let t = self.turn.fetch_add(1, Ordering::SeqCst);
        let message = if t == 0 {
            Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "run_shell".into(),
                    arguments: json!({
                        "command": format!("echo {}", self.marker)
                    }),
                },
            }])
        } else {
            Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: json!({
                        "summary": format!("Completed marker task with {}", self.marker)
                    }),
                },
            }])
        };
        Ok(ChatResponse {
            message,
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}

/// Observed lifecycle hook tracker.
struct HookTracker {
    broker: Arc<lokai_runtime::RuntimeActionBroker>,
    starts: AtomicUsize,
    steps: AtomicUsize,
    terminals: AtomicUsize,
}

impl ManagedRunHooks for HookTracker {
    fn started(&self, binding: &ManagedBinding) {
        self.broker.register_attempt_approval(
            binding.attempt_id.clone(),
            Arc::new(|_| Box::pin(async { true })),
        );
        self.starts.fetch_add(1, Ordering::SeqCst);
    }
    fn step(&self, _binding: &ManagedBinding, _step: &lokai_core::Step) {
        self.steps.fetch_add(1, Ordering::SeqCst);
    }
    fn terminal(&self, result: &StartIdentityJobResult) {
        self.broker.unregister_attempt_approval(&result.attempt_id);
        self.terminals.fetch_add(1, Ordering::SeqCst);
    }
    fn fail_approval_waits(&self, attempt: &AttemptId) {
        self.broker.unregister_attempt_approval(attempt);
    }
}

#[derive(Default)]
struct EphemeralAudit;
impl AuditSink for EphemeralAudit {
    fn message(&self, _: &str, _: &str, _: Option<&str>) {}
    fn tool_call(&self, _: &str, _: &str, _: &str, _: bool, _: &str, _: Option<&str>) {}
    fn file_change(&self, _: &str, _: &str, _: &str, _: Option<&str>, _: Option<&str>) {}
    fn note(&self, _: &str) {}
}

#[tokio::test]
async fn test_layout_consumer_managed_process() {
    let temp_dir = std::env::temp_dir().join(format!("layout-consumer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let work_dir = temp_dir.join("non_repo_workdir");
    std::fs::create_dir_all(&work_dir).unwrap();

    let store_path = temp_dir.join("store.db");
    let store = Arc::new(lokai_memory::SharedStore::open(&store_path, 1).unwrap());
    let artifact_dir = temp_dir.join("artifacts");
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            &artifact_dir,
            lokai_artifact::ScanPolicy::Scan(Arc::new(|_text| false)),
        )
        .unwrap(),
    );

    let validator: Arc<dyn ProcessResourceValidator> = Arc::new(NonCodingProcessValidator);
    let executor = Arc::new(ProcessExecutor::new(
        &work_dir,
        EnforcementLevel::Constrained,
        validator,
    ));

    let tool_calls_count = Arc::new(AtomicUsize::new(0));
    let successful_output = Arc::new(Mutex::new(None));
    let tool_host = ConsumerToolHost {
        executor: executor.clone(),
        tool_calls_observed: tool_calls_count.clone(),
        successful_output: successful_output.clone(),
    };

    let marker = format!("MARKER_LAYOUT_01_{}", std::process::id());
    let inference: Arc<dyn InferenceProvider> = Arc::new(DeterministicProvider {
        turn: Arc::new(AtomicUsize::new(0)),
        marker: marker.clone(),
    });

    let policy = Arc::new(lokai_policy::PolicyEngine::new(
        lokai_policy::PolicyMode::EstateStub,
    ));
    let runtime = EngineRuntime::new(policy.clone(), None, artifact_store.clone());

    let cfg = AgentConfig {
        model: "deterministic".into(),
        max_steps: 4,
        process_working_directory: Some(work_dir.clone()),
        workspace_root: None, // strictly non-coding!
        ..Default::default()
    };

    let agent = Agent::new(
        inference,
        tool_host,
        cfg,
    );

    let parts = AgentAssemblyParts {
        agent,
        audit: Box::new(EphemeralAudit),
        approval: ProductionApproval::host(Arc::new(|_| Box::pin(async { true }))),
        spawn: None,
        process_broker: None,
        context_compiler: None,
        post_edit_snapshot: Arc::new(|_| String::new()),
        resolve_under_root: Arc::new(|_, _| Err(())),
        // Fail-on-call hook ensures non-coding consumer never touches repo capture:
        capture_workspace_version: Arc::new(|_, _| {
            Err("accidental repository capture in non-coding consumer".into())
        }),
    };

    let mut assembled_agent = runtime.assemble_agent(AssemblyMode::CliEphemeral, parts).unwrap();

    let hooks = Arc::new(HookTracker {
        broker: runtime.action_broker().clone(),
        starts: AtomicUsize::new(0),
        steps: AtomicUsize::new(0),
        terminals: AtomicUsize::new(0),
    });
    let supervisor = Arc::new(DurableRunSupervisor::new(Some((*store).clone())));
    let managed_service = Arc::new(
        ManagedRunService::new(
            supervisor.clone(),
            Some((*store).clone()),
            artifact_store.clone(),
            policy.clone(),
        )
        .with_hooks(hooks.clone()),
    );

    let identity = AgentIdentity {
        id: IdentityId::new("non_coding_worker"),
        owning_application: "consumer".into(),
        bound_definition_digest: "sha256:def".into(),
        privilege_class: "default".into(),
        toolset_subscriptions: vec![],
        context_bindings: vec![],
        recovery_id: "rec_1".into(),
    };
    let job_spec = AgentJobSpec {
        identity_id: identity.id.clone(),
        definition_digest: "sha256:def".into(),
        input_digest: lokai_run::job_input_digest("Run shell command and return marker"),
        capability_bindings: vec![],
        artifact_bindings: vec![],
        recovery_id: "rec_1".into(),
    };
    let invocation = AgentInvocation {
        instructions: "You are a non-coding assistant.".into(),
        user_input: "Run shell command and return marker".into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 4,
        completion_tool: "finish".into(),
        discipline: Default::default(),
    };

    let cmd = StartIdentityJobCommand {
        identity,
        job_spec,
        invocation,
    };

    let result = managed_service
        .start_identity_job(cmd, &mut assembled_agent)
        .await
        .expect("start_identity_job must succeed");

    assert!(
        matches!(result.outcome, CandidateOutcome::Completed { .. }),
        "job must complete successfully, got: {:?}",
        result.outcome
    );
    assert_eq!(tool_calls_count.load(Ordering::SeqCst), 1, "tool call must be executed");
    let output = successful_output.lock().unwrap().clone()
        .expect("the authorized process must return a successful result");
    assert!(output.contains(&marker), "actual process output must contain marker: {output}");
    assert_eq!(hooks.starts.load(Ordering::SeqCst), 1, "hook started must be called");
    assert!(hooks.steps.load(Ordering::SeqCst) >= 2, "hook steps must be called");
    assert_eq!(hooks.terminals.load(Ordering::SeqCst), 1, "hook terminal must be called");

    // Verify durable record in store
    let snapshot = managed_service
        .inspect_run(&result.run_id)
        .await
        .expect("run must exist in durable store");
    assert_eq!(snapshot.run_id, result.run_id);
    assert_eq!(snapshot.state, RunState::Succeeded);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
"""

def sanitize_toml_table(table, dest_crates_dir):
    """Rewrite path dependencies to point to the temporary crates dir."""
    if not isinstance(table, dict):
        return table
    for k, v in list(table.items()):
        if isinstance(v, dict):
            if "path" in v:
                old_path = v["path"]
                # Determine crate name from path (e.g. "../lokai-domain" -> "lokai-domain")
                crate_name = Path(old_path).name
                if crate_name in ALLOWED_PACKAGES:
                    v["path"] = str(dest_crates_dir / crate_name).replace("\\", "/")
                else:
                    # Forbidden path dependency
                    pass
            sanitize_toml_table(v, dest_crates_dir)
    return table

def format_toml_value(val):
    if isinstance(val, bool):
        return "true" if val else "false"
    elif isinstance(val, (int, float)):
        return str(val)
    elif isinstance(val, str):
        return f'"{val}"'
    elif isinstance(val, list):
        items = [format_toml_value(x) for x in val]
        return f"[{', '.join(items)}]"
    elif isinstance(val, dict):
        items = [f"{k} = {format_toml_value(v)}" for k, v in val.items()]
        return f"{{ {', '.join(items)} }}"
    return str(val)

def clean_manifest(manifest_path: Path):
    lines = manifest_path.read_text(encoding="utf-8").splitlines()
    out = []
    skipping = False
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            # Normalize header by stripping outer brackets
            header = stripped.lstrip("[").rstrip("]").strip()
            if (
                header.endswith("dev-dependencies")
                or header == "dev-dependencies"
                or header.startswith("test")
                or header.startswith("bin")
                or header.startswith("example")
                or header.startswith("bench")
            ):
                skipping = True
            else:
                skipping = False
        if not skipping:
            out.append(line)
    manifest_path.write_text("\n".join(out) + "\n", encoding="utf-8")

def project_isolated_workspace(engine_dir: Path, temp_root: Path):
    print(f"[LAYOUT-CONSUMER-01] Creating isolated workspace in {temp_root}")

    # 1. Copy only allowed packages to their layer paths
    for pkg_name, rel_path in ALLOWED_PACKAGES.items():
        src = engine_dir / rel_path
        dst = temp_root / rel_path
        print(f"  Copying {pkg_name} from {src}")
        shutil.copytree(
            src,
            dst,
            ignore=shutil.ignore_patterns("target", "*.log", ".git"),
        )

        clean_manifest(dst / "Cargo.toml")

    # 2. Write workspace Cargo.toml from engine/Cargo.toml
    engine_cargo_path = engine_dir / "Cargo.toml"
    ws_content = engine_cargo_path.read_text(encoding="utf-8")
    new_members = (
        'members = [\n'
        '    "core/*",\n'
        '    "mantle/*",\n'
        '    "strata/*",\n'
        '    "atmos/*",\n'
        '    "consumer",\n'
        ']'
    )
    ws_content = re.sub(r'members\s*=\s*\[[^\]]*\]', new_members, ws_content)
    ws_manifest_path = temp_root / "Cargo.toml"
    ws_manifest_path.write_text(ws_content, encoding="utf-8")


    # 3. Create consumer package
    consumer_dir = temp_root / "consumer"
    consumer_tests_dir = consumer_dir / "tests"
    consumer_tests_dir.mkdir(parents=True, exist_ok=True)
    (consumer_dir / "src").mkdir(parents=True, exist_ok=True)
    (consumer_dir / "src" / "lib.rs").write_text("// consumer lib\n", encoding="utf-8")

    with open(consumer_dir / "Cargo.toml", "w", encoding="utf-8") as f:
        f.write(CONSUMER_CARGO_TOML)

    with open(consumer_tests_dir / "managed_process.rs", "w", encoding="utf-8") as f:
        f.write(CONSUMER_TEST_RS)

def verify_build_closure(temp_root: Path):
    print("[LAYOUT-CONSUMER-01] Checking resolved cargo metadata build closure...")
    res = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(temp_root / "Cargo.toml")],
        capture_output=True,
        text=True,
    )
    if res.returncode != 0:
        print(f"FAILED cargo metadata: {res.stderr}")
        sys.exit(1)

    import json
    metadata = json.loads(res.stdout)
    packages = [p["name"] for p in metadata.get("packages", [])]

    forbidden_found = [p for p in packages if p in FORBIDDEN_CRATES]
    if forbidden_found:
        print(f"FAILED: Forbidden coding crates detected in consumer build closure: {forbidden_found}")
        sys.exit(1)

    print(f"  Resolved {len(packages)} total packages. Forbidden crates check: 0 violations.")
    print("  CONFIRMED: zero lokai-app, zero lokai-orchestrator, zero lokai-tools in closure.")

def run_consumer_test(temp_root: Path):
    print("[LAYOUT-CONSUMER-01] Running consumer integration test...")
    cmd = [
        "cargo", "test", "--locked",
        "--manifest-path", str(temp_root / "Cargo.toml"),
        "-p", "layout-consumer",
        "--test", "managed_process",
        "--", "--nocapture",
    ]
    res = subprocess.run(cmd, capture_output=True, text=True)
    print(res.stdout)
    if res.returncode != 0:
        print(f"FAILED consumer test:\n{res.stderr}")
        sys.exit(1)
    print("[LAYOUT-CONSUMER-01] Consumer test passed successfully!")

def main():
    script_dir = Path(__file__).resolve().parent
    engine_dir = script_dir.parent

    temp_root = Path(tempfile.mkdtemp(prefix="lokai_layout_consumer_"))
    try:
        project_isolated_workspace(engine_dir, temp_root)
        verify_build_closure(temp_root)
        run_consumer_test(temp_root)
        print("\n[LAYOUT-CONSUMER-01] PROOF COMPLETE: SUCCESS (PASS)")
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)

if __name__ == "__main__":
    main()
