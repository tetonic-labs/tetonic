//! M5-4 signed-result acceptance helpers for `RemoteNodeProvider`.

use chrono::Utc;
use serde::Deserialize;
use tetonic_domain::ids::WorkerId;
use tetonic_domain::result_integrity::WorkerBehaviorSignals;
use tetonic_domain::{CommandEnvelope, LeaseProof, ResultVerificationRequirement};
use tetonic_fabric_protocol::{
    ExpectedResultBinding, JobEnvelope, ResultDispositionRecord, ResultEnvelope,
    ResultSigningKeyRecord,
};
use tetonic_inference::{
    ActiveJobRegistry, ChatResponse, FabricJobResult, GenUsage, InferenceError,
    InferenceProvenance, JobStatus, TokenSink,
};

use crate::disposition_persist::DispositionPersistence;
use crate::legacy::RemoteNodeProvider;
use crate::result_accept::{accept_remote_result, RemoteResultAcceptRequest};
use crate::result_sign::key_id_from_public;
use crate::run_bridge::RemoteResultRunBridge;
use crate::verification::RedundantCandidate;
use crate::FabricClientError;

/// Worker-local + verification timings extracted during signed-result accept (M6-3).
#[derive(Debug, Clone, Default)]
pub(crate) struct AcceptedChatTiming {
    pub queue_ms: Option<u64>,
    pub execute_ms: Option<u64>,
    pub verification_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatResponseBody {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub result: Option<FabricJobResult>,
    #[serde(default)]
    pub result_signing_public_key: Option<String>,
}

/// Bind compatibility wire fields to the payload subsequently authenticated.
pub(crate) fn validate_chat_payload(result: &FabricJobResult) -> Result<(), InferenceError> {
    let envelope: ResultEnvelope = serde_json::from_value(
        result
            .result_envelope
            .clone()
            .ok_or_else(|| InferenceError::Decode("missing signed result".into()))?,
    )
    .map_err(|e| InferenceError::Decode(e.to_string()))?;
    let delivered = serde_json::json!({
        "message": result.message, "usage": result.usage,
        "status": format!("{:?}", result.status), "error": result.error,
    });
    if delivered != envelope.payload {
        return Err(InferenceError::Decode(
            "chat fields differ from signed payload".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_result_job_id(
    expected: &str,
    result: &FabricJobResult,
) -> Result<(), FabricClientError> {
    if result.job_id != expected {
        return Err(FabricClientError::Http(format!(
            "fabric job_id mismatch: expected {expected}, got {}",
            result.job_id
        )));
    }
    Ok(())
}

pub(crate) fn validate_result_identity(
    expected_job_id: &str,
    expected_attempt_id: Option<&str>,
    result: &FabricJobResult,
) -> Result<(), FabricClientError> {
    validate_result_job_id(expected_job_id, result)?;
    if let Some(expected) = expected_attempt_id {
        match result.attempt_id.as_deref() {
            Some(got) if got == expected => Ok(()),
            Some(got) => Err(FabricClientError::Http(format!(
                "fabric attempt_id mismatch: expected {expected}, got {got}"
            ))),
            None => Err(FabricClientError::Http(
                "fabric result missing attempt_id".into(),
            )),
        }
    } else {
        Ok(())
    }
}

pub(crate) fn handle_chat_response_body(
    parsed: ChatResponseBody,
    expected_job_id: &str,
    expected_attempt_id: Option<&str>,
    worker_id: &str,
    registry: Option<&ActiveJobRegistry>,
    model: &str,
    on_token: &mut TokenSink<'_>,
) -> Result<ChatResponse, InferenceError> {
    if !parsed.ok {
        if parsed
            .result
            .as_ref()
            .is_some_and(|r| r.status == JobStatus::Preempted)
        {
            return Err(InferenceError::Preempted {
                node_id: worker_id.to_string(),
            });
        }
        return Err(InferenceError::Provider(
            parsed.error.unwrap_or_else(|| "worker rejected job".into()),
        ));
    }
    let result = parsed
        .result
        .ok_or_else(|| InferenceError::Decode("missing result in fabric chat response".into()))?;
    validate_result_identity(expected_job_id, expected_attempt_id, &result)
        .map_err(|e| InferenceError::Decode(e.to_string()))?;
    let _ = registry;
    if result.status == JobStatus::Preempted {
        return Err(InferenceError::Preempted {
            node_id: worker_id.to_string(),
        });
    }
    if result.status != JobStatus::Ok {
        return Err(InferenceError::Provider(
            result
                .error
                .unwrap_or_else(|| format!("job status {:?}", result.status)),
        ));
    }
    if !result.message.content.is_empty() {
        on_token(&result.message.content);
    }
    Ok(ChatResponse {
        message: result.message,
        usage: GenUsage::from(result.usage),
        provenance: InferenceProvenance {
            provider_kind: "remote".into(),
            worker_id: Some(worker_id.to_string()),
            model: model.to_string(),
            job_id: Some(expected_job_id.to_string()),
            attempt_id: expected_attempt_id.map(String::from),
            prompt_redacted: false,
            ..Default::default()
        },
    })
}

/// Flush buffered NDJSON tokens only after signed accept (R2-2).
/// Prefer stream deltas when present so we do not double-emit full content.
pub(crate) fn emit_accepted_stream_tokens(
    buffered: &[String],
    full_content: &str,
    on_token: &mut TokenSink<'_>,
) {
    if buffered.is_empty() || buffered.concat() != full_content {
        if !full_content.is_empty() {
            on_token(full_content);
        }
    } else {
        for delta in buffered {
            on_token(delta);
        }
    }
}

/// Production stream finish: never invoke `on_token` unless accept succeeded.
pub(crate) fn deliver_stream_after_accept(
    accept: Result<AcceptedChatTiming, InferenceError>,
    buffered: &[String],
    full_content: &str,
    on_token: &mut TokenSink<'_>,
) -> Result<AcceptedChatTiming, InferenceError> {
    let timing = accept?;
    emit_accepted_stream_tokens(buffered, full_content, on_token);
    Ok(timing)
}

impl RemoteNodeProvider {
    pub fn register_result_signing_key(&self, public_key: &[u8]) -> Result<(), FabricClientError> {
        let key_id = key_id_from_public(public_key);
        let mut keys = self
            .result_keys
            .write()
            .map_err(|_| FabricClientError::Http("result key registry poisoned".into()))?;
        keys.register(ResultSigningKeyRecord {
            key_id,
            worker_id: WorkerId::new(&self.worker_id),
            public_key: public_key.to_vec(),
            valid_from: Utc::now() - chrono::Duration::hours(1),
            valid_until: None,
            revoked: false,
            identity_certification: None,
            rotation_generation: 1,
        })
        .map_err(FabricClientError::Protocol)
    }

    pub fn with_disposition_persistence(mut self, persist: ArcDispositionPersistence) -> Self {
        self.disposition_persist = Some(persist);
        self
    }

    pub fn with_run_bridge(self, bridge: ArcRunBridge) -> Self {
        self.set_run_bridge(bridge);
        self
    }

    pub fn behavior_signals(&self) -> WorkerBehaviorSignals {
        self.behavior_signals
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Validate a signed remote chat result before using its payload (M5-4).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn accept_signed_chat_result(
        &self,
        protocol_job: JobEnvelope,
        envelope_json: serde_json::Value,
        pubkey_hex: Option<String>,
        verification: ResultVerificationRequirement,
        registry: Option<&ActiveJobRegistry>,
        session_id: Option<String>,
        trace_id: Option<String>,
    ) -> Result<AcceptedChatTiming, InferenceError> {
        self.accept_signed_chat_result_with_redundant(
            protocol_job,
            envelope_json,
            pubkey_hex,
            verification,
            vec![],
            registry,
            session_id,
            trace_id,
        )
        .await
    }

    /// Validate a signed remote chat result with explicit redundant candidates (M5-4 / R13).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn accept_signed_chat_result_with_redundant(
        &self,
        protocol_job: JobEnvelope,
        envelope_json: serde_json::Value,
        pubkey_hex: Option<String>,
        verification: ResultVerificationRequirement,
        redundant: Vec<RedundantCandidate>,
        registry: Option<&ActiveJobRegistry>,
        session_id: Option<String>,
        trace_id: Option<String>,
    ) -> Result<AcceptedChatTiming, InferenceError> {
        let job_id = protocol_job.job_id.0.clone();
        let attempt_id = protocol_job.attempt_id.0.clone();
        let run_id = protocol_job.run_id.0.clone();
        let task_id = protocol_job.task_id.clone();

        if let Some(hex_key) = pubkey_hex.as_deref() {
            let pk = hex::decode(hex_key).map_err(|e| {
                InferenceError::Provider(format!("invalid result_signing_public_key: {e}"))
            })?;
            self.register_result_signing_key(&pk)
                .map_err(|e| InferenceError::Provider(e.to_string()))?;
        }
        let envelope: ResultEnvelope = serde_json::from_value(envelope_json)
            .map_err(|e| InferenceError::Decode(format!("result envelope json: {e}")))?;

        let flags = registry
            .map(|r| r.peek_attempt(&job_id, &attempt_id))
            .unwrap_or(tetonic_inference::AttemptBindingFlags {
                known: true,
                attempt_canceled: false,
                attempt_superseded: false,
                another_attempt_won: false,
            });

        let mut run_snapshot = None;
        let mut lease_proof: Option<LeaseProof> = None;
        let mut command_envelope: Option<CommandEnvelope> = None;
        let bridge = self.run_bridge.read().ok().and_then(|g| g.clone());
        if let Some(bridge) = bridge.as_ref() {
            if let Ok(Some(snap)) = bridge.load_snapshot(&run_id).await {
                if let Ok(Some((proof, env))) =
                    bridge.lease_proof_for_attempt(&run_id, &attempt_id).await
                {
                    lease_proof = Some(proof);
                    command_envelope = Some(env);
                }
                run_snapshot = Some(snap);
            }
            if lease_proof.is_none() {
                return Err(InferenceError::Provider(format!(
                    "unleased attempt {attempt_id} rejected: RunSupervisor has no lease proof"
                )));
            }
        }

        let expected = ExpectedResultBinding {
            channel_worker_id: WorkerId::new(&self.worker_id),
            coordinator_id: self.estate_id.clone(),
            run_id: protocol_job.run_id.clone(),
            job_id: protocol_job.job_id.clone(),
            task_id: protocol_job.task_id.clone(),
            task_version: protocol_job.task_version,
            attempt_id: protocol_job.attempt_id.clone(),
            lease_id: protocol_job.lease_id.clone(),
            lease_epoch: protocol_job.lease_epoch,
            input_digest: protocol_job.input_digest.clone(),
            workspace_version: protocol_job.workspace_version.clone(),
            worker_revoked: self.is_worker_revoked(),
            attempt_canceled: flags.attempt_canceled || !flags.known,
            attempt_superseded: flags.attempt_superseded,
            another_attempt_won: flags.another_attempt_won,
            run_active: run_snapshot
                .as_ref()
                .map(|s| !s.cancellation.run_canceled)
                .unwrap_or(true),
            task_active: run_snapshot
                .as_ref()
                .and_then(|s| s.tasks.get(&task_id))
                .map(|t| t.state != tetonic_domain::TaskState::Canceled)
                .unwrap_or(true),
            known_revocation_epoch: self
                .policy_epoch()
                .load(std::sync::atomic::Ordering::Relaxed),
            output_limits: protocol_job.output_limits.clone(),
        };

        let (outcome, signals_snapshot, durable, verification_ms) = {
            let keys = self
                .result_keys
                .read()
                .map_err(|_| InferenceError::Provider("result key registry poisoned".into()))?;
            let mut dispositions = self
                .result_dispositions
                .write()
                .map_err(|_| InferenceError::Provider("disposition store poisoned".into()))?;
            let mut signals = self
                .behavior_signals
                .write()
                .map_err(|_| InferenceError::Provider("behavior signals poisoned".into()))?;
            let verify_started = std::time::Instant::now();
            let outcome = accept_remote_result(
                &RemoteResultAcceptRequest {
                    envelope: envelope.clone(),
                    expected,
                    verification,
                    redundant,
                    artifact_bytes: vec![],
                    now: Utc::now(),
                    run_snapshot,
                    lease_proof,
                    command_envelope,
                },
                &keys,
                &mut dispositions,
                &mut signals,
            );
            let verification_ms = verify_started.elapsed().as_millis() as u64;
            tetonic_telemetry::record_compute_stage(
                tetonic_telemetry::span_names::VERIFICATION,
                Some("infer"),
                Some(outcome.disposition.as_str()),
                None,
                Some("remote"),
                None,
                None,
                Some(verification_ms),
                false,
            );
            let signals_snapshot = signals.clone();
            let durable = ResultDispositionRecord {
                result_id: envelope.body.result_id.clone(),
                idempotency_key: envelope.body.idempotency_key.0.clone(),
                disposition: outcome.disposition,
                reason: outcome.reason.clone(),
                recorded_at: Utc::now(),
                audit_only: matches!(
                    outcome.disposition,
                    tetonic_domain::ResultDisposition::RejectedStaleAttempt
                        | tetonic_domain::ResultDisposition::Superseded
                        | tetonic_domain::ResultDisposition::RejectedCanceled
                ),
            };
            (outcome, signals_snapshot, durable, verification_ms)
        };

        let accepted = outcome.disposition == tetonic_domain::ResultDisposition::Accepted;
        let mut durable = durable;
        let mut reject_msg = if accepted {
            None
        } else {
            Some(format!(
                "remote result rejected ({}): {}",
                outcome.disposition.as_str(),
                outcome.reason
            ))
        };

        // Settle AJR before durable Accepted (R2-2). Failed settle must not persist Accepted.
        if accepted {
            if let Some(reg) = registry {
                if let Err(e) = reg.validate(&job_id, &attempt_id) {
                    durable.disposition = tetonic_domain::ResultDisposition::RejectedStaleAttempt;
                    durable.reason = format!("AJR settle before accept: {e}");
                    durable.audit_only = true;
                    reject_msg = Some(format!(
                        "remote result rejected ({}): {}",
                        durable.disposition.as_str(),
                        durable.reason
                    ));
                }
            }
        }

        if let Some(persist) = &self.disposition_persist {
            let _ = persist.record(
                &durable.result_id.0,
                &durable.idempotency_key,
                durable.disposition,
                &durable.reason,
                durable.audit_only,
                Some(&self.worker_id),
                trace_id.as_deref(),
            );
            let _ = persist.persist_behavior_signals(&self.worker_id, &signals_snapshot);
            let _ = persist.audit_disposition(session_id.as_deref(), &durable);
        }
        tracing::info!(
            target: "lokai_fabric_result",
            worker_id = %self.worker_id,
            result_id = %durable.result_id.0,
            disposition = durable.disposition.as_str(),
            reason = %durable.reason,
            trace_id = trace_id.as_deref().unwrap_or(""),
            "result disposition"
        );

        self.apply_behavior_signal_eligibility();

        if let Some(msg) = reject_msg {
            return Err(InferenceError::Provider(msg));
        }
        let local = envelope.body.execution_summary.local.as_ref();
        Ok(AcceptedChatTiming {
            queue_ms: local.and_then(|l| l.queue_ms),
            execute_ms: local.and_then(|l| l.execute_ms),
            verification_ms: Some(verification_ms),
        })
    }

    fn apply_behavior_signal_eligibility(&self) {
        let state = self.behavior_signals().derive_operational_state();
        if let Ok(mut reg) = self.capability_registry.write() {
            match state {
                tetonic_domain::WorkerOperationalState::Quarantined
                | tetonic_domain::WorkerOperationalState::Revoked => {
                    reg.force_quarantine(&self.worker_id);
                }
                tetonic_domain::WorkerOperationalState::Degraded => {
                    reg.force_degraded(&self.worker_id);
                }
                tetonic_domain::WorkerOperationalState::Healthy => {}
            }
        }
    }

    /// Scheduling eligibility from result-integrity behavior signals (M5-4).
    pub fn result_integrity_allows_scheduling(&self) -> bool {
        self.behavior_signals()
            .derive_operational_state()
            .allows_scheduling()
    }
}

pub type ArcDispositionPersistence = std::sync::Arc<dyn DispositionPersistence>;
pub type ArcRunBridge = std::sync::Arc<dyn RemoteResultRunBridge>;

#[cfg(test)]
mod r2_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use tetonic_domain::{CommandEnvelope, LeaseProof, ResultDisposition, RunSnapshot};
    use tetonic_inference::{ActiveJobRegistry, Message};

    use crate::legacy::RemoteNodeProvider;
    use crate::legacy_adapter::{build_infer_job_envelope, LegacyInferParams};
    use crate::result_sign::{build_signed_chat_result, key_id_from_public};
    use crate::run_bridge::RemoteResultRunBridge;
    use crate::DispositionPersistence;
    use lokai_enroll::KeyPair;
    use tetonic_egress::EgressGuard;
    use tetonic_fabric_protocol::ResultDispositionRecord;

    struct MemPersist {
        by_result: Mutex<HashMap<String, ResultDispositionRecord>>,
    }

    impl MemPersist {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                by_result: Mutex::new(HashMap::new()),
            })
        }
        fn get(&self, id: &str) -> Option<ResultDisposition> {
            self.by_result
                .lock()
                .unwrap()
                .get(id)
                .map(|r| r.disposition)
        }
    }

    impl DispositionPersistence for MemPersist {
        fn record(
            &self,
            result_id: &str,
            idempotency_key: &str,
            disposition: ResultDisposition,
            reason: &str,
            audit_only: bool,
            _worker_id: Option<&str>,
            _trace_id: Option<&str>,
        ) -> Result<ResultDispositionRecord, String> {
            let rec = ResultDispositionRecord {
                result_id: tetonic_domain::ids::ResultId::new(result_id),
                idempotency_key: idempotency_key.into(),
                disposition,
                reason: reason.into(),
                recorded_at: Utc::now(),
                audit_only,
            };
            self.by_result
                .lock()
                .unwrap()
                .insert(result_id.to_string(), rec.clone());
            Ok(rec)
        }
        fn get_by_result(
            &self,
            result_id: &str,
        ) -> Result<Option<ResultDispositionRecord>, String> {
            Ok(self.by_result.lock().unwrap().get(result_id).cloned())
        }
        fn get_by_idempotency(
            &self,
            _key: &str,
        ) -> Result<Option<ResultDispositionRecord>, String> {
            Ok(None)
        }
        fn persist_behavior_signals(
            &self,
            _worker_id: &str,
            _signals: &tetonic_domain::result_integrity::WorkerBehaviorSignals,
        ) -> Result<(), String> {
            Ok(())
        }
        fn audit_disposition(
            &self,
            _session_id: Option<&str>,
            _record: &ResultDispositionRecord,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    struct EmptyBridge;

    #[async_trait]
    impl RemoteResultRunBridge for EmptyBridge {
        async fn load_snapshot(&self, _run_id: &str) -> Result<Option<RunSnapshot>, String> {
            Ok(None)
        }
        async fn lease_proof_for_attempt(
            &self,
            _run_id: &str,
            _attempt_id: &str,
        ) -> Result<Option<(LeaseProof, CommandEnvelope)>, String> {
            Ok(None)
        }
    }

    fn provider(epoch: u64) -> RemoteNodeProvider {
        RemoteNodeProvider::new(
            "w1".into(),
            "box".into(),
            "127.0.0.1".parse().unwrap(),
            9443,
            vec![],
            Arc::new(KeyPair::generate()),
            Arc::new(EgressGuard::new()),
            "estate".into(),
            Arc::new(AtomicU64::new(epoch)),
        )
    }

    fn signed_chat(
        provider: &RemoteNodeProvider,
    ) -> (
        tetonic_fabric_protocol::JobEnvelope,
        serde_json::Value,
        String,
    ) {
        let sk = SigningKey::generate(&mut OsRng);
        let pk = sk.verifying_key().to_bytes().to_vec();
        provider.register_result_signing_key(&pk).unwrap();
        let job = build_infer_job_envelope(
            &LegacyInferParams {
                job_id: "job_r2".into(),
                attempt_id: "att_r2".into(),
                data_class: tetonic_inference::DataClass::RepositorySource,
                run_id: Some("run_r2".into()),
                task_id: Some("task_r2".into()),
            },
            &[Message::user("hi")],
            "qwen:7b",
        )
        .unwrap();
        let kid = key_id_from_public(&pk);
        let (result, env) = build_signed_chat_result(
            &job,
            "estate",
            "w1",
            &kid,
            &sk,
            Message::assistant("ok"),
            tetonic_inference::GenUsageSerde::default(),
            tetonic_inference::JobStatus::Ok,
            None,
        )
        .unwrap();
        let json = result.result_envelope.unwrap();
        let _ = env;
        let result_id = format!("res_{}", job.attempt_id.0);
        (job, json, result_id)
    }

    #[test]
    fn outer_chat_fields_must_equal_the_signed_payload() {
        let p = provider(0);
        let (job, json, _) = signed_chat(&p);
        let envelope: ResultEnvelope = serde_json::from_value(json.clone()).unwrap();
        let mut result = FabricJobResult {
            job_id: job.job_id.0,
            attempt_id: Some(job.attempt_id.0),
            message: serde_json::from_value(envelope.payload["message"].clone()).unwrap(),
            usage: serde_json::from_value(envelope.payload["usage"].clone()).unwrap(),
            status: JobStatus::Ok,
            error: None,
            result_envelope: Some(json),
        };
        assert!(validate_chat_payload(&result).is_ok());
        result.message.content = "different output".into();
        assert!(validate_chat_payload(&result).is_err());
    }

    #[tokio::test]
    async fn coordinator_epoch_rejects_stale_envelope() {
        let p = provider(5);
        let (job, json, _) = signed_chat(&p);
        let err = p
            .accept_signed_chat_result(
                job,
                json,
                None,
                tetonic_domain::ResultVerificationRequirement::default(),
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("stale") || msg.contains("epoch") || msg.contains("rejected"),
            "expected epoch reject, got {msg}"
        );
    }

    #[tokio::test]
    async fn unleased_attempt_rejected_when_bridge_set() {
        let p = provider(0).with_run_bridge(Arc::new(EmptyBridge));
        let (job, json, _) = signed_chat(&p);
        let err = p
            .accept_signed_chat_result(
                job,
                json,
                None,
                tetonic_domain::ResultVerificationRequirement::default(),
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unleased"), "got {err}");
    }

    #[tokio::test]
    async fn ajr_settle_failure_does_not_persist_accepted() {
        let persist = MemPersist::new();
        let p = provider(0).with_disposition_persistence(persist.clone());
        let (job, json, result_id) = signed_chat(&p);
        let reg = ActiveJobRegistry::new();
        let aid = job.attempt_id.0.clone();
        let jid = job.job_id.0.clone();
        let bound = reg.begin_attempt_with_id(&jid, None, &aid);
        assert_eq!(reg.validate(&jid, &bound), Ok(()));
        let err = p
            .accept_signed_chat_result(
                job,
                json,
                None,
                tetonic_domain::ResultVerificationRequirement::default(),
                Some(&reg),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("rejected") || err.to_string().contains("AJR"),
            "got {err}"
        );
        assert_ne!(
            persist.get(&result_id),
            Some(ResultDisposition::Accepted),
            "must not persist Accepted after settle failure"
        );
    }

    struct LookupBridge {
        looked_up: Mutex<Vec<String>>,
        proof: LeaseProof,
    }

    #[async_trait]
    impl RemoteResultRunBridge for LookupBridge {
        async fn load_snapshot(&self, _run_id: &str) -> Result<Option<RunSnapshot>, String> {
            Ok(None)
        }
        async fn lease_proof_for_attempt(
            &self,
            _run_id: &str,
            attempt_id: &str,
        ) -> Result<Option<(LeaseProof, CommandEnvelope)>, String> {
            self.looked_up.lock().unwrap().push(attempt_id.to_string());
            Ok(Some((
                self.proof.clone(),
                lokai_run::command_envelope("t", Some(1), "test"),
            )))
        }
    }

    #[tokio::test]
    async fn accept_loads_lease_proof_for_job_attempt_id() {
        let looked = Arc::new(LookupBridge {
            looked_up: Mutex::new(Vec::new()),
            proof: LeaseProof {
                lease_id: tetonic_domain::ids::LeaseId::new("lease_r2"),
                lease_epoch: 0,
                holder: tetonic_domain::ExecutionTargetId::worker("w1"),
            },
        });
        // Empty snapshot fail-closes; wrapper returns a snapshot so lookup is invoked.
        struct SnapBridge {
            inner: Arc<LookupBridge>,
        }
        #[async_trait]
        impl RemoteResultRunBridge for SnapBridge {
            async fn load_snapshot(&self, run_id: &str) -> Result<Option<RunSnapshot>, String> {
                let _ = run_id;
                Ok(Some(RunSnapshot {
                    run_id: tetonic_domain::ids::RunId::new("run_r2"),
                    session_id: Some(tetonic_domain::ids::SessionId::new("sess")),
                    state: tetonic_domain::RunState::Active,
                    sequence: 1,
                    workspace_version: None,
                    tasks: Default::default(),
                    attempts: Default::default(),
                    dependencies: Default::default(),
                    events: vec![],
                    delivery_index: Default::default(),
                    side_effect_commits: Default::default(),
                    deadlines: Default::default(),
                    cancellation: Default::default(),
                    speculation: Default::default(),
                    next_lease_epoch: 0,
                    job_spec: None,
                }))
            }
            async fn lease_proof_for_attempt(
                &self,
                run_id: &str,
                attempt_id: &str,
            ) -> Result<Option<(LeaseProof, CommandEnvelope)>, String> {
                self.inner.lease_proof_for_attempt(run_id, attempt_id).await
            }
        }

        let p = provider(0).with_run_bridge(Arc::new(SnapBridge {
            inner: looked.clone(),
        }));
        let (job, json, _) = signed_chat(&p);
        let expected = job.attempt_id.0.clone();
        let _ = p
            .accept_signed_chat_result(
                job,
                json,
                None,
                tetonic_domain::ResultVerificationRequirement::default(),
                None,
                None,
                None,
            )
            .await;
        assert_eq!(
            looked.looked_up.lock().unwrap().as_slice(),
            [expected.as_str()]
        );
    }

    #[tokio::test]
    async fn invalid_signature_emits_zero_buffered_tokens() {
        let p = provider(0);
        let (job, mut json, _) = signed_chat(&p);
        match json.get_mut("signature") {
            Some(serde_json::Value::Array(arr)) if !arr.is_empty() => {
                arr[0] = serde_json::json!(0);
            }
            Some(other) => *other = serde_json::json!(vec![0u8; 64]),
            None => json["signature"] = serde_json::json!(vec![0u8; 64]),
        }
        let mut emitted = Vec::new();
        let mut sink = |t: &str| emitted.push(t.to_string());
        let buffered = vec!["tok_secret".to_string()];
        let accept = p
            .accept_signed_chat_result(
                job,
                json,
                None,
                tetonic_domain::ResultVerificationRequirement::default(),
                None,
                None,
                None,
            )
            .await;
        assert!(accept.is_err(), "tampered signature must fail accept");
        let err = crate::legacy_result::deliver_stream_after_accept(
            accept,
            &buffered,
            "FULL_CONTENT",
            &mut sink,
        )
        .unwrap_err();
        assert!(
            emitted.is_empty(),
            "buffered tokens leaked after failed accept ({err})"
        );
    }
}

#[cfg(test)]
mod payload_binding_regressions {
    use super::*;
    #[test]
    fn mismatched_tokens_are_never_published() {
        let mut emitted = String::new();
        emit_accepted_stream_tokens(&["unverified".into()], "verified", &mut |s| {
            emitted.push_str(s)
        });
        assert_eq!(emitted, "verified");
    }
}
