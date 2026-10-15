use super::*;

#[tokio::test]
async fn pending_contributions_block_completion_through_mcp_and_skill_hosts() {
    let remaining = Arc::new(Mutex::new(
        ["research".into(), "review".into()].into_iter().collect(),
    ));
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    let registered = RegisteredToolHost {
        tools: tetonic_tools::Tools::without_repository().unwrap(),
        dispatch: Some(PlanDispatch {
            director: None,
            human: None,
            binding: "test".into(),
            sender,
            assignment_keys: vec!["research".into(), "review".into()],
            remaining: remaining.clone(),
        }),
    };
    assert!(registered
        .validate_tool_args("finish", &serde_json::json!({"summary":"done"}))
        .is_ok());
    let host = crate::skills::SkillToolHost {
        inner: Box::new(crate::mcp::McpToolHost {
            inner: Box::new(registered),
            registry: crate::mcp::McpRegistry::from_json(br#"{"connections":[]}"#).unwrap(),
            selected: Default::default(),
            consumer: Arc::new(tetonic_runtime::InMemoryCapabilityStore::default()),
            runtime: tokio::runtime::Handle::current(),
        }),
        library: None,
        selected: Default::default(),
    };
    let cancel = CancellationSignal::default();
    let blocked = host.check_completion(&cancel).unwrap_err();
    assert_eq!(blocked.kind, "incomplete_work");
    assert!(blocked.message.contains("research, review"));
    remaining.lock().unwrap().remove("research");
    let blocked = host.check_completion(&cancel).unwrap_err();
    assert!(blocked.message.contains("review"));
    assert!(!blocked.message.contains("research"));
    remaining.lock().unwrap().clear();
    assert!(host.check_completion(&cancel).is_ok());
}
