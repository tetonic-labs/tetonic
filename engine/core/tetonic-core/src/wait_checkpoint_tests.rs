use super::*;
use tetonic_inference::{FunctionCall, Message, ToolCall};

fn checkpoint() -> WaitCheckpoint {
    let invocation = tetonic_domain::AgentInvocation {
        instructions: "Coordinate the agreed work".into(),
        user_input: "Shared brief".into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 8,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline {
            spawn_tool: Some("dispatch".into()),
            ..Default::default()
        },
    };
    let pending = SpawnRequest {
        tool_name: "dispatch".into(),
        call_id: "tc_1_1".into(),
        attempt_id: Some("attempt".into()),
        parent_agent_id: "coordinator".into(),
        arguments: serde_json::json!({"keys":["second"]}),
        ..Default::default()
    };
    let received = ReceivedHostCall::new(
        SpawnRequest {
            call_id: "tc_1_0".into(),
            arguments: serde_json::json!({"keys":["first"]}),
            ..pending.clone()
        },
        "Private contribution",
    );
    WaitCheckpoint {
        version: 2,
        harness: serde_json::json!({}),
        // Compaction already removed the original received call and its result.
        messages: vec![
            Message::system("Compacted context"),
            Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: pending.tool_name.clone(),
                    arguments: pending.arguments.clone(),
                },
            }]),
        ],
        pending,
        received_host_calls: vec![received],
        prefix_len: 1,
        nonce: 1,
        call_no: 2,
        retrieved_paths: vec![],
        turn_id: Some("turn".into()),
        steps_used: 2,
        step_index: 2,
        reported_tokens: 321,
        monitor: crate::monitor::HeuristicMonitor::new(
            &crate::AgentConfig::default(),
            &invocation.discipline,
        ),
        invocation,
    }
}

#[test]
fn protected_checkpoint_scan_distinguishes_protocol_state_from_disclosed_content() {
    const OPAQUE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut value = serde_json::to_value(checkpoint()).unwrap();
    let id = format!("call_{OPAQUE}");
    value["pending"]["call_id"] = serde_json::json!(id);
    value["messages"][1]["continuation"] = serde_json::json!({
        "protocol":"openai-responses", "model":"pinned", "call_ids":[id],
        "items":[
            {"type":"reasoning","id":OPAQUE,"encrypted_content":OPAQUE,"summary":[]},
            {"type":"function_call","call_id":id,"name":"dispatch","arguments":"{\"keys\":[\"second\"]}"}
        ]
    });
    let saved: WaitCheckpoint = serde_json::from_value(value).unwrap();
    let scan = saved.disclosure_scan_text().unwrap();
    assert!(!scan.contains(OPAQUE));
    assert!(scan.contains("Shared brief") && scan.contains("Coordinate the agreed work"));
    assert!(
        serde_json::to_string(&saved).unwrap().contains(OPAQUE),
        "sealed checkpoint retains exact provider state"
    );
    for kind in 0..4 {
        let mut contaminated = saved.clone();
        match kind {
            0 => contaminated.invocation.user_input = OPAQUE.into(),
            1 => contaminated.messages[0].content = OPAQUE.into(),
            2 => {
                contaminated.harness =
                    serde_json::json!({"tool_schema":{"encrypted_content":OPAQUE}})
            }
            _ => {
                contaminated.received_host_calls[0].request.arguments =
                    serde_json::json!({"secret":OPAQUE})
            }
        }
        assert!(contaminated
            .disclosure_scan_text()
            .unwrap()
            .contains(OPAQUE));
    }
    let mut invalid = saved;
    invalid.pending.call_id = "mismatched".into();
    assert!(invalid.disclosure_scan_text().is_err());
}

#[test]
fn received_calls_survive_compaction_and_checkpoint_roundtrip_without_duplicating_output() {
    let bytes = serde_json::to_vec(&checkpoint()).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("Private contribution"));
    let saved: WaitCheckpoint = serde_json::from_slice(&bytes).unwrap();
    assert!(saved.validate().is_ok());
    let calls = saved.received_host_calls().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].matches_response("Private contribution"));
    assert!(!calls[0].matches_response("different contribution"));
    let mut conversation = saved.into_conversation().unwrap();
    assert_eq!(conversation.received_host_calls.len(), 1);
    assert_eq!(
        conversation
            .pending_resume
            .as_ref()
            .unwrap()
            .reported_tokens,
        321
    );
    conversation.begin_turn();
    assert!(conversation.received_host_calls.is_empty());
    let audit = Conversation::from_audit_messages(conversation.messages);
    assert!(
        audit.received_host_calls.is_empty(),
        "a transcript cannot invent delivery evidence"
    );
}

#[test]
fn malformed_delivery_history_and_pending_call_are_rejected() {
    for variant in 0..8 {
        let mut saved = checkpoint();
        match variant {
            0 => saved.received_host_calls[0].request.attempt_id = Some("other".into()),
            1 => saved.received_host_calls[0].request.parent_agent_id = "other".into(),
            2 => saved.received_host_calls[0].request.tool_name = "shell".into(),
            3 => saved.received_host_calls[0].response_digest = "bad".into(),
            4 => saved.received_host_calls[0].request.call_id = saved.pending.call_id.clone(),
            5 => {
                saved.steps_used = 4;
                saved
                    .received_host_calls
                    .push(saved.received_host_calls[0].clone());
            }
            6 => saved.pending.arguments = serde_json::json!({"keys":["changed"]}),
            _ => saved.pending.call_id = "wrong-call".into(),
        }
        assert!(saved.validate().is_err(), "variant {variant}");
    }
}

#[test]
fn old_root_checkpoints_remain_readable_but_have_no_coordinator_delivery_contract() {
    let mut value = serde_json::to_value(checkpoint()).unwrap();
    value["version"] = 1.into();
    value.as_object_mut().unwrap().remove("received_host_calls");
    let saved: WaitCheckpoint = serde_json::from_value(value).unwrap();
    assert!(saved.validate().is_ok());
    assert!(saved.received_host_calls().is_err());
}

#[test]
fn provider_pending_ids_and_protocol_state_survive_the_new_checkpoint_version() {
    for protocol in [
        "openai-responses",
        "anthropic-messages",
        "google-generate-content",
    ] {
        let mut value = serde_json::to_value(checkpoint()).unwrap();
        value["pending"]["call_id"] = "provider-pending-id".into();
        value["messages"][1]["continuation"] = serde_json::json!({"protocol":protocol,"model":"pinned","items":[{"opaque":"PRIVATE_CANARY"}],"call_ids":["provider-pending-id"]});
        let saved: WaitCheckpoint = serde_json::from_value(value).unwrap();
        assert!(saved.validate().is_ok());
        let bytes = serde_json::to_vec(&saved).unwrap();
        let restored: WaitCheckpoint = serde_json::from_slice(&bytes).unwrap();
        assert!(restored.validate().is_ok());
        assert!(restored.received_host_calls().unwrap()[0].matches_response("Private contribution"));
        assert!(!serde_json::to_string(&restored.messages[1])
            .unwrap()
            .contains("PRIVATE_CANARY"));
    }
}
