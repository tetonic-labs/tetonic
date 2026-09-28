    use super::*;
    use async_trait::async_trait;
    use tetonic_domain::{
        ActionKind, CapabilityError, ToolAdvertisement, ToolProposal, WorldAdapter,
    };
    use tetonic_inference::{ChatRequest, ChatResponse, FabricSnapshot, InferenceError, TokenSink};
    use tetonic_tools::Tools;

    fn test_inv(user: &str) -> AgentInvocation {
        test_inv_explain(user, false)
    }

    fn wire_test_capability_helpers(agent: Agent) -> Agent {
        agent
            .with_post_edit_snapshot(Arc::new(tetonic_tools::format_post_edit_snapshot))
            .with_resolve_under_root(Arc::new(|root, rel| {
                let abs =
                    tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
                std::fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
            }))
            .with_capture_workspace_version(Arc::new(|root, paths| {
                tetonic_transaction::version::capture_workspace_version(root, paths)
                    .map_err(|e| e.to_string())
            }))
    }

    fn test_inv_explain(user: &str, explain_turn: bool) -> AgentInvocation {
        AgentInvocation {
            instructions: "You are a test agent operating in the user's workspace.".into(),
            user_input: user.to_string(),
            explain_turn,
            empty_tool_nudge: false,
            max_steps: 16,
            completion_tool: "finish".into(),
            discipline: tetonic_domain::LoopDiscipline::default(),
        }
    }

    struct CountingTool;

    impl ToolHost for CountingTool {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(CountingTool)
        }
        fn propose(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Option<tetonic_domain::tool_host::ToolProposal> {
            None
        }
        fn is_tool_allowed(&self, name: &str) -> bool {
            name == "again"
        }
        fn is_read_only(&self, _: &str) -> bool {
            true
        }
        fn advertisements(&self) -> Vec<ToolAdvertisement> {
            vec![ToolAdvertisement {
                name: "again".into(),
                description: "continue".into(),
                parameters: serde_json::json!({"type":"object","properties":{}}),
            }]
        }
        fn validate_tool_args(&self, _: &str, _: &serde_json::Value) -> Result<(), String> {
            Ok(())
        }
        fn execute_authorized(
            &self,
            _: &str,
            _: &serde_json::Value,
            _: Option<&tetonic_domain::AuthorizedAction>,
            _: &tetonic_domain::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            ToolOutcome::ok("continued", "continued")
        }
    }

    struct CeilingProvider {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        report_tokens: bool,
    }

    #[async_trait]
    impl InferenceProvider for CeilingProvider {
        async fn chat(
            &self,
            _: ChatRequest,
            _: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut usage = tetonic_inference::GenUsage::default();
            if self.report_tokens {
                usage.prompt_tokens = Some(4);
                usage.eval_tokens = Some(1);
            }
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                    function: tetonic_inference::FunctionCall {
                        name: "again".into(),
                        arguments: serde_json::json!({}),
                    },
                }]),
                usage,
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn reported_token_ceiling_stops_the_next_model_call() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let provider = std::sync::Arc::new(CeilingProvider {
            calls: calls.clone(),
            report_tokens: true,
        });
        let agent = Agent::new(
            provider,
            CountingTool,
            AgentConfig {
                reported_token_ceiling: Some(5),
                max_steps: 4,
                ..AgentConfig::default()
            },
        );
        let mut convo = Conversation::new();
        let outcome = agent.turn(&mut convo, test_inv("continue"), |_| {}).await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            matches!(
                outcome,
                CandidateOutcome::Limited {
                    kind: LimitKind::EffortCap,
                    ..
                }
            ),
            "ceiling must stop the loop, got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn unreported_usage_does_not_count_toward_the_token_ceiling() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let provider = std::sync::Arc::new(CeilingProvider {
            calls: calls.clone(),
            report_tokens: false,
        });
        let agent = Agent::new(
            provider,
            CountingTool,
            AgentConfig {
                reported_token_ceiling: Some(1),
                max_steps: 3,
                ..AgentConfig::default()
            },
        );
        let mut convo = Conversation::new();
        let mut invocation = test_inv("continue");
        invocation.max_steps = 3;
        let _ = agent.turn(&mut convo, invocation, |_| {}).await;
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "missing provider usage must not be invented as spend"
        );
    }

    #[tokio::test]
    async fn cancel_during_compaction_does_not_start_the_next_model_call() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let saw_user_turn = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut convo = Conversation::new();
        let cancel = convo.cancel_handle();
        convo.messages.push(Message::system("system"));
        for _ in 0..8 {
            convo
                .messages
                .push(Message::user("WORD ".repeat(800)));
        }
        struct CancelDuringSummary {
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
            saw_user_turn: std::sync::Arc<std::sync::atomic::AtomicBool>,
            cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        #[async_trait]
        impl InferenceProvider for CancelDuringSummary {
            async fn chat(
                &self,
                request: ChatRequest,
                _: &mut TokenSink<'_>,
            ) -> Result<ChatResponse, InferenceError> {
                self.calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if request.messages.iter().any(|message| {
                    message.role == "user" && message.content.contains("FRESHUSER")
                }) {
                    self.saw_user_turn
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                self.cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(ChatResponse {
                    message: Message::assistant("folded"),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
        let provider = std::sync::Arc::new(CancelDuringSummary {
            calls: calls.clone(),
            saw_user_turn: saw_user_turn.clone(),
            cancel,
        });
        let agent = Agent::new(provider, CountingTool, AgentConfig::default());
        let mut invocation = test_inv("FRESHUSER");
        invocation.discipline.compaction_system_prompt = Some("summarize older turns".into());
        let outcome = agent.turn(&mut convo, invocation, |_| {}).await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            !saw_user_turn.load(std::sync::atomic::Ordering::SeqCst),
            "the canceled turn still called the model with the user request"
        );
        assert!(
            matches!(outcome, CandidateOutcome::Canceled { .. }),
            "cancel during compaction must stop the turn, got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn cancel_before_finish_does_not_record_completion() {
        let mut convo = Conversation::new();
        let cancel = convo.cancel_handle();
        struct FinishAfterCancel {
            cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        #[async_trait]
        impl InferenceProvider for FinishAfterCancel {
            async fn chat(
                &self,
                _: ChatRequest,
                _: &mut TokenSink<'_>,
            ) -> Result<ChatResponse, InferenceError> {
                self.cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "SHOULD_NOT_FINISH"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
        let agent = Agent::new(
            std::sync::Arc::new(FinishAfterCancel { cancel }),
            CountingTool,
            AgentConfig {
                max_steps: 2,
                ..AgentConfig::default()
            },
        );
        let outcome = agent.turn(&mut convo, test_inv("finish the task"), |_| {}).await;
        assert!(
            matches!(outcome, CandidateOutcome::Canceled { .. }),
            "finish must not complete a canceled turn, got {outcome:?}"
        );
        let rendered = format!("{outcome:?}");
        assert!(
            !rendered.contains("SHOULD_NOT_FINISH"),
            "canceled turn recorded the finish summary: {rendered}"
        );
    }

    struct MockTestProvider {
        requests: std::sync::Mutex<Vec<ChatRequest>>,
    }

    #[tokio::test]
    async fn stable_catalog_does_not_authorize_inactive_tools_or_spawn() {
        struct AttemptsForbiddenTools(std::sync::atomic::AtomicUsize);
        #[async_trait]
        impl InferenceProvider for AttemptsForbiddenTools {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: chrono::Utc::now(),
                }
            }
            async fn chat(
                &self,
                _: ChatRequest,
                _: &mut TokenSink<'_>,
            ) -> Result<ChatResponse, InferenceError> {
                let calls = if self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    vec![
                        (
                            "write_file",
                            serde_json::json!({"path":"forbidden.txt","content":"no"}),
                        ),
                        (
                            "spawn_agent",
                            serde_json::json!({"role":"coder","task":"write a file"}),
                        ),
                    ]
                } else {
                    vec![(
                        "finish",
                        serde_json::json!({"summary":"Completed with permitted tools only."}),
                    )]
                };
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(
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
        let dir = tempfile::tempdir().unwrap();
        let full = Tools::new(tetonic_tools::Workspace::new(dir.path()).unwrap(), false)
            .with_orchestration(true);
        let catalog = full
            .advertisements()
            .into_iter()
            .map(|t| ToolSchema::function(&t.name, &t.description, t.parameters))
            .collect();
        let restricted = full.with_allowed_tools(["finish".to_string()].into_iter().collect());
        let agent = Agent::new(
            Arc::new(AttemptsForbiddenTools(Default::default())),
            restricted,
            AgentConfig::default(),
        )
        .with_stable_tool_catalog(catalog, "Only finish is permitted this turn.".into())
        .unwrap();
        assert_eq!(agent.advertised_tool_names(), vec!["finish"]);
        let mut invocation = test_inv("Complete without writing or spawning");
        invocation.discipline.spawn_tool = Some("spawn_agent".into());
        let mut denied = Vec::new();
        let result = agent
            .turn(&mut Conversation::new(), invocation, |step| {
                if let Step::ToolResult {
                    name, ok: false, ..
                } = step
                {
                    denied.push(name);
                }
            })
            .await;
        assert!(result.is_completed());
        assert_eq!(denied, vec!["write_file", "spawn_agent"]);
        assert!(!dir.path().join("forbidden.txt").exists());
    }

    #[async_trait]
    impl InferenceProvider for MockTestProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            self.requests.lock().unwrap().push(req);
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![
                    tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "finish".into(),
                            arguments: serde_json::json!({"summary": "done"}),
                        },
                    },
                ]),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_tool_schemas_sorted_deterministically() {
        let temp = tempfile::tempdir().unwrap();
        let ws = tetonic_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let agent = Agent::new(provider, tools, AgentConfig::default());

        for i in 1..agent.schemas.len() {
            assert!(
                agent.schemas[i - 1].function.name <= agent.schemas[i].function.name,
                "schemas must be sorted alphabetically: {} > {}",
                agent.schemas[i - 1].function.name,
                agent.schemas[i].function.name
            );
        }
    }

    #[tokio::test]
    async fn rebind_preserves_agent_and_history_but_changes_next_request() {
        struct Count;
        impl Tokenizer for Count {
            fn count(&self, _: &str) -> usize {
                7
            }
        }
        let old = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let new = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let mut agent = Agent::new(old.clone(), EmptyToolHost, AgentConfig::default());
        agent.config.agent_id = "existing-agent".into();
        agent.config.draft_model = Some("old-draft".into());
        let mut conversation = Conversation::from_audit_messages(vec![
            Message::system("preserve instructions"),
            Message::user("preserve history"),
        ]);
        assert_eq!(conversation.messages[0].cached_tokens(|| 999), 999);
        let initial_model = agent.config.model.clone();
        assert!(agent
            .replace_inference(
                &mut conversation,
                crate::AgentInferenceBinding {
                    provider: new.clone(),
                    model: String::new(),
                    num_ctx: 8192,
                    tokenizer: Box::new(Count),
                }
            )
            .is_err());
        assert_eq!(agent.config.model, initial_model);
        assert_eq!(conversation.messages[0].cached_tokens(|| 7), 999);
        agent
            .replace_inference(
                &mut conversation,
                crate::AgentInferenceBinding {
                    provider: new.clone(),
                    model: "replacement".into(),
                    num_ctx: 8192,
                    tokenizer: Box::new(Count),
                },
            )
            .unwrap();
        assert_eq!(agent.execution_agent_id(), "existing-agent");
        assert_eq!(conversation.messages[0].cached_tokens(|| 7), 7);
        assert_eq!(agent.tools_token_count, 7);
        assert!(agent.config.draft_model.is_none());
        agent
            .turn(
                &mut conversation,
                test_inv_explain("continue", true),
                |_| {},
            )
            .await;
        assert!(old.requests.lock().unwrap().is_empty());
        let requests = new.requests.lock().unwrap();
        assert_eq!(requests[0].model, "replacement");
        assert!(requests[0]
            .messages
            .iter()
            .any(|m| m.content == "preserve history"));
    }

    #[tokio::test]
    async fn test_prefix_kv_cache_invariance() {
        let temp = tempfile::tempdir().unwrap();
        let ws = tetonic_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let agent = Agent::new(provider.clone(), tools, AgentConfig::default());

        let mut convo = Conversation::new();
        agent
            .turn(&mut convo, test_inv("Inspect repository"), |_| {})
            .await;

        let reqs = provider.requests.lock().unwrap();
        assert!(!reqs.is_empty());
        let first_sys = &reqs[0].messages[0];
        let first_user = &reqs[0].messages[1];

        // Ensure system prompt is non-empty and starts with operator card
        assert_eq!(first_sys.role, "system");
        assert_eq!(first_user.role, "user");

        let sys_content = &first_sys.content;
        let user_content = &first_user.content;
        assert!(!sys_content.is_empty());
        assert!(!user_content.is_empty());
    }

    struct MultiToolMockProvider {
        turn: std::sync::atomic::AtomicUsize,
        write_first: bool,
    }

    #[async_trait]
    impl InferenceProvider for MultiToolMockProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if t == 0 {
                let mut calls = Vec::new();
                if self.write_first {
                    calls.push(tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "update".into(),
                            arguments: serde_json::json!({}),
                        },
                    });
                }
                calls.extend(
                    ["a.txt", "b.txt", "c.txt"].map(|path| tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "read_file".into(),
                            arguments: serde_json::json!({"path": path}),
                        },
                    }),
                );
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(calls),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "inspected 3 files"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    struct StreamingPrefetchMockProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl InferenceProvider for StreamingPrefetchMockProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                on_token(
                    "{\"name\": \"read_file\", \"arguments\": {\"path\": \"speculative.txt\"}}",
                );
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({"path": "speculative.txt"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "read speculative"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_multiple_read_only_tool_execution() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.txt"), "hello from a").unwrap();
        std::fs::write(temp.path().join("b.txt"), "hello from b").unwrap();
        std::fs::write(temp.path().join("c.txt"), "hello from c").unwrap();

        let ws = tetonic_tools::Workspace::new(temp.path()).unwrap();
        let root = ws.root().to_path_buf();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(MultiToolMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
            write_first: false,
        });
        let agent = Agent::new(
            provider,
            tools,
            AgentConfig {
                workspace_root: Some(root),
                ..AgentConfig::default()
            },
        )
        .with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);

        let mut convo = Conversation::new();
        agent.turn(&mut convo, test_inv("Read files"), |_| {}).await;

        // Check that messages contains tool outcomes for a, b, c in exact sequence
        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 3);
        assert!(tool_messages[0].content.contains("hello from a"));
        assert!(tool_messages[1].content.contains("hello from b"));
        assert!(tool_messages[2].content.contains("hello from c"));
    }

    #[tokio::test]
    async fn test_streaming_lookahead_prefetch_execution() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("speculative.txt"),
            "speculative lookahead content",
        )
        .unwrap();

        let ws = tetonic_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(StreamingPrefetchMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let config = AgentConfig {
            workspace_root: Some(temp.path().to_path_buf()),
            ..AgentConfig::default()
        };
        let agent =
            Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);

        let mut convo = Conversation::new();
        agent
            .turn(&mut convo, test_inv("Read speculative file"), |_| {})
            .await;

        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 1);
        assert!(tool_messages[0]
            .content
            .contains("speculative lookahead content"));
    }

    struct EscapePrefetchMockProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl InferenceProvider for EscapePrefetchMockProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                on_token("{\"name\": \"read_file\", \"arguments\": {\"path\": \"../secret.txt\"}}");
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({"path": "../secret.txt"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "done"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_read_file_host_byte_leak_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let ws_dir = parent.path().join("ws");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(parent.path().join("secret.txt"), "super secret host bytes").unwrap();
        std::fs::write(ws_dir.join("ok.txt"), "inside").unwrap();

        let ws = tetonic_tools::Workspace::new(&ws_dir).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(EscapePrefetchMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let config = AgentConfig {
            workspace_root: Some(ws_dir.clone()),
            ..AgentConfig::default()
        };
        let agent =
            Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);
        let mut convo = Conversation::new();
        agent
            .turn(&mut convo, test_inv("Read outside file"), |_| {})
            .await;
        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 1);
        assert!(
            !tool_messages[0].content.contains("super secret host bytes"),
            "host bytes leaked: {}",
            tool_messages[0].content
        );
    }

    struct SlicedReadMockProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl InferenceProvider for SlicedReadMockProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({
                                    "path": "large_file.rs",
                                    "start_line": 100,
                                    "end_line": 110
                                }),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({
                                    "summary": "The large file slice has line 100 to 110 explained in detail."
                                }),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_explain_turn_allows_sliced_read_on_large_file() {
        let temp = tempfile::tempdir().unwrap();
        // Create a 50KB file (>16KB MAX_EXPLAIN_BYTES)
        let large_content = (1..=2000)
            .map(|i| format!("line {i}: some code content here"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(large_content.len() > 30 * 1024);
        std::fs::write(temp.path().join("large_file.rs"), &large_content).unwrap();

        let ws = tetonic_tools::Workspace::new(temp.path()).unwrap();
        let root = ws.root().to_path_buf();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(SlicedReadMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let config = AgentConfig {
            explain_turn: true,
            workspace_root: Some(root),
            ..AgentConfig::default()
        };
        let agent =
            Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);

        let mut convo = Conversation::new();
        agent
            .turn(
                &mut convo,
                test_inv_explain("Explain lines 100-110", true),
                |_| {},
            )
            .await;

        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 1);
        assert!(!tool_messages[0].content.contains("too large"));
        assert!(tool_messages[0].content.contains("line 100"));
    }

    struct IssueAllBroker;

    #[async_trait]
    impl tetonic_domain::sinks::ActionBroker for IssueAllBroker {
        async fn evaluate_and_issue(
            &self,
            action: &ProposedAction,
        ) -> Result<tetonic_domain::IssuedCapability, CapabilityError> {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Ok(tetonic_domain::IssuedCapability {
                capability_id: tetonic_domain::CapabilityId::new("cap_m2_verify"),
                session_id: action.session_id.clone(),
                run_id: action.run_id.clone(),
                task_id: action.task_id.clone(),
                attempt_id: action.attempt_id.clone(),
                agent_id: action.agent_id.clone(),
                action_kind: action.kind.clone(),
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: action.workspace_version.clone(),
                data_classification: action.data_class,
                issuance_timestamp: now,
                expiration: now + 300,
                max_use_count: 8,
                current_use_count: 0,
                issuing_policy_version: "v2".into(),
                approval_record_id: None,
                revoked: false,
            })
        }
    }

    #[test]
    fn staged_overlay_is_visible_on_tool_host() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("f.txt"), "live\n").unwrap();
        let ws = tetonic_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let staged = tools.execute(
            "write_file",
            &serde_json::json!({ "path": "f.txt", "content": "staged\n" }),
        );
        assert!(staged.ok, "stage write: {}", staged.content);
        let overlay = tools
            .verification_overlay_if_staged()
            .expect("overlay query")
            .expect("staged overlay path");
        assert!(
            overlay.display().to_string().contains("verify_overlay"),
            "staged verify must expose overlay path; {overlay:?}"
        );
    }

    #[test]
    fn execute_gated_does_not_call_check_tool() {
        let src = include_str!("agent.rs");
        let mut in_block = false;
        let mut production = String::new();
        for line in src.lines() {
            let trim = line.trim();
            if trim.starts_with("#[cfg(test)]") {
                in_block = true;
            }
            if in_block {
                continue;
            }
            if trim.starts_with("//") {
                continue;
            }
            production.push_str(line);
            production.push('\n');
        }
        let n = production.matches(".check_tool(").count();
        assert_eq!(n, 0, "execute_gated must not call check_tool (found {n})");
    }

    #[test]
    fn agent_tools_field_is_dyn_tool_host_not_concrete_tools() {
        let src = include_str!("agent.rs");
        let mut in_block = false;
        let mut production = String::new();
        for line in src.lines() {
            let trim = line.trim();
            if trim.starts_with("#[cfg(test)]") {
                in_block = true;
            }
            if in_block {
                continue;
            }
            production.push_str(line);
            production.push('\n');
        }
        assert!(
            production.contains("tools: Box<dyn ToolHost>"),
            "Agent must store dyn ToolHost"
        );
        assert!(
            !production.contains("tools: Tools")
                && !production.contains("tools: tetonic_tools::Tools"),
            "Agent must not store concrete tetonic_tools::Tools"
        );
        assert!(
            production.contains("context_compiler: Option<Arc<dyn ContextCompiler>>"),
            "Agent must store dyn ContextCompiler"
        );
    }

    struct DummyHost;

    #[derive(Clone)]
    struct OrderedHost(Arc<std::sync::Mutex<Vec<String>>>);

    impl ToolHost for OrderedHost {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(self.clone())
        }
        fn propose(&self, _name: &str, _args: &Value) -> Option<ToolProposal> {
            None
        }
        fn is_tool_allowed(&self, _name: &str) -> bool {
            true
        }
        fn is_read_only(&self, name: &str) -> bool {
            name != "update"
        }
        fn advertisements(&self) -> Vec<ToolAdvertisement> {
            vec![]
        }
        fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
            Ok(())
        }
        fn execute_authorized(
            &self,
            name: &str,
            _args: &Value,
            _auth: Option<&AuthorizedAction>,
            _cancel: &tetonic_domain::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            let mut calls = self.0.lock().unwrap();
            if name == "read_file" {
                assert_eq!(
                    calls.first().map(String::as_str),
                    Some("update"),
                    "read ran before mutation"
                );
            }
            calls.push(name.into());
            ToolOutcome::ok("ok", "ok")
        }
    }

    #[tokio::test]
    async fn mixed_tool_batch_preserves_mutation_before_reads() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agent = Agent::new(
            Arc::new(MultiToolMockProvider {
                turn: std::sync::atomic::AtomicUsize::new(0),
                write_first: true,
            }),
            OrderedHost(calls.clone()),
            AgentConfig::default(),
        );
        let outcome = agent
            .turn(
                &mut Conversation::new(),
                test_inv("update and inspect"),
                |_| {},
            )
            .await;
        assert!(matches!(outcome, CandidateOutcome::Completed { .. }));
        assert_eq!(
            *calls.lock().unwrap(),
            ["update", "read_file", "read_file", "read_file"]
        );
    }

    #[tokio::test]
    async fn batched_reads_obey_discipline_before_execution() {
        let calls = Arc::new(std::sync::Mutex::new(vec!["update".to_owned()]));
        let agent = Agent::new(
            Arc::new(MultiToolMockProvider {
                turn: std::sync::atomic::AtomicUsize::new(0),
                write_first: false,
            }),
            OrderedHost(calls.clone()),
            AgentConfig {
                workspace_root: Some(std::env::temp_dir()),
                ..Default::default()
            },
        )
        .with_resolve_under_root(Arc::new(|_, _| Ok(100)));
        let mut invocation = test_inv_explain("inspect", true);
        invocation.discipline.whole_file_tools = vec!["read_file".into()];
        invocation.discipline.limits.max_whole_file_bytes = Some(1);
        agent
            .turn(&mut Conversation::new(), invocation, |_| {})
            .await;
        assert_eq!(
            *calls.lock().unwrap(),
            ["update"],
            "rejected reads must never execute"
        );
    }

    impl Clone for DummyHost {
        fn clone(&self) -> Self {
            DummyHost
        }
    }

    impl ToolHost for DummyHost {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(self.clone())
        }
        fn propose(&self, name: &str, _args: &Value) -> Option<ToolProposal> {
            match name {
                "read_file" => Some(ToolProposal {
                    kind: ActionKind::ReadFile,
                    resolved_path: None,
                }),
                _ => None,
            }
        }
        fn is_tool_allowed(&self, _name: &str) -> bool {
            true
        }
        fn is_read_only(&self, _name: &str) -> bool {
            true
        }
        fn advertisements(&self) -> Vec<ToolAdvertisement> {
            vec![]
        }
        fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
            Ok(())
        }
        fn execute_authorized(
            &self,
            _name: &str,
            _args: &Value,
            _auth: Option<&AuthorizedAction>,
            _cancel: &tetonic_domain::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            ToolOutcome::ok("ok", "ok")
        }
    }

    #[test]
    fn agent_constructs_with_dummy_tool_host_not_tools() {
        let provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let _agent = Agent::new(provider, DummyHost, AgentConfig::default());
    }

    fn boom_capture() -> CaptureWorkspaceVersion {
        Arc::new(|_, _| Err("boom".into()))
    }

    struct CaptureErrToolProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl InferenceProvider for CaptureErrToolProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({"path": "a.txt"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "done"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn sub03_capture_hook_err_is_per_site() {
        let config = AgentConfig {
            workspace_root: Some(std::path::PathBuf::from("/tmp/sub03-capture-err")),
            ..AgentConfig::default()
        };

        let finish_provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let compiled_agent = Agent::new(finish_provider, DummyHost, config.clone())
            .with_capture_workspace_version(boom_capture());
        let mut convo = Conversation::new();
        let compiled = compiled_agent
            .turn(&mut convo, test_inv("no tools"), |_| {})
            .await;
        assert!(
            !matches!(compiled, CandidateOutcome::Failed { .. }),
            "compiled capture Err without compiler must not Failed: {compiled:?}"
        );

        let tool_provider = Arc::new(CaptureErrToolProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let brokered = Agent::new(tool_provider, DummyHost, config)
            .with_action_broker(Arc::new(IssueAllBroker))
            .with_capture_workspace_version(boom_capture());
        let mut convo = Conversation::new();
        brokered
            .turn(&mut convo, test_inv("Read a file"), |_| {})
            .await;
        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(
            tool_messages.len(),
            1,
            "brokered tool must produce one outcome"
        );
        assert!(
            tool_messages[0].content.contains("workspace version"),
            "broker present+Err must deny with workspace version: {}",
            tool_messages[0].content
        );
    }

    struct MockEmptyToolProvider;

    #[async_trait]
    impl InferenceProvider for MockEmptyToolProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            Ok(ChatResponse {
                message: Message::assistant("I am providing an answer without calling any tools."),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_empty_tools_limited_invokes_abort_staged_mutations() {
        let aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let aborted_clone = aborted.clone();
        let abort_hook: AbortStaged = Arc::new(move || {
            aborted_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let provider = Arc::new(MockEmptyToolProvider);
        let agent =
            Agent::new(provider, DummyHost, AgentConfig::default()).with_abort_staged(abort_hook);

        let mut convo = Conversation::new();
        let outcome = agent.turn(&mut convo, test_inv("hello"), |_| {}).await;

        assert!(matches!(
            outcome,
            CandidateOutcome::Limited {
                kind: LimitKind::EmptyTools,
                ..
            }
        ));
        assert!(
            aborted.load(std::sync::atomic::Ordering::SeqCst),
            "abort_staged hook must be invoked when turn finishes with LimitKind::EmptyTools"
        );
    }

    struct MockContinuousBrain;

    #[async_trait]
    impl tetonic_domain::Brain for MockContinuousBrain {
        async fn complete(
            &self,
            _req: tetonic_domain::BrainRequest,
            _on_token: &mut tetonic_domain::BrainTokenSink<'_>,
        ) -> Result<tetonic_domain::BrainResponse, tetonic_domain::BrainError> {
            unimplemented!()
        }

        async fn perceive(
            &self,
            perception: tetonic_domain::Perception,
        ) -> Result<Option<tetonic_domain::WorldAction>, tetonic_domain::BrainError> {
            if perception.urgency == tetonic_domain::Urgency::High {
                Ok(Some(tetonic_domain::WorldAction::bare(
                    "emergency_action",
                    tetonic_domain::BrainPathway::Reflexive {
                        model: "mock".into(),
                    },
                )))
            } else {
                Ok(None)
            }
        }

        fn describe(&self) -> &str {
            "mock:continuous"
        }

        fn last_cost(&self) -> tetonic_domain::BrainCost {
            Default::default()
        }
    }

    #[tokio::test]
    async fn test_run_continuous_executes_perceptions_and_emits_actions() {
        let brain = Arc::new(MockContinuousBrain);
        let agent = Agent::default().with_brain(brain);

        let (perception_tx, perception_rx) = tokio::sync::mpsc::channel(10);
        let (action_tx, mut action_rx) = tokio::sync::mpsc::channel(10);

        let agent_handle = tokio::spawn(async move {
            agent.run_continuous(perception_rx, action_tx).await
        });

        perception_tx
            .send(tetonic_domain::Perception {
                when: chrono::Utc::now(),
                sequence: 1,
                urgency: tetonic_domain::Urgency::Low,
                signals: vec![],
                events: vec![],
                state: tetonic_domain::WorldState {
                    schema_id: "test".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        perception_tx
            .send(tetonic_domain::Perception {
                when: chrono::Utc::now(),
                sequence: 2,
                urgency: tetonic_domain::Urgency::High,
                signals: vec![],
                events: vec![],
                state: tetonic_domain::WorldState {
                    schema_id: "test".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        let action = action_rx.recv().await.expect("action emitted");
        assert_eq!(action.kind, "emergency_action");

        drop(perception_tx);
        let result = agent_handle.await.unwrap();
        assert!(result.is_ok());
    }

    struct MockWorldAdapter {
        manifest: tetonic_domain::WorldManifest,
        estop: tetonic_domain::EstopSwitch,
        executed_actions: std::sync::Mutex<Vec<tetonic_domain::WorldAction>>,
        perception_rx:
            std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<tetonic_domain::Perception>>>,
    }

    impl MockWorldAdapter {
        fn new(
            manifest: tetonic_domain::WorldManifest,
        ) -> (
            Arc<Self>,
            tokio::sync::mpsc::Sender<tetonic_domain::Perception>,
        ) {
            let (tx, rx) = tokio::sync::mpsc::channel(10);
            let adapter = Arc::new(Self {
                manifest,
                estop: tetonic_domain::EstopSwitch::new(),
                executed_actions: std::sync::Mutex::new(Vec::new()),
                perception_rx: std::sync::Mutex::new(Some(rx)),
            });
            (adapter, tx)
        }
    }

    #[async_trait::async_trait]
    impl tetonic_domain::WorldAdapter for MockWorldAdapter {
        fn open(&self) -> (tetonic_domain::PerceptionSender, tetonic_domain::PerceptionReceiver) {
            let rx = self.perception_rx.lock().unwrap().take().expect("open once");
            let (tx, _) = tokio::sync::mpsc::channel(1);
            (tx, rx)
        }

        async fn execute(
            &self,
            action: tetonic_domain::WorldAction,
        ) -> Result<tetonic_domain::ActionResult, tetonic_domain::WorldError> {
            self.estop.check(&action.kind)?;
            self.executed_actions.lock().unwrap().push(action);
            Ok(tetonic_domain::ActionResult {
                success: true,
                feedback: Some("executed".into()),
                state_changed: true,
            })
        }

        fn describe(&self) -> &str {
            "mock:world"
        }

        fn manifest(&self) -> tetonic_domain::WorldManifest {
            self.manifest.clone()
        }

        fn trigger_estop(&self, reason: String) -> Result<(), tetonic_domain::WorldError> {
            self.estop.trigger(reason);
            Ok(())
        }

        fn resume(&self) -> Result<(), tetonic_domain::WorldError> {
            self.estop.resume();
            Ok(())
        }

        fn is_estopped(&self) -> bool {
            self.estop.is_estopped()
        }
    }

    #[tokio::test]
    async fn test_run_in_world_executes_actions_when_manifest_valid() {
        let brain = Arc::new(MockContinuousBrain);
        let agent = Agent::default().with_brain(brain);

        let manifest = tetonic_domain::WorldManifest::new("mock_world", "1.0")
            .with_affordance(tetonic_domain::Affordance::instant(
                "emergency_action",
                "Execute emergency response",
            ));

        let (adapter, perception_tx) = MockWorldAdapter::new(manifest);
        let adapter_clone = adapter.clone();

        let agent_handle = tokio::spawn(async move {
            agent.run_in_world(adapter_clone).await
        });

        perception_tx
            .send(tetonic_domain::Perception {
                when: chrono::Utc::now(),
                sequence: 1,
                urgency: tetonic_domain::Urgency::High,
                signals: vec![],
                events: vec![],
                state: tetonic_domain::WorldState {
                    schema_id: "test".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        // Give the actor loop a tick to process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(perception_tx);
        let _ = agent_handle.await.unwrap();

        let executed = adapter.executed_actions.lock().unwrap();
        assert_eq!(executed.len(), 1);
        assert_eq!(executed[0].kind, "emergency_action");
    }

    #[tokio::test]
    async fn test_run_in_world_rejects_unmanifested_actions() {
        let brain = Arc::new(MockContinuousBrain);
        let agent = Agent::default().with_brain(brain);

        // Manifest does NOT include "emergency_action"
        let manifest = tetonic_domain::WorldManifest::new("mock_world", "1.0")
            .with_affordance(tetonic_domain::Affordance::instant(
                "other_action",
                "Other action",
            ));

        let (adapter, perception_tx) = MockWorldAdapter::new(manifest);
        let adapter_clone = adapter.clone();

        let agent_handle = tokio::spawn(async move {
            agent.run_in_world(adapter_clone).await
        });

        perception_tx
            .send(tetonic_domain::Perception {
                when: chrono::Utc::now(),
                sequence: 1,
                urgency: tetonic_domain::Urgency::High,
                signals: vec![],
                events: vec![],
                state: tetonic_domain::WorldState {
                    schema_id: "test".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(perception_tx);
        let _ = agent_handle.await.unwrap();

        let executed = adapter.executed_actions.lock().unwrap();
        assert_eq!(executed.len(), 0); // Action was rejected by manifest!
    }

    #[tokio::test]
    async fn test_run_in_world_idle_cancel_does_not_dispatch() {
        let brain = Arc::new(MockContinuousBrain);
        let scope = tetonic_domain::work_scope::WorkScope::default();
        let mut agent = Agent::default().with_brain(brain);
        agent.bind_work_scope(scope.clone()).unwrap();
        let manifest = tetonic_domain::WorldManifest::new("mock_world", "1.0")
            .with_affordance(tetonic_domain::Affordance::instant(
                "emergency_action",
                "Execute emergency response",
            ));
        let (adapter, _perception_tx) = MockWorldAdapter::new(manifest);
        let adapter_clone = adapter.clone();
        let agent_handle = tokio::spawn(async move { agent.run_in_world(adapter_clone).await });
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        scope.cancel();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(1), agent_handle)
            .await
            .expect("idle world wait must observe cancellation")
            .unwrap()
            .unwrap();
        assert!(matches!(outcome, CandidateOutcome::Canceled { .. }));
        assert!(adapter.executed_actions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_run_in_world_denied_authority_does_not_reach_adapter() {
        struct DenyGate;
        #[async_trait::async_trait]
        impl crate::ExecutionGate for DenyGate {
            async fn authorize(&self) -> Result<(), ()> {
                Err(())
            }
        }
        let brain = Arc::new(MockContinuousBrain);
        let mut agent = Agent::default().with_brain(brain);
        agent.bind_execution_gate(Some(Arc::new(DenyGate)));
        let manifest = tetonic_domain::WorldManifest::new("mock_world", "1.0")
            .with_affordance(tetonic_domain::Affordance::instant(
                "emergency_action",
                "Execute emergency response",
            ));
        let (adapter, perception_tx) = MockWorldAdapter::new(manifest);
        let adapter_clone = adapter.clone();
        let agent_handle = tokio::spawn(async move { agent.run_in_world(adapter_clone).await });
        perception_tx
            .send(tetonic_domain::Perception {
                when: chrono::Utc::now(),
                sequence: 1,
                urgency: tetonic_domain::Urgency::High,
                signals: vec![],
                events: vec![],
                state: tetonic_domain::WorldState {
                    schema_id: "test".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(perception_tx);
        agent_handle.await.unwrap().unwrap();
        assert!(adapter.executed_actions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_run_in_world_enforces_estop_interlock_and_aborts_mutations() {
        let brain = Arc::new(MockContinuousBrain);
        let aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let aborted_clone = aborted.clone();

        let agent = Agent::default()
            .with_brain(brain)
            .with_abort_staged(Arc::new(move || {
                aborted_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            }));

        let manifest = tetonic_domain::WorldManifest::new("mock_world", "1.0")
            .with_affordance(tetonic_domain::Affordance::instant(
                "emergency_action",
                "Execute emergency response",
            ));

        let (adapter, perception_tx) = MockWorldAdapter::new(manifest);
        // Authoritatively engage E-Stop
        adapter.trigger_estop("Safety containment breach".into()).unwrap();

        let adapter_clone = adapter.clone();
        let agent_handle = tokio::spawn(async move {
            agent.run_in_world(adapter_clone).await
        });

        perception_tx
            .send(tetonic_domain::Perception {
                when: chrono::Utc::now(),
                sequence: 1,
                urgency: tetonic_domain::Urgency::High,
                signals: vec![],
                events: vec![],
                state: tetonic_domain::WorldState {
                    schema_id: "test".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(perception_tx);
        let _ = agent_handle.await.unwrap();

        // 1. Staged mutations must be aborted
        assert!(aborted.load(std::sync::atomic::Ordering::SeqCst));
        // 2. Action must NOT have executed
        let executed = adapter.executed_actions.lock().unwrap();
        assert_eq!(executed.len(), 0);
    }
