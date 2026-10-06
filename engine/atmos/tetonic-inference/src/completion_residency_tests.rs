//! A finished zero-retention response can outlive its runner, not its evidence.
use super::*;

fn request(secret: bool) -> ChatRequest {
    ChatRequest {
        model: "test-model".into(),
        num_ctx: Some(4096),
        keep_alive: Some("30m".into()),
        fabric: Some(FabricCallMeta {
            data_class: if secret {
                DataClass::Secret
            } else {
                DataClass::RepositorySource
            },
            ..Default::default()
        }),
        outbound_scan: OutboundScan::from_scan(false),
        ..Default::default()
    }
}

async fn after_dispatch(
    chunks: Vec<Value>,
    placement: Value,
) -> (OllamaProvider, tokio::task::JoinHandle<Vec<Value>>) {
    scripted_chunks(vec![
        ("/api/ps", vec![json!({"models":[]})]),
        ("/api/generate", vec![json!({"done":true})]),
        ("/api/ps", vec![resident(4096)]),
        ("/api/chat", chunks),
        ("/api/ps", vec![placement]),
    ])
    .await
}

#[tokio::test]
async fn finished_secret_response_keeps_content_tools_and_usage_after_expected_unload() {
    for buffered in [false, true] {
        let mut chunks = vec![];
        if buffered {
            chunks.push(json!({"done":false,"message":{"role":"assistant","thinking":"PRIVATE_REASONING_CANARY","content":""}}));
            chunks.push(json!({"done":false,"message":{"role":"assistant","content":"Ready. "}}));
        }
        chunks.push(json!({"done":true,"done_reason":"stop","prompt_eval_count":87,"eval_count":13,
            "message":{"role":"assistant","content":"Result","tool_calls":[{"function":{"name":"finish","arguments":{"summary":"Result"}}}]}}));
        let (provider, server) = after_dispatch(chunks, json!({"models":[]})).await;
        let mut streamed = vec![];
        let response = provider
            .chat(request(true), &mut |s| streamed.push(s.to_owned()))
            .await
            .unwrap();
        assert_eq!(
            response.message.content,
            if buffered { "Ready. Result" } else { "Result" }
        );
        assert_eq!(
            streamed,
            vec![response.message.content.clone()],
            "buffered output is released only on the completion receipt"
        );
        assert_eq!(
            response.message.tool_calls.unwrap()[0].function.name,
            "finish"
        );
        assert_eq!(response.usage.prompt_tokens, Some(87));
        assert_eq!(response.usage.eval_tokens, Some(13));
        assert_eq!(response.usage.finish_reason.as_deref(), Some("stop"));
        let requests = server.await.unwrap();
        assert_eq!(
            requests[3]["keep_alive"], "0",
            "privacy retention must not be relaxed"
        );
        assert_eq!(requests[3]["options"]["num_ctx"], 4096);
        assert_eq!(requests.len(), 5, "no model replay or extra load");
    }
}

#[tokio::test]
async fn missing_runner_does_not_turn_truncated_secret_output_into_success() {
    let (provider, server) = after_dispatch(
        vec![json!({"done":false,"message":{"role":"assistant","content":"partial"}})],
        json!({"models":[]}),
    )
    .await;
    let result = provider
        .chat(request(true), &mut |_| {
            panic!("unconfirmed response must not be released")
        })
        .await;
    assert!(matches!(
        result,
        Err(InferenceError::IncompleteStream { .. })
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn unexpected_disappearance_and_observed_cpu_spill_still_fail_closed() {
    for (secret, placement) in [(false, json!({"models":[]})), (true, spilled())] {
        let (provider, server) = after_dispatch(
            vec![json!({"done":true,"message":{"role":"assistant","content":"answer"}})],
            placement,
        )
        .await;
        let result = provider
            .chat(request(secret), &mut |_| panic!("invalid placement"))
            .await;
        if secret {
            assert!(matches!(
                result,
                Err(InferenceError::GpuSpillDetected { .. })
            ));
        } else {
            assert!(matches!(
                result,
                Err(InferenceError::ModelResidencyUnavailable)
            ));
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn secret_requests_still_require_placement_before_sending_the_prompt() {
    let (provider, server) = scripted(vec![
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", json!({"models":[]})),
    ])
    .await;
    assert!(matches!(
        provider
            .chat(request(true), &mut |_| panic!("no generation authorized"))
            .await,
        Err(InferenceError::ModelResidencyUnavailable)
    ));
    assert!(server
        .await
        .unwrap()
        .iter()
        .all(|r| r.get("messages").is_none()));
}

#[tokio::test]
async fn provider_errors_are_not_obscured_by_a_post_response_residency_check() {
    let (provider, server) = scripted(vec![
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", resident(4096)),
        ("/api/chat", json!({"error":"runner failed"})),
    ])
    .await;
    assert!(
        matches!(provider.chat(request(true), &mut |_| panic!("failed response")).await, Err(InferenceError::Provider(message)) if message == "runner failed")
    );
    assert_eq!(server.await.unwrap().len(), 4);
}
