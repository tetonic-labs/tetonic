//! Real process effects through created agents; inference alone is a fixture.
use super::*;
use std::path::PathBuf;

struct ShellTransport {
    provider: &'static str,
    calls: Mutex<Vec<Value>>,
}
#[async_trait::async_trait]
impl HostedTransport for ShellTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(body.clone());
        Ok(if calls.len() == 1 {
            super::parity::completion(
                self.provider,
                "run_shell",
                json!({"command":"echo terminal-proof>receipt.txt"}),
                "shell-call",
            )
        } else {
            super::parity::completion(
                self.provider,
                "finish",
                json!({"summary":super::parity::output(&body,self.provider,"shell-call")}),
                "done",
            )
        })
    }
}

#[tokio::test]
async fn shell_provider_parity_requires_a_live_exact_once_owner_decision() {
    for provider in ["openai", "anthropic", "google"] {
        for decision in ["allow", "deny", "cancel", "expired", "unselected"] {
            tokio::task::LocalSet::new()
                .run_until(async {
                    let dir = tempfile::tempdir().unwrap();
                    let root = dir.path().join("work");
                    std::fs::create_dir(&root).unwrap();
                    let mut workspace = LocalWorkspace::open_with_workspace(
                        dir.path().join("state.db"),
                        "offline".into(),
                        "http://127.0.0.1:1".into(),
                        Some(root.clone()),
                    )
                    .await
                    .unwrap();
                    workspace.services.keys = Arc::new(ProviderKeys {
                        store: workspace.services.keys.store.clone(),
                        vault: Arc::new(Vault::default()),
                    });
                    workspace
                        .save_provider_key(SaveProviderKey {
                            provider: provider.into(),
                            api_key: "fixture-key".into(),
                        })
                        .await
                        .unwrap();
                    let transport = Arc::new(ShellTransport {
                        provider,
                        calls: Mutex::default(),
                    });
                    workspace.services.hosted_transport = Some(transport.clone());
                    let catalog = workspace.agent_catalog().await.unwrap();
                    for profile in &catalog.runtime_profiles {
                        assert!(profile.tools.contains(&"run_shell".into()));
                        assert!(
                            !profile.tools.contains(&"outline".into()),
                            "no unbound index advertised"
                        );
                    }
                    let agent = workspace
                        .create_agent(CreateLocalAgent {
                            provider: provider.into(),
                            hosted_consent: true,
                            hosted_tools_consent: true,
                            expected_workspace_root: catalog.workspace_root,
                            request_id: uuid::Uuid::new_v4().to_string(),
                            name: "Operator".into(),
                            purpose: "Use granted local commands".into(),
                            model: "fixture-model".into(),
                            harness: "general".into(),
                            max_steps: 3,
                            max_seconds: 30,
                            max_tokens: 1024,
                            tools: Some(if decision == "unselected" {
                                vec![]
                            } else {
                                vec!["run_shell".into()]
                            }),
                        })
                        .await
                        .unwrap();
                    let id = uuid::Uuid::new_v4().to_string();
                    workspace
                        .submit_for_agent(
                            id.clone(),
                            "Write the terminal receipt".into(),
                            agent.key,
                        )
                        .await
                        .unwrap();
                    if decision != "unselected" {
                        let approval =
                            tokio::time::timeout(std::time::Duration::from_secs(6), async {
                                loop {
                                    if let Some(a) = workspace
                                        .approvals()
                                        .await
                                        .unwrap()
                                        .pending_approvals
                                        .into_iter()
                                        .next()
                                    {
                                        break a;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                                }
                            })
                            .await
                            .unwrap_or_else(|e| panic!("shell approval should appear ({provider}/{decision}): {e:?}; calls={:?}", transport.calls.lock().unwrap()));
                        assert!(
                            !root.join("receipt.txt").exists(),
                            "no command runs before approval"
                        );
                        let proposal = approval.proposal.as_ref().unwrap();
                        assert_eq!(proposal.command, "echo terminal-proof>receipt.txt");
                        assert_eq!(proposal.digest(), approval.proposal_digest);
                        assert_eq!(
                            PathBuf::from(&proposal.working_directory),
                            tetonic_tools::Workspace::new(&root).unwrap().root()
                        );
                        if cfg!(windows) {
                            assert!(!proposal.confinement_warnings.is_empty());
                        }
                        assert!(workspace
                            .resolve_approval(
                                &approval.approval_id,
                                ResolveApprovalRequest {
                                    allow: true,
                                    proposal_digest: "changed-command".into()
                                }
                            )
                            .await
                            .is_err());
                        let hidden = workspace.services
                            .keys
                            .store
                            .write({
                                let row = approval.clone();
                                move |db| {
                                    db.register_control_principal("other-member")?;
                                    db.set_organization_member(
                                        ORG,
                                        "other-member",
                                        tetonic_memory::OrganizationRole::Member,
                                    )?;
                                    db.add_team_member(ORG, TEAM, "other-member")?;
                                    assert!(db
                                        .list_pending_effect_approvals("other-member", ORG, TEAM)?
                                        .is_empty());
                                    assert!(db
                                        .resolve_effect_approval(
                                            tetonic_memory::ResolveEffectApproval {
                                                actor: "other-member",
                                                org: ORG,
                                                team: TEAM,
                                                approval_id: &row.approval_id,
                                                proposal_digest: &row.proposal_digest,
                                                allow: true,
                                                now_unix: chrono::Utc::now().timestamp(),
                                            }
                                        )
                                        .is_err());
                                    Ok::<_, tetonic_memory::StoreError>(())
                                }
                            })
                            .await
                            .unwrap();
                        hidden.unwrap();
                        if decision == "cancel" {
                            workspace.cancel(&id).await.unwrap();
                            assert!(workspace
                                .resolve_approval(
                                    &approval.approval_id,
                                    ResolveApprovalRequest {
                                        allow: true,
                                        proposal_digest: approval.proposal_digest.clone()
                                    }
                                )
                                .await
                                .is_err());
                        } else if decision == "expired" {
                            let row = approval.clone();
                            let result = workspace.services
                                .keys
                                .store
                                .write(move |db| {
                                    db.resolve_effect_approval(
                                        tetonic_memory::ResolveEffectApproval {
                                            actor: OWNER,
                                            org: ORG,
                                            team: TEAM,
                                            approval_id: &row.approval_id,
                                            proposal_digest: &row.proposal_digest,
                                            allow: true,
                                            now_unix: row.expires_at,
                                        },
                                    )
                                })
                                .await
                                .unwrap();
                            assert!(result.is_err());
                            workspace.cancel(&id).await.unwrap();
                        } else {
                            let resolved = workspace
                                .resolve_approval(
                                    &approval.approval_id,
                                    ResolveApprovalRequest {
                                        allow: decision == "allow",
                                        proposal_digest: approval.proposal_digest.clone(),
                                    },
                                )
                                .await
                                .unwrap();
                            assert_eq!(
                                resolved.status,
                                if decision == "allow" {
                                    "approved"
                                } else {
                                    "rejected"
                                }
                            );
                        }
                        let result = settled(&workspace, &id).await;
                        if matches!(decision, "cancel" | "expired") {
                            assert_eq!(result.state, "canceled");
                        }
                        let replay = workspace.services
                            .keys
                            .store
                            .write(move |db| {
                                db.consume_shell_approval(
                                    ORG,
                                    TEAM,
                                    &approval.approval_id,
                                    &approval.proposal_digest,
                                    chrono::Utc::now().timestamp(),
                                )
                            })
                            .await
                            .unwrap();
                        assert!(
                            !matches!(replay, Ok(Some(true))),
                            "decision cannot be replayed"
                        );
                    } else {
                        settled(&workspace, &id).await;
                    }
                    let calls = transport.calls.lock().unwrap().clone();
                    assert_eq!(
                        super::parity::names(&calls[0], provider).contains(&"run_shell"),
                        decision != "unselected"
                    );
                    if decision == "allow" {
                        assert!(root.join("receipt.txt").exists(), "{provider}: {calls:?}");
                        assert_eq!(
                            std::fs::read_to_string(root.join("receipt.txt"))
                                .unwrap()
                                .trim(),
                            "terminal-proof"
                        );
                        assert!(super::parity::output(&calls[1], provider, "shell-call")
                            .contains("exit code: 0"));
                    } else {
                        assert!(!root.join("receipt.txt").exists());
                    }
                    assert!(workspace
                        .approvals()
                        .await
                        .unwrap()
                        .pending_approvals
                        .is_empty());
                })
                .await;
        }
    }
}
