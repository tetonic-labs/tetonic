//! Invalid JSON and asynchronous host failures must obey the same progress cap
//! as ordinary tools, without preventing a corrected call or changing authority.
use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

struct SequenceProvider {
    arguments: Vec<Value>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

#[async_trait]
impl InferenceProvider for SequenceProvider {
    async fn chat(
        &self,
        request: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request);
        let (name, arguments) = match self.arguments.get(index) {
            Some(args) => ("control", args.clone()),
            None => (
                "finish",
                serde_json::json!({"summary":"Finished after correction"}),
            ),
        };
        Ok(ChatResponse {
            message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                function: tetonic_inference::FunctionCall {
                    name: name.into(),
                    arguments,
                },
            }]),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

#[derive(Clone)]
struct RecoveryHost(Arc<AtomicUsize>);

impl ToolHost for RecoveryHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
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
    fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String> {
        if name == "control" && !args["valid"].as_bool().unwrap_or(false) {
            Err("missing required valid field".into())
        } else {
            Ok(())
        }
    }
    fn execute_authorized(
        &self,
        _: &str,
        _: &Value,
        _: Option<&AuthorizedAction>,
        _: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok("Recorded", "Recorded")
    }
}

async fn exercise(
    arguments: Vec<Value>,
    host_control: bool,
    expected_calls: usize,
    expected_effects: usize,
    completed: bool,
) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let effects = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent::new(
        Arc::new(SequenceProvider {
            arguments,
            requests: requests.clone(),
        }),
        RecoveryHost(effects.clone()),
        AgentConfig {
            no_progress_limit: 2,
            ..Default::default()
        },
    );
    let mut invocation = test_inv("Prepare work within the current permissions");
    if host_control {
        invocation.discipline.spawn_tool = Some("control".into());
        let effects = effects.clone();
        agent = agent.with_spawn(Box::new(move |request, _| {
            let effects = effects.clone();
            Box::pin(async move {
                if request.arguments["reject"] == true {
                    ToolOutcome::fail("Proposal still invalid", "rejected")
                } else {
                    effects.fetch_add(1, Ordering::SeqCst);
                    ToolOutcome::ok("Recorded", "Recorded")
                }
            })
        }));
    }
    let mut conversation = Conversation::new();
    let outcome = agent.turn(&mut conversation, invocation, |_| {}).await;
    assert_eq!(requests.lock().unwrap().len(), expected_calls);
    assert_eq!(effects.load(Ordering::SeqCst), expected_effects);
    if completed {
        assert!(
            matches!(outcome, CandidateOutcome::Completed { .. }),
            "{outcome:?}"
        );
    } else {
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
    }
    // The rejected call's specific diagnostic reaches the next inference call.
    assert!(requests.lock().unwrap()[1]
        .messages
        .iter()
        .any(|m| m.role == "tool"
            && (m.content.contains("missing required valid field")
                || m.content.contains("Proposal still invalid"))));
}

#[tokio::test]
async fn malformed_calls_stop_before_spending_the_entire_step_budget() {
    exercise(
        vec![serde_json::json!({}), serde_json::json!({})],
        false,
        2,
        0,
        false,
    )
    .await;
}

#[tokio::test]
async fn corrected_arguments_can_execute_and_finish() {
    exercise(
        vec![serde_json::json!({}), serde_json::json!({"valid":true})],
        false,
        3,
        1,
        true,
    )
    .await;
}

#[tokio::test]
async fn failed_host_controls_stop_and_successful_controls_reset_the_failure_streak() {
    let failed = serde_json::json!({"valid":true,"reject":true});
    let valid = serde_json::json!({"valid":true});
    exercise(vec![failed.clone(), failed.clone()], true, 2, 0, false).await;
    // Repeated successful control reads are legitimate even with identical args.
    exercise(
        vec![
            failed.clone(),
            valid.clone(),
            valid.clone(),
            valid.clone(),
            failed,
            valid,
        ],
        true,
        7,
        4,
        true,
    )
    .await;
}

#[tokio::test]
async fn argument_and_host_failures_share_one_progress_limit() {
    exercise(
        vec![
            serde_json::json!({}),
            serde_json::json!({"valid":true,"reject":true}),
        ],
        true,
        2,
        0,
        false,
    )
    .await;
}
