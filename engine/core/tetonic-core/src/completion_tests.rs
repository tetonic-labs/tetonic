use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};
use tetonic_domain::work_scope::CancellationSignal;

struct Script {
    messages: Vec<Message>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}
#[async_trait]
impl InferenceProvider for Script {
    async fn chat(
        &self,
        request: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len().min(self.messages.len() - 1);
        requests.push(request);
        Ok(ChatResponse {
            message: self.messages[index].clone(),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

#[derive(Clone, Default)]
struct Host {
    ready: Arc<AtomicBool>,
    effects: Arc<AtomicUsize>,
    on_check: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl ToolHost for Host {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn check_completion(
        &self,
        _: &CancellationSignal,
    ) -> Result<(), tetonic_domain::CompletionBlocker> {
        if let Some(check) = &self.on_check {
            check();
        }
        if self.ready.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(tetonic_domain::CompletionBlocker::new(
                "Repair the failed check before finishing",
                "verification_required",
            ))
        }
    }
    fn propose(&self, _: &str, _: &Value) -> Option<ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, _: &str) -> bool {
        true
    }
    fn is_read_only(&self, _: &str) -> bool {
        true
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        vec![]
    }
    fn validate_tool_args(&self, _: &str, _: &Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        _: &Value,
        _: Option<&AuthorizedAction>,
        _: &CancellationSignal,
    ) -> ToolOutcome {
        self.effects.fetch_add(1, Ordering::SeqCst);
        if name == "repair" {
            self.ready.store(true, Ordering::SeqCst);
        }
        ToolOutcome::ok("Checked", "Checked")
    }
}
fn calls(names: &[&str]) -> Message {
    Message::assistant("").with_tool_calls(
        names
            .iter()
            .map(|name| tetonic_inference::ToolCall {
                function: tetonic_inference::FunctionCall {
                    name: (*name).into(),
                    arguments: serde_json::json!({"summary":"Complete"}),
                },
            })
            .collect(),
    )
}
fn setup(messages: Vec<Message>, host: Host) -> (Agent, Arc<Mutex<Vec<ChatRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let agent = Agent::new(
        Arc::new(Script {
            messages,
            requests: requests.clone(),
        }),
        host,
        AgentConfig {
            no_progress_limit: 2,
            ..Default::default()
        },
    );
    (agent, requests)
}

#[tokio::test]
async fn premature_completion_returns_feedback_then_allows_repair_in_the_same_turn() {
    for prose in [false, true] {
        let host = Host::default();
        let answer = if prose {
            Message::assistant("All done")
        } else {
            calls(&["finish"])
        };
        let (agent, requests) = setup(
            vec![answer.clone(), calls(&["repair"]), answer],
            host.clone(),
        );
        let mut steps = Vec::new();
        let outcome = agent
            .turn(
                &mut Conversation::new(),
                test_inv_explain("Deliver", prose),
                |s| steps.push(s),
            )
            .await;
        assert!(
            matches!(outcome, CandidateOutcome::Completed { .. }),
            "{outcome:?}"
        );
        assert_eq!(host.effects.load(Ordering::SeqCst), 1);
        assert_eq!(
            steps
                .iter()
                .filter(|s| matches!(s, Step::Answer(_)))
                .count(),
            1
        );
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        let feedback = requests[1].messages.last().unwrap();
        assert!(feedback.content.contains("Repair the failed check"));
        assert_eq!(feedback.role, if prose { "user" } else { "tool" });
        if !prose {
            assert!(feedback.tool_call_id.is_some());
        }
    }
}

#[tokio::test]
async fn repeating_done_without_repair_stops_at_the_existing_progress_limit() {
    for prose in [false, true] {
        let answer = if prose {
            Message::assistant("All done")
        } else {
            calls(&["finish"])
        };
        let (agent, requests) = setup(vec![answer], Host::default());
        let outcome = agent
            .turn(
                &mut Conversation::new(),
                test_inv_explain("Deliver", prose),
                |s| assert!(!matches!(s, Step::Answer(_))),
            )
            .await;
        assert!(
            matches!(
                outcome,
                CandidateOutcome::Limited {
                    kind: LimitKind::NoProgress,
                    ..
                }
            ),
            "{outcome:?}"
        );
        assert_eq!(requests.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn effort_cap_answer_cannot_bypass_unfinished_work() {
    let mut message = calls(&["inspect"]);
    message.content =
        "This long explanation must not count as completion while a check still needs repair."
            .into();
    let (agent, requests) = setup(vec![message], Host::default());
    let mut invocation = test_inv_explain("Explain", true);
    invocation.max_steps = 1;
    let outcome = agent
        .turn(&mut Conversation::new(), invocation, |s| {
            assert!(!matches!(s, Step::Answer(_)))
        })
        .await;
    assert!(
        matches!(
            outcome,
            CandidateOutcome::Limited {
                kind: LimitKind::EffortCap,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn completion_batches_have_no_effects_and_every_call_gets_feedback() {
    for names in [vec!["finish", "repair"], vec!["repair", "finish"]] {
        let host = Host::default();
        host.ready.store(true, Ordering::SeqCst);
        let (agent, requests) = setup(vec![calls(&names), calls(&["finish"])], host.clone());
        let outcome = agent
            .turn(&mut Conversation::new(), test_inv("Deliver"), |_| {})
            .await;
        assert!(
            matches!(outcome, CandidateOutcome::Completed { .. }),
            "{outcome:?}"
        );
        assert_eq!(host.effects.load(Ordering::SeqCst), 0);
        let requests = requests.lock().unwrap();
        let feedback: Vec<_> = requests[1]
            .messages
            .iter()
            .filter(|m| m.role == "tool")
            .collect();
        assert_eq!(feedback.len(), 2);
        assert!(feedback
            .iter()
            .all(|m| m.content.contains("No calls in this batch were executed")));
        assert_ne!(feedback[0].tool_call_id, feedback[1].tool_call_id);
    }
}

#[tokio::test]
async fn cancellation_during_readiness_wins_over_completion() {
    let mut conversation = Conversation::new();
    let cancel = conversation.cancel_handle();
    let host = Host {
        on_check: Some(Arc::new(move || cancel.store(true, Ordering::SeqCst))),
        ..Default::default()
    };
    host.ready.store(true, Ordering::SeqCst);
    let (agent, _) = setup(vec![calls(&["finish"])], host);
    let outcome = agent
        .turn(&mut conversation, test_inv("Deliver"), |s| {
            assert!(!matches!(s, Step::Answer(_)))
        })
        .await;
    assert!(
        matches!(outcome, CandidateOutcome::Canceled { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn revocation_during_readiness_wins_over_completion() {
    struct Gate(Arc<AtomicBool>);
    #[async_trait]
    impl crate::ExecutionGate for Gate {
        async fn authorize(&self) -> Result<(), ()> {
            if self.0.load(Ordering::SeqCst) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    let revoked = Arc::new(AtomicBool::new(false));
    let flag = revoked.clone();
    let host = Host {
        on_check: Some(Arc::new(move || flag.store(true, Ordering::SeqCst))),
        ..Default::default()
    };
    host.ready.store(true, Ordering::SeqCst);
    let (mut agent, _) = setup(vec![calls(&["finish"])], host);
    agent.bind_execution_gate(Some(Arc::new(Gate(revoked))));
    let outcome = agent
        .turn(&mut Conversation::new(), test_inv("Deliver"), |s| {
            assert!(!matches!(s, Step::Answer(_)))
        })
        .await;
    assert!(
        matches!(outcome, CandidateOutcome::Failed { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn readiness_host_failure_cannot_be_treated_as_completion() {
    let host = Host {
        on_check: Some(Arc::new(|| panic!("fixture host failed"))),
        ..Default::default()
    };
    let (agent, requests) = setup(vec![calls(&["finish"])], host);
    let outcome = agent
        .turn(&mut Conversation::new(), test_inv("Deliver"), |s| {
            assert!(!matches!(s, Step::Answer(_)))
        })
        .await;
    assert!(
        matches!(outcome, CandidateOutcome::Failed { .. }),
        "{outcome:?}"
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
}
