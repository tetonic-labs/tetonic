//! Dispatch guard wiring, payload aggregation, and redaction (M2-2).

use lokai_domain::{
    Classification, ClassificationSource, DataClass, DispatchDecision, DispatchDenied,
    DispatchDestination, DispatchGuard, DispatchRequest, ProjectPlacementPolicy, WorkerTrust,
};
use lokai_policy::{
    classification_from_data_class, classify_message_payloads, combine_classifications,
};

use crate::{ChatRequest, Message};

/// Build aggregated classification for the complete outbound chat payload.
pub fn aggregate_chat_classification(req: &ChatRequest) -> Option<Classification> {
    let mut parts = Vec::new();

    if let Some(fabric) = req.fabric.as_ref() {
        parts.push(classification_from_data_class(
            fabric.data_class,
            ClassificationSource::SessionFloor,
        ));
        if let Some(ctx) = fabric.context_data_class {
            parts.push(classification_from_data_class(
                ctx,
                ClassificationSource::DerivedFromInput,
            ));
        }
        if let Some(summary) = &fabric.payload_classification {
            parts.push(classification_from_data_class(
                summary.class,
                ClassificationSource::AggregatedPayload,
            ));
        }
    }

    let message_refs: Vec<(&str, &str)> = req
        .messages
        .iter()
        .map(|m| (m.role.as_str(), m.content.as_str()))
        .collect();
    if let Some(content_class) = classify_message_payloads(&message_refs) {
        parts.push(content_class);
    }

    combine_classifications(parts)
}

pub fn dispatch_request_for_chat(
    req: &ChatRequest,
    worker_id: &str,
    post_redaction: bool,
    worker_trust: Option<WorkerTrust>,
    project_policy: ProjectPlacementPolicy,
) -> DispatchRequest {
    let session = req
        .fabric
        .as_ref()
        .map(|f| classification_from_data_class(f.data_class, ClassificationSource::SessionFloor));
    DispatchRequest {
        payload: aggregate_chat_classification(req),
        session,
        destination: DispatchDestination::RemoteWorker {
            worker_id: worker_id.to_string(),
        },
        post_redaction,
        worker_trust,
        project_policy,
    }
}

/// Stamp aggregated classification onto fabric metadata before dispatch.
pub fn stamp_request_classification(req: &mut ChatRequest) {
    let Some(c) = aggregate_chat_classification(req) else {
        return;
    };
    let summary = c.summary();
    if let Some(fabric) = req.fabric.as_mut() {
        fabric.payload_classification = Some(summary);
        fabric.data_class = lokai_policy::restrict_data_class(fabric.data_class, c.class);
    }
}

/// Outcome of planning a remote inference attempt (includes optional redaction).
#[derive(Clone)]
pub struct RemoteAttemptPlan {
    pub request: ChatRequest,
    pub decision: DispatchDecision,
    pub reason_code: Option<&'static str>,
    pub reason: Option<String>,
    pub classification: Option<Classification>,
    pub redacted: bool,
}

/// Evaluate remote dispatch; attempt deterministic redaction when content blocks remote.
pub fn plan_remote_attempt(
    guard: &dyn DispatchGuard,
    req: &ChatRequest,
    worker_id: &str,
    post_redaction: bool,
    worker_trust: Option<WorkerTrust>,
) -> RemoteAttemptPlan {
    let classification = aggregate_chat_classification(req);
    match evaluate_remote_dispatch(guard, req, worker_id, post_redaction, worker_trust) {
        Ok(decision) if decision.allows_remote() || post_redaction => RemoteAttemptPlan {
            request: req.clone(),
            decision,
            reason_code: None,
            reason: None,
            classification,
            redacted: post_redaction,
        },
        Ok(decision @ DispatchDecision::LocalOnly) if !post_redaction => {
            if let Ok((redacted, _manifest)) = redact_chat_request_for_remote(req) {
                let redacted_class = aggregate_chat_classification(&redacted);
                if let Ok(redacted_decision) =
                    evaluate_remote_dispatch(guard, &redacted, worker_id, true, worker_trust)
                {
                    if redacted_decision.allows_remote() {
                        return RemoteAttemptPlan {
                            request: redacted,
                            decision: redacted_decision,
                            reason_code: None,
                            reason: None,
                            classification: redacted_class,
                            redacted: true,
                        };
                    }
                }
            }
            RemoteAttemptPlan {
                request: req.clone(),
                decision,
                reason_code: Some("secret_local_only"),
                reason: Some("secret or policy requires local execution".into()),
                classification,
                redacted: false,
            }
        }
        Ok(decision) => RemoteAttemptPlan {
            request: req.clone(),
            decision,
            reason_code: None,
            reason: None,
            classification,
            redacted: post_redaction,
        },
        Err(deny) => RemoteAttemptPlan {
            request: req.clone(),
            decision: DispatchDecision::Denied,
            reason_code: Some(deny.reason_code),
            reason: Some(deny.reason),
            classification,
            redacted: false,
        },
    }
}

/// Evaluate whether remote placement is allowed for this chat request.
pub fn evaluate_remote_dispatch(
    guard: &dyn DispatchGuard,
    req: &ChatRequest,
    worker_id: &str,
    post_redaction: bool,
    worker_trust: Option<WorkerTrust>,
) -> Result<DispatchDecision, DispatchDenied> {
    guard.evaluate(&dispatch_request_for_chat(
        req,
        worker_id,
        post_redaction,
        worker_trust,
        guard.project_placement_policy(),
    ))
}

/// Build a dispatch request for non-chat remote jobs (embed, compute, artifact transfer).
pub fn dispatch_request_for_job(
    data_class: DataClass,
    worker_id: &str,
    worker_trust: WorkerTrust,
    project_policy: ProjectPlacementPolicy,
    post_redaction: bool,
) -> DispatchRequest {
    DispatchRequest {
        payload: Some(classification_from_data_class(
            data_class,
            ClassificationSource::AggregatedPayload,
        )),
        session: None,
        destination: DispatchDestination::RemoteWorker {
            worker_id: worker_id.to_string(),
        },
        post_redaction,
        worker_trust: Some(worker_trust),
        project_policy,
    }
}

/// Common guard entry for any remote job boundary (M5-3).
pub fn evaluate_remote_job_dispatch(
    guard: &dyn DispatchGuard,
    data_class: DataClass,
    worker_id: &str,
    worker_trust: WorkerTrust,
    project_policy: ProjectPlacementPolicy,
) -> Result<DispatchDecision, DispatchDenied> {
    guard.evaluate(&dispatch_request_for_job(
        data_class,
        worker_id,
        worker_trust,
        project_policy,
        false,
    ))
}

/// Redaction manifest for deterministic path/field stripping (M2-2 basic).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedactionManifest {
    pub redacted_message_indices: Vec<usize>,
    pub redacted_fields: Vec<String>,
    pub failed: bool,
}

/// Redact assignment-style secret lines (`password=`, `export KEY=`, etc.).
/// Inline token patterns (e.g. `sk-…` in prose) are not stripped here so residual
/// secrets can be detected on rescan.
fn redact_secret_assignment_lines(content: &str) -> (String, bool) {
    let mut changed = false;
    let mut out = String::new();
    for line in content.lines() {
        let t = line.trim();
        let mut redact_line = false;
        if t.starts_with("export ") || t.starts_with("set ") {
            let rest = t.split_once(' ').map(|(_, v)| v).unwrap_or(t);
            if let Some((key, _val)) = rest.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                if key.ends_with("_key")
                    || key.ends_with("_token")
                    || key.ends_with("_secret")
                    || key.ends_with("_password")
                    || matches!(key.as_str(), "password" | "secret" | "api_key" | "apikey")
                {
                    redact_line = true;
                }
            }
        } else if let Some((key, _val)) = t.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            if matches!(
                key.as_str(),
                "password"
                    | "passwd"
                    | "secret"
                    | "api_key"
                    | "apikey"
                    | "token"
                    | "access_token"
                    | "private_key"
            ) || key.ends_with("_secret")
                || key.ends_with("_token")
                || key.ends_with("_key")
            {
                redact_line = true;
            }
        }
        if redact_line {
            changed = true;
            out.push_str("[redacted — secret material withheld locally]");
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    while out.ends_with('\n') {
        out.pop();
    }
    (out, changed)
}

/// Deterministic redaction: strip assignment-style secret lines, then rescan full payload.
pub fn redact_chat_request_for_remote(
    req: &ChatRequest,
) -> Result<(ChatRequest, RedactionManifest), DispatchDenied> {
    let mut manifest = RedactionManifest::default();
    let mut redacted = req.clone();

    for (i, msg) in redacted.messages.iter_mut().enumerate() {
        let is_secret = classify_message_payloads(&[(msg.role.as_str(), msg.content.as_str())])
            .is_some_and(|c| c.class == DataClass::Secret);
        if !is_secret {
            continue;
        }
        let (partial, line_redacted) = redact_secret_assignment_lines(&msg.content);
        if line_redacted {
            msg.content = partial;
            manifest.redacted_message_indices.push(i);
            manifest
                .redacted_fields
                .push(format!("messages[{i}].content"));
        } else {
            manifest.redacted_message_indices.push(i);
            manifest
                .redacted_fields
                .push(format!("messages[{i}].content"));
            msg.content = "[redacted — secret material withheld locally]".into();
        }
    }

    if manifest.redacted_message_indices.is_empty() {
        return Err(DispatchDenied::new(
            "redaction_noop",
            "redaction requested but no secret fields identified",
        ));
    }

    let post = aggregate_chat_classification(&redacted);
    if post.as_ref().is_some_and(|c| c.class == DataClass::Secret) {
        manifest.failed = true;
        return Err(DispatchDenied::new(
            "redaction_failed",
            "redaction did not remove secret classification from payload",
        ));
    }

    if let Some(agg) = post {
        if let Some(fabric) = redacted.fabric.as_mut() {
            fabric.data_class = agg.class;
        }
    }

    Ok((redacted, manifest))
}

/// Failover redaction: withhold full prompt on retry (privacy + M2-2).
pub fn redact_messages_for_failover(messages: &[Message]) -> Vec<Message> {
    let _ = messages;
    vec![Message::user(
        "[failover retry — prior worker did not complete; full prompt withheld for privacy]",
    )]
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    use crate::FabricCallMeta;
    use lokai_policy::PolicyDispatchGuard;
    use lokai_policy::PolicyEngine;

    fn guard() -> PolicyDispatchGuard {
        PolicyDispatchGuard::from_engine(PolicyEngine::default())
    }

    fn make_req(messages: Vec<Message>, fabric: Option<FabricCallMeta>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages,
            fabric,
            ..Default::default()
        }
    }

    #[test]
    fn secret_in_earlier_message_forces_local() {
        let req = make_req(
            vec![
                Message::user("API_KEY=super-secret-value"),
                Message::user("summarize the readme"),
            ],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn secret_from_tool_result_forces_local() {
        let req = make_req(
            vec![Message::tool(
                "run_shell",
                "-----BEGIN PRIVATE KEY-----\nabc",
            )],
            Some(FabricCallMeta {
                data_class: DataClass::Public,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn missing_fabric_and_no_heuristic_denies_remote() {
        let req = make_req(vec![Message::user("hello")], None);
        let err = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap_err();
        assert_eq!(err.reason_code, "missing_classification");
    }

    #[test]
    fn redaction_failure_when_no_secret_fields_to_redact() {
        let req = make_req(
            vec![Message::user("hello world")],
            Some(FabricCallMeta {
                data_class: DataClass::Secret,
                ..Default::default()
            }),
        );
        assert!(redact_chat_request_for_remote(&req).is_err());
    }

    #[test]
    fn redaction_removes_label_but_secret_bytes_still_classified_as_secret() {
        let req = make_req(
            vec![Message::user("password=hunter2")],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let (redacted, manifest) = redact_chat_request_for_remote(&req).unwrap();
        assert!(!manifest.redacted_message_indices.is_empty());
        assert!(!redacted.messages[0].content.contains("hunter2"));
        let post = aggregate_chat_classification(&redacted).unwrap();
        assert_ne!(post.class, DataClass::Secret);
    }

    #[test]
    fn secret_in_process_output_forces_local() {
        let req = make_req(
            vec![Message::tool(
                "run_shell",
                "export DATABASE_PASSWORD=supersecret\n",
            )],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn derived_summary_with_secret_forces_local() {
        let req = make_req(
            vec![Message::user(
                "Summary of prior work: user shared API_KEY=sk-live-abc123",
            )],
            Some(FabricCallMeta {
                data_class: DataClass::Public,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn plan_remote_attempt_retries_with_redaction() {
        let req = make_req(
            vec![Message::user("password=hunter2")],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let plan = plan_remote_attempt(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        );
        assert!(plan.redacted);
        assert!(plan.decision.allows_remote());
    }

    #[test]
    fn failover_to_external_untrusted_blocked_for_repository_source() {
        let req = make_req(
            vec![Message::user("summarize readme")],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let trusted = evaluate_remote_dispatch(
            &guard(),
            &req,
            "trusted",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert!(trusted.allows_remote());

        let untrusted = evaluate_remote_dispatch(
            &guard(),
            &req,
            "external",
            false,
            Some(WorkerTrust::ExternalUntrusted),
        )
        .unwrap();
        assert_eq!(untrusted, DispatchDecision::LocalOnly);
    }

    #[test]
    fn legacy_chat_secret_payload_forces_local() {
        let req = make_req(
            vec![Message::user("token=sk-secret-value")],
            Some(FabricCallMeta {
                data_class: DataClass::Public,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "legacy-worker",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn secret_in_context_artifact_forces_local() {
        let req = make_req(
            vec![Message::user("summarize the readme")],
            Some(FabricCallMeta {
                data_class: DataClass::Public,
                context_data_class: Some(DataClass::Secret),
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn failover_gets_fresh_trust_evaluation_per_target() {
        let req = make_req(
            vec![Message::user("summarize readme")],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let owner = plan_remote_attempt(
            &guard(),
            &req,
            "owner-box",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        );
        assert!(owner.decision.allows_remote());

        let external = plan_remote_attempt(
            &guard(),
            &req,
            "external-box",
            false,
            Some(WorkerTrust::ExternalUntrusted),
        );
        assert_eq!(external.decision, DispatchDecision::LocalOnly);
        assert!(!external.redacted);
    }

    #[test]
    fn embed_job_secret_forces_local_only() {
        let d = evaluate_remote_job_dispatch(
            &guard(),
            DataClass::Secret,
            "w1",
            WorkerTrust::OwnerControlledEstate,
            ProjectPlacementPolicy::default(),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn compute_job_external_untrusted_denied_repository_source() {
        let d = evaluate_remote_job_dispatch(
            &guard(),
            DataClass::RepositorySource,
            "external",
            WorkerTrust::ExternalUntrusted,
            ProjectPlacementPolicy::default(),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn redaction_leaves_residual_secret_still_local() {
        let req = make_req(
            vec![Message::user(
                "Repository deployment notes\npassword=hunter2\nAlso billing uses sk-live-abc123456",
            )],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let err = redact_chat_request_for_remote(&req).unwrap_err();
        assert_eq!(err.reason_code, "redaction_failed");
    }

    #[test]
    fn repository_source_allowed_for_owner_estate() {
        let req = make_req(
            vec![Message::user("hello")],
            Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert!(d.allows_remote());
    }

    #[test]
    fn sensitive_source_to_external_worker_local_only() {
        let req = make_req(
            vec![Message::user("hello")],
            Some(FabricCallMeta {
                data_class: DataClass::SensitiveSource,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "external",
            false,
            Some(WorkerTrust::ExternalUntrusted),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn public_task_with_secret_session_floor_forces_local() {
        let req = make_req(
            vec![Message::user("hello world")],
            Some(FabricCallMeta {
                data_class: DataClass::Secret,
                ..Default::default()
            }),
        );
        let d = evaluate_remote_dispatch(
            &guard(),
            &req,
            "w1",
            false,
            Some(WorkerTrust::OwnerControlledEstate),
        )
        .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }
}
