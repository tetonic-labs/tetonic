use super::*;

struct BatchProvider(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl tetonic_inference::InferenceProvider for BatchProvider {
    async fn chat(
        &self,
        request: tetonic_inference::ChatRequest,
        _: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
        let calls = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            vec![
                ("update", serde_json::json!({})),
                ("ask_human", serde_json::json!({"question":"Audience?"})),
            ]
        } else {
            assert_eq!(
                request
                    .messages
                    .iter()
                    .filter(|m| m.role == "tool"
                        && m.content.contains("No calls in this batch were executed"))
                    .count(),
                2
            );
            vec![("finish", serde_json::json!({"summary":"No changes made"}))]
        };
        Ok(tetonic_inference::ChatResponse {
            message: tetonic_inference::Message::assistant("").with_tool_calls(
                calls
                    .into_iter()
                    .map(|(name, arguments)| tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: name.into(),
                            arguments,
                        },
                    })
                    .collect(),
            ),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn handoff_batch_cannot_partially_execute_then_lose_its_remaining_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (base, dir) = test_service();
            let service = waiting_service(&dir.path().join("batch.db"), base.artifacts().clone());
            let calls = Arc::new(AtomicUsize::new(0));
            let effects = Arc::new(AtomicUsize::new(0));
            let agent = tetonic_core::Agent::new(
                Arc::new(BatchProvider(calls.clone())),
                Host(effects.clone()),
                tetonic_core::AgentConfig {
                    max_steps: 4,
                    ..Default::default()
                },
            )
            .with_spawn(Box::new(|_, _| {
                Box::pin(async { panic!("batched handoff must not run") })
            }))
            .with_durable_waits();
            let ManagedSubmission::Started {
                binding,
                completion,
            } = service
                .submit_identity_job_with_context(
                    command(),
                    agent,
                    admission(Arc::new(AtomicBool::new(true)), 30),
                    None,
                )
                .await
                .unwrap()
            else {
                panic!()
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(result.outcome.is_completed(), "{:?}", result.outcome);
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert_eq!(effects.load(Ordering::SeqCst), 0);
            assert!(service.inspect_run(&binding.run_id).await.unwrap().attempts
                [&binding.attempt_id]
                .suspension
                .is_none());
        })
        .await;
}
