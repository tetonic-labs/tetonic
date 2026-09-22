//! Regression: observed/sessionless completion must agree with durable finalization.
use async_trait::async_trait;
use lokai_app::{
    commands::StartIdentityJobCommand,
    events::{ApplicationEvent, ApplicationEventSink},
    services::{DefaultRunService, RunService},
};
use lokai_domain::*;
use lokai_inference::*;
use lokai_run::{DurableRunSupervisor, RunSupervisor};
use std::sync::{Arc, Mutex};

struct FailFinish(DurableRunSupervisor);
#[async_trait]
impl RunSupervisor for FailFinish {
    async fn handle(&self, command: RunCommand) -> Result<RunCommandResult, RunSupervisorError> {
        if matches!(command, RunCommand::FinishRun(_)) {
            return Err(RunSupervisorError::Persistence(
                "injected terminal persistence failure".into(),
            ));
        }
        self.0.handle(command).await
    }
    async fn snapshot(&self, run: RunId) -> Result<RunSnapshot, RunSupervisorError> {
        self.0.snapshot(run).await
    }
    async fn resume_from_sequence(
        &self,
        run: RunId,
        after: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, RunSupervisorError> {
        self.0.resume_from_sequence(run, after, limit).await
    }
}
struct Finisher;
#[async_trait]
impl InferenceProvider for Finisher {
    async fn chat(
        &self,
        _: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        Ok(ChatResponse {
            message: Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: serde_json::json!({"summary":"candidate done"}),
                },
            }]),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}
#[derive(Default)]
struct Sink(Mutex<Vec<ApplicationEvent>>);
impl ApplicationEventSink for Sink {
    fn send(&self, event: ApplicationEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn sessionless_completion_failure_is_reported_by_inline_and_detached_doors() {
    for detached in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let store = lokai_memory::SharedStore::open(tmp.path().join("db.sqlite"), 1).unwrap();
        let supervisor = Arc::new(FailFinish(DurableRunSupervisor::new(Some(store.clone()))));
        let sink = Arc::new(Sink::default());
        let artifacts = Arc::new(
            lokai_artifact::LocalArtifactStore::new(
                tmp.path().join("artifacts"),
                lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
            )
            .unwrap(),
        );
        let checked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = checked.clone();
        let runs = DefaultRunService::new(
            Some(store),
            Arc::new(lokai_policy::PolicyEngine::default()),
            sink.clone(),
            supervisor.clone(),
            Arc::new(lokai_app::SessionLiveStore::new()),
            artifacts,
        )
        .with_execution_policy(Arc::new(move |identity, spec, role, _, _| {
            assert_eq!(identity.unwrap().owning_application, "factory");
            assert_eq!(spec.definition_digest, "factory-v1");
            assert_eq!(role, None); // manager must not substitute a coding role
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }));
        let identity = AgentIdentity {
            id: IdentityId::new("factory-actor"),
            owning_application: "factory".into(),
            bound_definition_digest: "factory-v1".into(),
            privilege_class: "default".into(),
            toolset_subscriptions: vec![],
            context_bindings: vec![],
            recovery_id: "factory".into(),
        };
        let cmd = StartIdentityJobCommand {
            job_spec: AgentJobSpec {
                identity_id: identity.id.clone(),
                definition_digest: identity.bound_definition_digest.clone(),
                input_digest: lokai_run::job_input_digest("work"),
                capability_bindings: vec![],
                artifact_bindings: vec![],
                recovery_id: "job".into(),
            },
            identity,
            invocation: AgentInvocation {
                instructions: "complete work".into(),
                user_input: "work".into(),
                explain_turn: false,
                empty_tool_nudge: false,
                max_steps: 2,
                completion_tool: "finish".into(),
                discipline: Default::default(),
            },
        };
        let mut agent = lokai_core::Agent::new(
            Arc::new(Finisher),
            lokai_tools::Tools::new(lokai_tools::Workspace::new(tmp.path()).unwrap(), false),
            Default::default(),
        );
        let result = if detached {
            tokio::task::LocalSet::new()
                .run_until(async {
                    let (_, rx) = runs.submit_identity_job(cmd, agent).await.unwrap();
                    tokio::time::timeout(std::time::Duration::from_secs(5), rx)
                        .await
                        .unwrap()
                        .unwrap()
                })
                .await
        } else {
            runs.start_identity_job(cmd, &mut agent).await.unwrap()
        };
        assert_eq!(checked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            matches!(result.outcome, CandidateOutcome::Failed { .. }),
            "{:?}",
            result.outcome
        );
        assert_ne!(
            supervisor.snapshot(result.run_id).await.unwrap().state,
            RunState::Succeeded
        );
        let events = sink.0.lock().unwrap();
        let terminal: Vec<_> = events
            .iter()
            .filter_map(|ev| match ev {
                ApplicationEvent::TurnCompleted { status, .. } => Some(status),
                _ => None,
            })
            .collect();
        assert_eq!(terminal.len(), 1);
        assert_ne!(terminal[0], "ok");
    }
}
