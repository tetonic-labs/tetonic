use super::*;
use tetonic_memory::{
    BlackboardAccess, BlackboardCommand, BlackboardEmoji, BlackboardKind, CapabilityScope,
    SaveCapabilityPolicy,
};
use tetonic_policy::capabilities::{CapabilityPolicy, CommunicationScope};

fn invoke_for_work(
    db: &tetonic_memory::Store,
    work: &str,
    actor: &str,
    call: &str,
    command: BlackboardCommand,
    stale_lease: bool,
) -> tetonic_memory::Result<Value> {
    let work_item = db.get_team_work_item(ORG, TEAM, work)?.unwrap();
    let attempt = work_item.attempt_id.unwrap();
    let run = db.load_run_snapshot(&work_item.run_id.unwrap())?.unwrap();
    let lease = run.attempts[&tetonic_domain::AttemptId::new(&attempt)]
        .lease
        .as_ref()
        .unwrap();
    let proof = tetonic_domain::LeaseProof {
        lease_id: lease.lease_id.clone(),
        lease_epoch: lease.lease_epoch + u64::from(stale_lease),
        holder: lease.holder.clone(),
    };
    db.use_blackboard(
        BlackboardAccess {
            actor,
            org: ORG,
            team: TEAM,
            work,
            attempt: &attempt,
            call_id: call,
            now: chrono::Utc::now().timestamp() as u64,
            lease: &proof,
        },
        command,
    )
}

pub(super) fn reply(scenario: u8, child: bool, request: &Value) -> (&'static str, Value) {
    let messages = request["messages"].as_array().unwrap();
    let outputs = messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .collect::<Vec<_>>();
    if !child {
        return if outputs.is_empty() {
            (
                DISPATCH,
                json!({"assignment_keys":if scenario==16 {vec!["compare","check","wrap"]} else {vec!["compare","check"]}}),
            )
        } else {
            (
                "finish",
                json!({"summary":"Both independent contributions completed."}),
            )
        };
    }
    let compare = messages.iter().any(|m| {
        m["role"] == "user"
            && m["content"]
                .as_str()
                .unwrap_or_default()
                .contains("COMPARE_CANARY")
    });
    let payload_at = |index: usize| {
        let value = outputs[index]["content"].as_str().unwrap();
        let start = value.find('{').expect(value);
        serde_json::from_str::<Value>(&value[start..]).expect(value)
    };
    let payload = || payload_at(outputs.len() - 1);
    if messages.iter().any(|m| {
        m["role"] == "user"
            && m["content"]
                .as_str()
                .unwrap_or_default()
                .contains("WRAP_CANARY")
    }) {
        return match outputs.len() {
            0 => ("blackboard", json!({"action":"read"})),
            1 => {
                let board = payload();
                assert_eq!(
                    board["threads"].as_array().unwrap().len(),
                    1,
                    "same identity must retain its audience membership in a new assignment"
                );
                (
                    "blackboard",
                    json!({"action":"reply","thread_id":board["threads"][0]["id"],"body":"FOLLOW_ON_CANARY: Used the shared review in the recommendation."}),
                )
            }
            _ => (
                "finish",
                json!({"summary":"Follow-on work reused the collaboration thread."}),
            ),
        };
    }
    match (compare, outputs.len()) {
        (_, 0) => ("blackboard", json!({"action":"peers"})),
        (true, 1) => (
            "blackboard",
            json!({"action":"post","audience":[payload()["peers"][0]["agent_id"]],"title":"Attendance assumption","kind":"question","body":"SHARED_CANARY: Check the attendance assumption while I finish the comparison."}),
        ),
        (false, 1) => ("blackboard", json!({"action":"read"})),
        (false, 2) => {
            let result = payload();
            assert!(result["threads"][0]["messages"][0]["body"]
                .as_str()
                .unwrap()
                .contains("SHARED_CANARY"));
            (
                "blackboard",
                json!({"action":"react","thread_id":result["threads"][0]["id"],"message_id":result["threads"][0]["messages"][0]["id"],"emoji":"👀","present":true}),
            )
        }
        (false, 3) => {
            assert_eq!(payload()["recorded"], true);
            assert_eq!(payload()["waiting"], false);
            let board = payload_at(1);
            (
                "blackboard",
                json!({"action":"reply","thread_id":board["threads"][0]["id"],"reply_to":board["threads"][0]["messages"][0]["id"],"body":"REPLY_CANARY: Flagged attendance as an assumption in the review."}),
            )
        }
        _ => (
            "finish",
            json!({"summary":"Independent contribution finished; board update does not block work."}),
        ),
    }
}

async fn seed_board(workspace: &LocalWorkspace, follow_on: bool) -> String {
    let source = seed_roster_options(workspace, follow_on, true, true).await;
    for agent in workspace
        .services
        .agents()
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.editable && a.key != shaping::GUIDE)
    {
        workspace
            .update_agent(crate::workspace::UpdateLocalAgent {
                agent_key: agent.key,
                expected_definition_digest: agent.definition_digest,
                configuration: CreateLocalAgent {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: agent.name,
                    purpose: agent.purpose,
                    model: agent.model,
                    provider: agent.provider,
                    harness: "general".into(),
                    max_steps: 8,
                    max_seconds: 120,
                    max_tokens: 4096,
                    tools: Some(vec!["blackboard".into()]),
                    workspace_root: None,
                    expected_workspace_root: None,
                    hosted_consent: false,
                    hosted_tools_consent: false,
                },
            })
            .await
            .unwrap();
    }
    source
}

#[tokio::test]
async fn blackboard_agents_exchange_real_threads_without_waiting_or_private_transcript_sharing() {
    check_exchange(14).await;
}

#[tokio::test]
async fn blackboard_identity_keeps_access_across_assignments_in_the_same_effort() {
    check_exchange(16).await;
}

async fn check_exchange(scenario: u8) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    let (url, calls, server) = scripted_server(scenario).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(path.clone(), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_board(&workspace, scenario == 16).await;
            workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let result = groups::terminal(&workspace, &source).await;
            assert_eq!(
                result.state,
                "completed",
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            assert!(result
                .receipt
                .content
                .assignments
                .iter()
                .take(2)
                .all(|a| a.depends_on.is_empty()));
            let board = workspace
                .blackboard(BlackboardQuery::default())
                .await
                .unwrap();
            assert_eq!(board.threads.len(), 1);
            let reply_count = if scenario == 16 { 2 } else { 1 };
            assert_eq!(board.threads[0].reply_count, reply_count);
            assert_eq!(
                board.threads[0].messages.len(),
                1,
                "summaries do not dump replies"
            );
            let query = BlackboardQuery {
                thread_id: Some(board.threads[0].id.clone()),
                ..Default::default()
            };
            let detail = workspace.blackboard(query.clone()).await.unwrap();
            assert_eq!(detail.threads[0].messages.len(), reply_count + 1);
            assert!(detail.threads[0].messages[1].body.contains("REPLY_CANARY"));
            assert_ne!(
                detail.threads[0].messages[0].author.agent_id,
                detail.threads[0].messages[1].author.agent_id
            );
            let reaction = &detail.threads[0].messages[0].reactions[0];
            assert_eq!(reaction.emoji, BlackboardEmoji::Looking);
            assert_eq!(
                reaction.agents,
                vec![detail.threads[0].messages[1].author.clone()]
            );
            assert_eq!(
                board.threads[0].messages[0].reactions,
                detail.threads[0].messages[0].reactions
            );
            assert!(!serde_json::to_string(&detail)
                .unwrap()
                .contains("PRIVATE_CANARY"));
            assert!(!calls
                .lock()
                .unwrap()
                .iter()
                .any(|r| r["messages"].to_string().contains("PRIVATE_CANARY")));
            let reopened = tetonic_memory::Store::open(&path).unwrap();
            assert_eq!(
                reopened
                    .inspect_blackboard(OWNER, ORG, TEAM, &query)
                    .unwrap()
                    .threads,
                detail.threads
            );
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn blackboard_permissions_fence_audiences_retries_revocation_and_foreign_work() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(15).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("board.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_board(&workspace, false).await;
            let start = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while calls.lock().unwrap().len() < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let pins = start.receipt.assignments.clone();
            workspace
                .services
                .local
                .store()
                .write(move |db| {
                    let identities = pins
                        .iter()
                        .map(|p| {
                            db.get_organization_agent_revision(
                                OWNER,
                                ORG,
                                &p.agent_key,
                                &p.definition_digest,
                            )
                            .unwrap()
                            .unwrap()
                            .identity
                            .identity_id
                        })
                        .collect::<Vec<_>>();
                    let invoke =
                        |index: usize, actor: &str, call: &str, command: BlackboardCommand| {
                            invoke_for_work(db, &pins[index].work_id, actor, call, command, false)
                        };
                    assert!(invoke_for_work(
                        db,
                        &pins[0].work_id,
                        OWNER,
                        "stale",
                        BlackboardCommand::Peers,
                        true
                    )
                    .is_err());
                    let post = BlackboardCommand::Post {
                        audience: vec![identities[1].clone()],
                        title: "Check assumption".into(),
                        kind: BlackboardKind::Question,
                        body: "Deliberate shared information".into(),
                    };
                    let first = invoke(0, OWNER, "post", post.clone()).unwrap();
                    assert_eq!(first["waiting"], false);
                    assert_eq!(
                        invoke(0, OWNER, "post", post.clone()).unwrap(),
                        first,
                        "same call is idempotent"
                    );
                    let thread = first["thread_id"].as_str().unwrap().to_owned();
                    let mut different = post.clone();
                    if let BlackboardCommand::Post { body, .. } = &mut different {
                        *body = "Changed body".into();
                    }
                    assert!(
                        invoke(0, OWNER, "post", different).is_err(),
                        "a reused call id cannot mutate its receipt"
                    );
                    assert!(invoke(
                        1,
                        OWNER,
                        "resolve-other",
                        BlackboardCommand::Resolve {
                            thread_id: thread.clone()
                        }
                    )
                    .is_err());
                    assert!(invoke(
                        1,
                        OWNER,
                        "wrong-parent",
                        BlackboardCommand::Reply {
                            thread_id: thread.clone(),
                            body: "Wrong parent".into(),
                            reply_to: Some("foreign-message".into())
                        }
                    )
                    .is_err());
                    for i in 0..22 {
                        invoke(0, OWNER, &format!("topic-{i}"), post.clone()).unwrap();
                    }
                    let first_page = invoke(
                        1,
                        OWNER,
                        "page",
                        BlackboardCommand::Read {
                            thread_id: None,
                            offset: 0,
                        },
                    )
                    .unwrap();
                    let last_page = invoke(
                        1,
                        OWNER,
                        "page",
                        BlackboardCommand::Read {
                            thread_id: None,
                            offset: 20,
                        },
                    )
                    .unwrap();
                    assert_eq!(first_page["threads"].as_array().unwrap().len(), 20);
                    assert_eq!(first_page["has_more"], true);
                    assert_eq!(last_page["threads"].as_array().unwrap().len(), 3);
                    assert_eq!(last_page["has_more"], false);
                    let ids = first_page["threads"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .chain(last_page["threads"].as_array().unwrap())
                        .map(|t| t["id"].as_str().unwrap())
                        .collect::<std::collections::HashSet<_>>();
                    assert_eq!(ids.len(), 23);
                    let scope = BlackboardQuery {
                        work_ids: Some(vec![pins[0].work_id.clone()]),
                        ..Default::default()
                    };
                    assert_eq!(
                        db.inspect_blackboard(OWNER, ORG, TEAM, &scope)
                            .unwrap()
                            .threads
                            .len(),
                        20
                    );
                    let empty = BlackboardQuery {
                        work_ids: Some(vec![]),
                        ..Default::default()
                    };
                    assert!(db
                        .inspect_blackboard(OWNER, ORG, TEAM, &empty)
                        .unwrap()
                        .threads
                        .is_empty());
                    let other = BlackboardQuery {
                        work_ids: Some(vec!["other-work".into()]),
                        ..Default::default()
                    };
                    assert!(db
                        .inspect_blackboard(OWNER, ORG, TEAM, &other)
                        .unwrap()
                        .threads
                        .is_empty());
                    assert!(invoke(0, "outsider", "post", post.clone()).is_err());
                    assert!(invoke(
                        0,
                        OWNER,
                        "foreign",
                        BlackboardCommand::Post {
                            audience: vec!["foreign-agent".into()],
                            title: "Foreign".into(),
                            kind: BlackboardKind::Question,
                            body: "Must not share".into()
                        }
                    )
                    .is_err());
                    assert!(invoke(
                        1,
                        OWNER,
                        "bad-thread",
                        BlackboardCommand::Reply {
                            thread_id: "foreign-thread".into(),
                            body: "Leak".into(),
                            reply_to: None
                        }
                    )
                    .is_err());
                    let read = BlackboardCommand::Read {
                        thread_id: Some(thread.clone()),
                        offset: 0,
                    };
                    let before = invoke(1, OWNER, "read", read.clone()).unwrap();
                    let react = BlackboardCommand::React {
                        thread_id: thread.clone(),
                        message_id: before["threads"][0]["messages"][0]["id"]
                            .as_str()
                            .unwrap()
                            .into(),
                        emoji: BlackboardEmoji::Like,
                        present: true,
                    };
                    let receipt = invoke(1, OWNER, "react", react.clone()).unwrap();
                    assert_eq!(receipt["waiting"], false);
                    assert_eq!(invoke(1, OWNER, "react", react.clone()).unwrap(), receipt);
                    invoke(1, OWNER, "repeat-reaction", react.clone()).unwrap();
                    let once = invoke(1, OWNER, "read", read.clone()).unwrap();
                    assert_eq!(
                        once["threads"][0]["messages"][0]["reactions"][0]["agents"]
                            .as_array()
                            .unwrap()
                            .len(),
                        1,
                        "the same identity cannot inflate a reaction count with another call"
                    );
                    assert_eq!(
                        once["threads"][0]["updated_at"], before["threads"][0]["updated_at"],
                        "reactions do not reorder conversations"
                    );
                    invoke(0, OWNER, "also-like", react.clone()).unwrap();
                    let mut remove = react.clone();
                    if let BlackboardCommand::React { present, .. } = &mut remove {
                        *present = false;
                    }
                    assert!(
                        invoke(1, OWNER, "react", remove.clone()).is_err(),
                        "a call id cannot change its intent"
                    );
                    invoke(1, OWNER, "remove-own", remove.clone()).unwrap();
                    let remaining = invoke(1, OWNER, "read", read.clone()).unwrap();
                    let agents = remaining["threads"][0]["messages"][0]["reactions"][0]["agents"]
                        .as_array()
                        .unwrap();
                    assert_eq!(agents.len(), 1);
                    assert_eq!(
                        agents[0]["agent_id"], identities[0],
                        "removal preserves someone else's reaction"
                    );
                    invoke(1, OWNER, "remove-again", remove).unwrap();
                    invoke(1, OWNER, "add-again", react.clone()).unwrap();
                    let mut bad_message = react.clone();
                    if let BlackboardCommand::React { message_id, .. } = &mut bad_message {
                        *message_id = "other-thread-message".into();
                    }
                    assert!(invoke(1, OWNER, "bad-message", bad_message).is_err());
                    assert!(invoke_for_work(
                        db,
                        &pins[1].work_id,
                        OWNER,
                        "stale-react",
                        react.clone(),
                        true
                    )
                    .is_err());
                    assert!(invoke(1, "outsider", "react", react.clone()).is_err());
                    assert_eq!(
                        invoke(1, OWNER, "read", read.clone()).unwrap()["threads"]
                            .as_array()
                            .unwrap()
                            .len(),
                        1
                    );
                    invoke(
                        0,
                        OWNER,
                        "resolve",
                        BlackboardCommand::Resolve {
                            thread_id: thread.clone(),
                        },
                    )
                    .unwrap();
                    let mut acknowledge = react.clone();
                    if let BlackboardCommand::React { emoji, .. } = &mut acknowledge {
                        *emoji = BlackboardEmoji::Acknowledge;
                    }
                    invoke(1, OWNER, "acknowledge-resolved", acknowledge).unwrap();
                    assert!(invoke(
                        1,
                        OWNER,
                        "reply-resolved",
                        BlackboardCommand::Reply {
                            thread_id: thread.clone(),
                            body: "Late reply".into(),
                            reply_to: None
                        }
                    )
                    .is_err());
                    let restrict = |scope, id: String, revision, communication| {
                        db.save_capability_policy(
                            OWNER,
                            ORG,
                            TEAM,
                            &SaveCapabilityPolicy {
                                scope,
                                scope_id: id,
                                expected_revision: revision,
                                request_id: uuid::Uuid::new_v4().to_string(),
                                policy: Some(CapabilityPolicy {
                                    communication: Some(communication),
                                    ..Default::default()
                                }),
                            },
                        )
                        .unwrap()
                    };
                    restrict(
                        CapabilityScope::Agent,
                        identities[1].clone(),
                        0,
                        CommunicationScope::Blocked,
                    );
                    assert!(
                        invoke(1, OWNER, "react", react.clone()).is_err(),
                        "revocation fences reaction retries too"
                    );
                    assert!(invoke(1, OWNER, "react-after-revoke", react.clone()).is_err());
                    assert!(
                        invoke(0, OWNER, "post", post.clone()).is_err(),
                        "recipient revocation applies even to retry"
                    );
                    assert!(invoke(0, OWNER, "read", read.clone()).unwrap()["threads"]
                        .as_array()
                        .unwrap()
                        .is_empty());
                    assert!(invoke(
                        1,
                        OWNER,
                        "reply",
                        BlackboardCommand::Reply {
                            thread_id: thread.clone(),
                            body: "Reply".into(),
                            reply_to: None
                        }
                    )
                    .is_err());
                    restrict(
                        CapabilityScope::Agent,
                        identities[1].clone(),
                        1,
                        CommunicationScope::SelectedAgents {
                            agent_ids: vec![identities[0].clone()],
                        },
                    );
                    assert_eq!(
                        invoke(1, OWNER, "read", read.clone()).unwrap()["threads"]
                            .as_array()
                            .unwrap()
                            .len(),
                        1
                    );
                    restrict(
                        CapabilityScope::Workspace,
                        String::new(),
                        0,
                        CommunicationScope::Blocked,
                    );
                    assert!(
                        invoke(0, OWNER, "blocked", post.clone()).is_err(),
                        "agent allow cannot override workspace deny"
                    );
                    let hidden = invoke(
                        0,
                        OWNER,
                        "page",
                        BlackboardCommand::Read {
                            thread_id: None,
                            offset: 0,
                        },
                    )
                    .unwrap();
                    assert_eq!(
                        hidden["has_more"], false,
                        "hidden conversations cannot leak a page count"
                    );
                    assert!(hidden["threads"].as_array().unwrap().is_empty());
                    assert!(db
                        .inspect_blackboard("outsider", ORG, TEAM, &BlackboardQuery::default())
                        .is_err());
                    assert_eq!(
                        db.inspect_blackboard(
                            OWNER,
                            ORG,
                            TEAM,
                            &BlackboardQuery {
                                thread_id: Some(thread.clone()),
                                ..Default::default()
                            }
                        )
                        .unwrap()
                        .threads[0]
                            .reply_count,
                        0
                    );
                    restrict(
                        CapabilityScope::Workspace,
                        String::new(),
                        1,
                        CommunicationScope::AssignedWork,
                    );
                    let source = db
                        .huddle_execution_for_work(OWNER, ORG, TEAM, &pins[0].work_id)
                        .unwrap()
                        .unwrap()
                        .source_work_id;
                    let team_id = db
                        .work_team_for_work(OWNER, ORG, TEAM, &source)
                        .unwrap()
                        .unwrap()
                        .id;
                    restrict(
                        CapabilityScope::Team,
                        team_id.clone(),
                        0,
                        CommunicationScope::Blocked,
                    );
                    assert!(
                        invoke(0, OWNER, "team-blocked", post.clone()).is_err(),
                        "team ceiling applies to dispatched work"
                    );
                    restrict(
                        CapabilityScope::Team,
                        team_id,
                        1,
                        CommunicationScope::AssignedWork,
                    );
                    for i in 0..7 {
                        invoke(0, OWNER, &format!("limit-{i}"), post.clone()).unwrap();
                    }
                    assert!(
                        invoke(0, OWNER, "over-limit", post.clone()).is_err(),
                        "bounded contributions prevent unbounded chatter"
                    );
                    assert_eq!(
                        invoke(0, OWNER, "post", post).unwrap(),
                        first,
                        "retry does not consume another contribution"
                    );
                })
                .await
                .unwrap();
            // Removing the current grant fences a still-running pinned revision, too.
            let agent = workspace
                .services
                .agents()
                .await
                .unwrap()
                .into_iter()
                .find(|agent| agent.key == start.receipt.assignments[1].agent_key)
                .unwrap();
            workspace
                .update_agent(crate::workspace::UpdateLocalAgent {
                    agent_key: agent.key,
                    expected_definition_digest: agent.definition_digest,
                    configuration: CreateLocalAgent {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: agent.name,
                        purpose: agent.purpose,
                        model: agent.model,
                        provider: agent.provider,
                        harness: "general".into(),
                        max_steps: 8,
                        max_seconds: 120,
                        max_tokens: 4096,
                        tools: Some(vec![]),
                        workspace_root: None,
                        expected_workspace_root: None,
                        hosted_consent: false,
                        hosted_tools_consent: false,
                    },
                })
                .await
                .unwrap();
            let work = start.receipt.assignments[1].work_id.clone();
            workspace
                .services
                .local
                .store()
                .write(move |db| {
                    assert!(invoke_for_work(
                        db,
                        &work,
                        OWNER,
                        "revoked",
                        BlackboardCommand::Peers,
                        false
                    )
                    .is_err());
                })
                .await
                .unwrap();
            workspace.cancel(&start.receipt.root_work_id).await.unwrap();
            let work = start.receipt.assignments[0].work_id.clone();
            workspace
                .services
                .local
                .store()
                .write(move |db| {
                    assert!(invoke_for_work(
                        db,
                        &work,
                        OWNER,
                        "after-cancel",
                        BlackboardCommand::Peers,
                        false
                    )
                    .is_err());
                })
                .await
                .unwrap();
        })
        .await;
    server.abort();
}
