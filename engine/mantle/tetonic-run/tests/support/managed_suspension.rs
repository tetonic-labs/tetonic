//! Real managed executor/SQLite/artifact-store tests; no provider network traffic.
use super::*;
use std::sync::atomic::AtomicUsize;
use tetonic_domain::{AgentInvocation, AttemptState, RunState, ToolHost, ToolOutcome};
use tetonic_run::managed::{ManagedSubmission, StartIdentityJobCommand};

#[path = "managed_suspension_batch.rs"]
mod batch;
#[path = "managed_suspension_faults.rs"]
mod faults;

fn waiting_service(
    path: &std::path::Path,
    artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
) -> ManagedRunService {
    let service = durable_service(path, artifacts);
    service
        .store()
        .unwrap()
        .write_sync(|db| {
            db.register_control_principal("alice")?;
            db.set_organization_member(
                "org",
                "alice",
                tetonic_memory::OrganizationRole::Administrator,
            )?;
            if db.information_context_kind("private")?.is_some() {
                return Ok(());
            }
            db.create_information_context(
                "alice",
                "private",
                &tetonic_memory::ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
        })
        .unwrap()
        .unwrap();
    service
}

struct Provider(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl tetonic_inference::InferenceProvider for Provider {
    async fn chat(
        &self,
        request: tetonic_inference::ChatRequest,
        _: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        let (name, args) = match n {
            0 => ("update", serde_json::json!({})),
            1 => ("ask_human", serde_json::json!({"question":"Audience?"})),
            2 => {
                assert_eq!(
                    request.messages.iter().filter(|m| m.role == "user").count(),
                    1
                );
                assert!(request
                    .messages
                    .iter()
                    .any(|m| m.role == "tool" && m.content.contains("Beginners")));
                (
                    "finish",
                    serde_json::json!({"summary":"Ready for beginners"}),
                )
            }
            _ => panic!("previous inference was replayed"),
        };
        Ok(tetonic_inference::ChatResponse {
            message: tetonic_inference::Message::assistant("").with_tool_calls(vec![
                tetonic_inference::ToolCall {
                    function: tetonic_inference::FunctionCall {
                        name: name.into(),
                        arguments: args,
                    },
                },
            ]),
            usage: tetonic_inference::GenUsage {
                eval_tokens: Some(10),
                ..Default::default()
            },
            provenance: Default::default(),
        })
    }
}
#[derive(Clone)]
struct Host(Arc<AtomicUsize>);
impl ToolHost for Host {
    fn checkpoint_ready(&self) -> bool {
        true
    }
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(&self, _: &str, _: &serde_json::Value) -> Option<tetonic_domain::ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, _: &str) -> bool {
        true
    }
    fn is_read_only(&self, _: &str) -> bool {
        false
    }
    fn advertisements(&self) -> Vec<tetonic_domain::ToolAdvertisement> {
        ["update", "ask_human", "finish"]
            .into_iter()
            .map(|name| tetonic_domain::ToolAdvertisement {
                name: name.into(),
                description: "fixture".into(),
                parameters: serde_json::json!({"type":"object"}),
            })
            .collect()
    }
    fn validate_tool_args(&self, _: &str, _: &serde_json::Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        _: &serde_json::Value,
        _: Option<&tetonic_domain::AuthorizedAction>,
        _: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        assert_eq!(name, "update");
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok("updated", "previous action finished")
    }
}

fn agent(
    calls: Arc<AtomicUsize>,
    effects: Arc<AtomicUsize>,
    answer: Arc<AtomicBool>,
    ceiling: u64,
) -> tetonic_core::Agent {
    tetonic_core::Agent::new(
        Arc::new(Provider(calls)),
        Host(effects),
        tetonic_core::AgentConfig {
            max_steps: 4,
            reported_token_ceiling: Some(ceiling),
            ..Default::default()
        },
    )
    .with_spawn(Box::new(move |request, _| {
        let answer = answer.clone();
        Box::pin(async move {
            assert_eq!(request.tool_name, "ask_human");
            while !answer.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            ToolOutcome::ok("answered", "Beginners")
        })
    }))
    .with_durable_waits()
}

fn command() -> StartIdentityJobCommand {
    let (identity, mut job_spec) = test_identity_and_spec();
    job_spec.capability_bindings = vec!["update".into(), "ask_human".into(), "finish".into()];
    StartIdentityJobCommand {
        identity,
        job_spec,
        invocation: AgentInvocation {
            instructions: "Follow the fixture. PRIVATE_CHECKPOINT_CANARY".into(),
            user_input: "hello".into(),
            explain_turn: false,
            empty_tool_nudge: false,
            max_steps: 4,
            completion_tool: "finish".into(),
            discipline: tetonic_domain::LoopDiscipline {
                handoff_tool: Some("ask_human".into()),
                ..Default::default()
            },
        },
    }
}
fn admission(allowed: Arc<AtomicBool>, seconds: u64) -> AdmissionContext {
    let mut context = context(allowed, "original-audit");
    context.deadline = Some(chrono::Utc::now().timestamp() as u64 + seconds);
    context
}
async fn parked(
    service: &ManagedRunService,
    binding: &tetonic_run::ManagedBinding,
) -> tetonic_domain::RunSnapshot {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
            if snapshot.attempts[&binding.attempt_id].state == AttemptState::Suspended {
                return snapshot;
            }
            assert!(
                !matches!(snapshot.state, RunState::Failed | RunState::Canceled),
                "{:?}",
                snapshot
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn waiting_releases_capacity_freezes_execution_time_and_reclaims_before_inference() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (base, dir) = test_service();
            let path = dir.path().join("waiting.db");
            let service = waiting_service(&path, base.artifacts().clone());
            let allowed = Arc::new(AtomicBool::new(true));
            let answer = Arc::new(AtomicBool::new(false));
            let calls = Arc::new(AtomicUsize::new(0));
            let effects = Arc::new(AtomicUsize::new(0));
            let ManagedSubmission::Started {
                binding,
                completion,
            } = service
                .submit_identity_job_with_context(
                    command(),
                    agent(calls.clone(), effects.clone(), answer.clone(), 100),
                    admission(allowed.clone(), 3),
                    None,
                )
                .await
                .unwrap()
            else {
                panic!("not started")
            };
            let snapshot = parked(&service, &binding).await;
            let original_deadline = snapshot.tasks[&binding.task_id].binding.deadline.unwrap();
            // A distinct request for the same registered identity can take the freed slot.
            let mut other_context = context(allowed, "second-audit");
            other_context.activation.as_mut().unwrap().request_id = "second-work".into();
            let ManagedAdmission::Admitted(second) = admit(&service, other_context).await.unwrap()
            else {
                panic!("capacity not released")
            };
            while chrono::Utc::now().timestamp() as u64 <= original_deadline {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            answer.store(true, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "answer must not bypass occupied identity capacity"
            );
            assert_eq!(
                service.inspect_run(&binding.run_id).await.unwrap().attempts[&binding.attempt_id]
                    .state,
                AttemptState::Suspended
            );
            service.cancel_run(&second.run_id).await.unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(10), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(result.outcome.is_completed(), "{:?}", result.outcome);
            assert_eq!(effects.load(Ordering::SeqCst), 1);
            let snapshot = service.inspect_run(&binding.run_id).await.unwrap();
            assert_eq!(snapshot.attempts.len(), 1);
            assert_eq!(snapshot.state, RunState::Succeeded);
            assert!(snapshot.tasks[&binding.task_id].binding.deadline.unwrap() > original_deadline);
            assert!(!serde_json::to_string(&snapshot)
                .unwrap()
                .contains("PRIVATE_CHECKPOINT_CANARY"));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn restart_restores_exact_pending_call_and_keeps_effort_counters() {
    for ceiling in [100, 20] {
        let (base, dir) = test_service();
        let path = dir.path().join("restart.db");
        let service = waiting_service(&path, base.artifacts().clone());
        let allowed = Arc::new(AtomicBool::new(true));
        let answer = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let effects = Arc::new(AtomicUsize::new(0));
        let local = tokio::task::LocalSet::new();
        let (receipt, attempt) = local
            .run_until(async {
                let ManagedSubmission::Started { binding, .. } = service
                    .submit_identity_job_with_context(
                        command(),
                        agent(calls.clone(), effects.clone(), answer.clone(), ceiling),
                        admission(allowed.clone(), 30),
                        None,
                    )
                    .await
                    .unwrap()
                else {
                    panic!()
                };
                parked(&service, &binding).await;
                (
                    ActivationReceipt {
                        run_id: binding.run_id,
                        task_id: binding.task_id,
                        audit_session_id: "original-audit".into(),
                    },
                    binding.attempt_id,
                )
            })
            .await;
        drop(local); // executor vanished; only the sealed checkpoint and journal survive
        let reopened = waiting_service(&path, base.artifacts().clone());
        assert_eq!(
            reopened.inspect_run(&receipt.run_id).await.unwrap().state,
            RunState::Active
        );
        tokio::task::LocalSet::new()
            .run_until(async {
                allowed.store(false, Ordering::SeqCst);
                assert!(reopened
                    .restore_suspended_root(
                        receipt.clone(),
                        command(),
                        agent(calls.clone(), effects.clone(), answer.clone(), ceiling),
                        admission(allowed.clone(), 999),
                        None
                    )
                    .await
                    .is_err());
                assert_eq!(calls.load(Ordering::SeqCst), 2);
                allowed.store(true, Ordering::SeqCst);
                assert!(
                    reopened
                        .restore_suspended_root(
                            receipt.clone(),
                            command(),
                            agent(calls.clone(), effects.clone(), answer.clone(), ceiling + 1),
                            admission(allowed.clone(), 999),
                            None,
                        )
                        .await
                        .is_err(),
                    "a new token allowance must not replace the pinned one"
                );
                let mut wrong = command();
                wrong.invocation.instructions = "different task".into();
                assert!(reopened
                    .restore_suspended_root(
                        receipt.clone(),
                        wrong,
                        agent(calls.clone(), effects.clone(), answer.clone(), ceiling),
                        admission(allowed.clone(), 999),
                        None
                    )
                    .await
                    .is_err());
                let ManagedSubmission::Started {
                    binding,
                    completion,
                } = reopened
                    .restore_suspended_root(
                        receipt.clone(),
                        command(),
                        agent(calls.clone(), effects.clone(), answer.clone(), ceiling),
                        admission(allowed.clone(), 999),
                        None,
                    )
                    .await
                    .unwrap()
                else {
                    panic!("not restored")
                };
                assert_eq!(binding.attempt_id, attempt);
                assert!(matches!(
                    reopened
                        .restore_suspended_root(
                            receipt.clone(),
                            command(),
                            agent(calls.clone(), effects.clone(), answer.clone(), ceiling),
                            admission(allowed.clone(), 999),
                            None
                        )
                        .await
                        .unwrap(),
                    ManagedSubmission::Existing(_)
                ));
                answer.store(true, Ordering::SeqCst);
                let result = tokio::time::timeout(std::time::Duration::from_secs(10), completion)
                    .await
                    .unwrap()
                    .unwrap();
                if ceiling == 100 {
                    assert!(result.outcome.is_completed(), "{:?}", result.outcome);
                } else {
                    assert!(
                        matches!(result.outcome, CandidateOutcome::Limited { .. }),
                        "saved token usage must still apply: {:?}",
                        result.outcome
                    );
                }
                assert_eq!(
                    calls.load(Ordering::SeqCst),
                    if ceiling == 100 { 3 } else { 2 }
                );
                assert_eq!(
                    effects.load(Ordering::SeqCst),
                    1,
                    "a completed pre-wait effect cannot be replayed"
                );
                assert_eq!(
                    reopened
                        .inspect_run(&receipt.run_id)
                        .await
                        .unwrap()
                        .attempts
                        .len(),
                    1
                );
            })
            .await;
    }
}
