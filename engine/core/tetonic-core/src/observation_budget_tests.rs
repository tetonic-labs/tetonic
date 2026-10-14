use super::*;
use serde_json::json;
use std::sync::Mutex;

#[derive(Clone)]
struct Pages(Arc<Mutex<Vec<Value>>>);
impl ToolHost for Pages {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn is_tool_allowed(&self, _: &str) -> bool {
        true
    }
    fn is_read_only(&self, name: &str) -> bool {
        name == "observe"
    }
    fn propose(&self, _: &str, _: &Value) -> Option<ToolProposal> {
        None
    }
    fn limit_observation(&self, name: &str, args: &Value, bytes: usize) -> Option<Value> {
        (name == "observe").then(|| {
            let mut v = args.clone();
            v["bytes"] = json!(bytes);
            v
        })
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        ["observe", "deliver", "finish"]
            .iter()
            .map(|name| ToolAdvertisement {
                name: name.to_string(),
                description: "test".into(),
                parameters: json!({"type":"object"}),
            })
            .collect()
    }
    fn validate_tool_args(&self, _: &str, _: &Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        args: &Value,
        _: Option<&AuthorizedAction>,
        _: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        self.0
            .lock()
            .unwrap()
            .push(json!({"tool":name,"args":args}));
        ToolOutcome::ok(
            "actual observation",
            if name == "observe" {
                "x".repeat(args["bytes"].as_u64().unwrap() as usize)
            } else {
                "actual delivery receipt".into()
            },
        )
    }
}

struct BatchProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    low_budget: bool,
}
#[async_trait]
impl InferenceProvider for BatchProvider {
    async fn chat(
        &self,
        req: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let mut requests = self.requests.lock().unwrap();
        let first = requests.is_empty();
        requests.push(req);
        let output = if first {
            (0..6).map(|n| json!({"type":"function_call","call_id":format!("page-{n}"),"name":"observe","arguments":json!({"key":n,"bytes":1000000}).to_string()})).collect::<Vec<_>>()
        } else {
            vec![
                json!({"type":"function_call","call_id":"deliver","name":"deliver","arguments":"{}"}),
                json!({"type":"function_call","call_id":"finish","name":"finish","arguments":"{\"summary\":\"actual work receipt\"}"}),
            ]
        };
        let mut resp = tetonic_inference::hosted::responses::response(
            json!({"status":"completed","output":output}),
            "model",
        )
        .unwrap();
        resp.usage.prompt_tokens = Some(if self.low_budget && first { 1000 } else { 1 });
        resp.usage.eval_tokens = Some(0);
        Ok(resp)
    }
}

#[tokio::test]
async fn pages_share_one_allowance_without_dropping_evidence_or_breaking_native_batches() {
    for low_budget in [false, true] {
        let requests = Arc::new(Mutex::new(vec![]));
        let effects = Arc::new(Mutex::new(vec![]));
        let agent = Agent::new(
            Arc::new(BatchProvider {
                requests: requests.clone(),
                low_budget,
            }),
            Pages(effects.clone()),
            AgentConfig {
                model: "model".into(),
                reported_token_ceiling: low_budget.then_some(1200),
                ..Default::default()
            },
        );
        let mut convo = Conversation::new();
        let result = agent
            .turn(
                &mut convo,
                test_inv("ASSIGNED WORK MUST REMAIN VISIBLE"),
                |_| {},
            )
            .await;
        assert!(result.is_completed(), "{result:?}");
        let calls = effects.lock().unwrap();
        let reads: Vec<_> = calls.iter().filter(|v| v["tool"] == "observe").collect();
        let requests = requests.lock().unwrap();
        let last = requests.last().unwrap();
        assert!(last
            .messages
            .iter()
            .any(|m| m.content.contains("ASSIGNED WORK MUST REMAIN VISIBLE")));
        let observations: Vec<_> = last.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(observations.len(), 6, "never erase earlier results to fit");
        assert_eq!(
            reads.len(),
            6,
            "sizing must not prohibit evidence or verification reads"
        );
        assert!(reads.iter().all(|v| v["args"]["bytes"] == 512));
        assert_eq!(
            observations.iter().map(|m| m.content.len()).sum::<usize>(),
            3072
        );
        assert_eq!(
            calls.last().unwrap()["tool"],
            "deliver",
            "observation control must not suppress delivery"
        );
        tetonic_inference::hosted::responses::request(
            last,
            &tetonic_inference::hosted::HostedModelConfig::responses("model", 100),
        )
        .unwrap();
    }
}
