//! `/v1/chat` handler — scheduler integration and ingress dedup replay.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::Stream;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use lokai_fabric_client::{build_signed_chat_result_timed, fabric_job_to_envelope};
use lokai_fabric_protocol::{
    CancellationAcknowledged, CancellationRequest, FabricEnvelope, IdempotencyKey,
    IngressDecision as ProtocolIngressDecision, JobEnvelope, JobOffer, LeaseRenewalRequest,
    LeaseRenewalResponse, LifecycleContext, ProtocolVersion, TerminalOutcome, WorkerLocalDurations,
};
use lokai_inference::{
    ChatRequest, FabricJob, FabricJobResult, GenUsageSerde, InferenceError, InferenceProvider,
    JobStatus, Message, OutboundScan,
};
use serde_json::json;
use tokio::sync::watch;

use crate::scheduler::SchedulerError;

use crate::fabric::{
    body_limit_response, collect_limited_body, json_response, reject_stale_policy_epoch,
    worker_accepts_data_class, FabricBody, FabricState,
};

async fn respond_terminal(
    state: &FabricState,
    envelope: &JobEnvelope,
    outcome: TerminalOutcome,
    status: StatusCode,
    body: serde_json::Value,
) -> Response<FabricBody> {
    match state
        .job_ingress
        .mark_terminal(envelope, outcome, Some(body.to_string()))
        .await
    {
        Ok(()) => json_response(status, body),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({
                "ok": false,
                "error": e.message,
                "code": "ingress_persist_failed",
            }),
        ),
    }
}

async fn scheduler_refuse(
    state: &FabricState,
    envelope: &JobEnvelope,
    err: SchedulerError,
) -> Response<FabricBody> {
    let (status, body, reason) = match err {
        SchedulerError::OwnerActiveBlocksCircle => (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({
                "ok": false,
                "error": "owner active — circle jobs not accepted",
                "code": "owner_active",
            }),
            "owner_active",
        ),
        SchedulerError::Preempted(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"ok": false, "error": "preempted", "code": "owner_preempt"}),
            "owner_preempt",
        ),
    };
    respond_terminal(
        state,
        envelope,
        TerminalOutcome::Rejected(reason.into()),
        status,
        body,
    )
    .await
}

async fn mark_stream_terminal(
    state: &FabricState,
    envelope: &JobEnvelope,
    outcome: TerminalOutcome,
    cache: serde_json::Value,
) {
    if let Err(e) = state
        .job_ingress
        .mark_terminal(envelope, outcome, Some(cache.to_string()))
        .await
    {
        tracing::error!(
            error = %e.message,
            "ingress persist failed after stream terminal"
        );
    }
}

fn seal_result(
    state: &FabricState,
    job_envelope: &JobEnvelope,
    message: Message,
    usage: GenUsageSerde,
    status: JobStatus,
    error: Option<String>,
    local: Option<WorkerLocalDurations>,
) -> FabricJobResult {
    let duration_ms = local.as_ref().and_then(|l| l.execute_ms);
    match build_signed_chat_result_timed(
        job_envelope,
        &state.estate_id,
        &state.peer_id,
        &state.result_key_id,
        state.result_signing_key.as_ref(),
        message.clone(),
        usage.clone(),
        status,
        error.clone(),
        duration_ms,
        local,
    ) {
        Ok((result, _)) => result,
        Err(e) => {
            tracing::error!("result signing failed: {e}");
            FabricJobResult {
                job_id: job_envelope.job_id.0.clone(),
                attempt_id: Some(job_envelope.attempt_id.0.clone()),
                message,
                usage,
                status: JobStatus::Error,
                error: Some(format!("result signing failed: {e}")),
                result_envelope: None,
            }
        }
    }
}

fn local_durations(queue_ms: u64, execute_started: std::time::Instant) -> WorkerLocalDurations {
    WorkerLocalDurations {
        queue_ms: Some(queue_ms),
        execute_ms: Some(execute_started.elapsed().as_millis() as u64),
        model_load_ms: None,
        serialize_out_ms: None,
    }
}

fn spawn_lease_expiry_watch(state: Arc<FabricState>, job_id: String, attempt_id: String) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let expired = {
                let leases = state.leases.lock().await;
                match leases.expires_at(&attempt_id) {
                    None => return,
                    Some(exp) => std::time::Instant::now() >= exp,
                }
            };
            if expired {
                tracing::warn!(
                    job_id = %job_id,
                    attempt_id = %attempt_id,
                    "typed job lease expired without renewal — canceling"
                );
                let _ = state.scheduler.cancel_job(&job_id).await;
                let mut leases = state.leases.lock().await;
                leases.remove(&attempt_id);
                return;
            }
        }
    });
}

pub(super) async fn jobs_cancel(
    state: Arc<FabricState>,
    req: Request<Incoming>,
) -> Response<FabricBody> {
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    let Ok(cancel_req): Result<FabricEnvelope<CancellationRequest>, _> =
        serde_json::from_slice(&body)
    else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "invalid json cancellation envelope"}),
        );
    };

    let cancel = cancel_req.payload;
    if let Err(e) = lokai_fabric_protocol::validate_cancellation(&cancel) {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": e.message, "code": format!("{:?}", e.code)}),
        );
    }

    let attempt_id = cancel.attempt_id.0.clone();
    let job_id = {
        let map = state.attempt_jobs.lock().await;
        map.get(&attempt_id).cloned()
    };
    // Scheduler acquires with JobEnvelope.job_id; cancel maps attempt → job via attempt_jobs.
    let matched = if let Some(ref jid) = job_id {
        state.scheduler.cancel_job(jid).await
    } else {
        // Best-effort: some callers may pass job_id as attempt_id.
        state.scheduler.cancel_job(&attempt_id).await
    };

    let ack = CancellationAcknowledged {
        context: LifecycleContext {
            job_id: lokai_domain::ids::JobId::new(job_id.as_deref().unwrap_or(&attempt_id)),
            task_id: cancel.task_id.clone(),
            attempt_id: cancel.attempt_id.clone(),
            lease_id: cancel.lease_id.clone(),
            lease_epoch: cancel.lease_epoch,
            worker_id: lokai_domain::ids::WorkerId::new(&state.peer_id),
            sequence_number: 1,
            protocol_version: ProtocolVersion(lokai_fabric_protocol::PROTOCOL_VERSION),
            idempotency_key: IdempotencyKey(format!("cancel:{}", attempt_id)),
        },
    };
    {
        let mut cs = state.cancel_state.lock().await;
        let _ = cs.acknowledge(&cancel, ack);
    }

    json_response(
        StatusCode::OK,
        json!({"ok": true, "matched": matched, "attempt_id": attempt_id}),
    )
}

pub(super) async fn jobs_lease(
    state: Arc<FabricState>,
    req: Request<Incoming>,
) -> Response<FabricBody> {
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    let Ok(lease_env): Result<FabricEnvelope<LeaseRenewalRequest>, _> =
        serde_json::from_slice(&body)
    else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "invalid json lease envelope"}),
        );
    };

    let lease_req = lease_env.payload;
    if let Err(e) = lokai_fabric_protocol::validate_lifecycle_context(
        &lease_req.context,
        &lease_req.context.worker_id,
    ) {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": e.message, "code": format!("{:?}", e.code)}),
        );
    }

    {
        let cs = state.cancel_state.lock().await;
        if let Err(e) =
            cs.can_renew_lease(&lease_req.context.attempt_id, lease_req.context.lease_epoch)
        {
            return json_response(
                StatusCode::CONFLICT,
                json!({
                    "ok": false,
                    "error": e.message,
                    "code": "lease_renewal_canceled",
                }),
            );
        }
    }

    let job_id = lease_req.context.job_id.0.clone();
    let attempt_id = lease_req.context.attempt_id.0.clone();
    let lease_epoch = lease_req.context.lease_epoch;
    let known_attempt = {
        let map = state.attempt_jobs.lock().await;
        map.get(&attempt_id).map(|j| j == &job_id).unwrap_or(false)
    };
    let running = state.scheduler.is_running(&job_id).await;
    if !running && !known_attempt {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({
                "ok": false,
                "error": "no matching in-flight job for lease renewal",
                "code": "lease_job_not_found",
            }),
        );
    }

    let ttl = crate::lease_table::default_lease_ttl();
    let granted_instant = {
        let mut leases = state.leases.lock().await;
        if let Some(until) = leases.renew(&attempt_id, &job_id, lease_epoch, ttl) {
            until
        } else {
            let until = std::time::Instant::now() + ttl;
            leases.insert(crate::lease_table::ActiveLease {
                job_id: job_id.clone(),
                attempt_id: attempt_id.clone(),
                lease_epoch,
                expires_at: until,
            });
            until
        }
    };
    let remain = granted_instant.saturating_duration_since(std::time::Instant::now());
    let granted = chrono::Utc::now()
        + chrono::Duration::from_std(remain).unwrap_or_else(|_| chrono::Duration::seconds(30));
    let resp = LeaseRenewalResponse {
        context: lease_req.context,
        granted_expires_at: granted,
    };
    json_response(
        StatusCode::OK,
        serde_json::to_value(resp).unwrap_or(json!({"ok": true})),
    )
}

pub(super) async fn jobs(state: Arc<FabricState>, req: Request<Incoming>) -> Response<FabricBody> {
    let accept = req
        .headers()
        .get(hyper::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    let Ok(envelope): Result<FabricEnvelope<JobOffer>, _> = serde_json::from_slice(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "invalid json envelope"}),
        );
    };

    let job_envelope = envelope.payload.job;

    if !worker_accepts_data_class(job_envelope.data_class) {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({"ok": false, "error": "secret data class rejected on worker"}),
        );
    }

    if let Some(resp) = reject_stale_policy_epoch(&state, envelope.revocation_epoch).await {
        return resp;
    }

    if !matches!(
        job_envelope.payload,
        lokai_fabric_protocol::VersionedJobPayload::V1Infer(_)
    ) {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "missing V1Infer payload"}),
        );
    }
    {
        let cs = state.cancel_state.lock().await;
        if let Err(e) = cs.can_renew_lease(&job_envelope.attempt_id, job_envelope.lease_epoch) {
            return json_response(
                StatusCode::CONFLICT,
                json!({
                    "ok": false,
                    "error": e.message,
                    "code": "lease_renewal_canceled",
                }),
            );
        }
    }

    match state.job_ingress.accept(&job_envelope).await {
        Ok(ProtocolIngressDecision::ReplayExisting(rec)) => {
            if let Some(cached) = rec.cached_response_json {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(
                        Full::new(Bytes::from(cached))
                            .map_err(|never: Infallible| match never {})
                            .boxed(),
                    )
                    .unwrap();
            }
            return json_response(
                StatusCode::CONFLICT,
                json!({
                    "ok": false,
                    "error": "terminal job missing cached response for replay",
                    "code": "replay_missing_cache",
                }),
            );
        }
        Ok(ProtocolIngressDecision::DuplicateInFlight) => {
            return json_response(
                StatusCode::CONFLICT,
                json!({
                    "ok": false,
                    "error": "duplicate job delivery in progress",
                    "code": "duplicate_in_flight",
                }),
            );
        }
        Ok(ProtocolIngressDecision::AcceptNew) => {}
        Err(e) => {
            let status = if e.code == lokai_fabric_protocol::FabricErrorCode::InternalFailure {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::CONFLICT
            };
            return json_response(
                status,
                json!({
                    "ok": false,
                    "error": e.message,
                    "code": format!("{:?}", e.code),
                }),
            );
        }
    }

    {
        let mut map = state.attempt_jobs.lock().await;
        map.insert(
            job_envelope.attempt_id.0.clone(),
            job_envelope.job_id.0.clone(),
        );
    }
    {
        let ttl = crate::lease_table::default_lease_ttl();
        let mut leases = state.leases.lock().await;
        leases.insert(crate::lease_table::ActiveLease {
            job_id: job_envelope.job_id.0.clone(),
            attempt_id: job_envelope.attempt_id.0.clone(),
            lease_epoch: job_envelope.lease_epoch,
            expires_at: std::time::Instant::now() + ttl,
        });
    }
    spawn_lease_expiry_watch(
        state.clone(),
        job_envelope.job_id.0.clone(),
        job_envelope.attempt_id.0.clone(),
    );

    let lokai_fabric_protocol::VersionedJobPayload::V1Infer(ref payload) = job_envelope.payload
    else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "missing V1Infer payload"}),
        );
    };
    let messages: Vec<Message> =
        serde_json::from_value(payload.get("messages").cloned().unwrap_or_default())
            .unwrap_or_default();
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tools: Vec<lokai_inference::ToolSchema> =
        serde_json::from_value(payload.get("tools").cloned().unwrap_or_default())
            .unwrap_or_default();

    let job = FabricJob {
        job_id: job_envelope.job_id.0.clone(),
        attempt_id: Some(job_envelope.attempt_id.0.clone()),
        estate_id: envelope.coordinator_id.0.clone(),
        session_id: Some(job_envelope.run_id.0.clone()),
        agent_id: job_envelope.task_id.0.clone(),
        step_index: 0,
        model,
        tier: None,
        messages,
        tools,
        options: lokai_inference::SampleOptions {
            stream: Some(true),
            ..Default::default()
        },
        priority: lokai_inference::JobPriority::OwnerInteractive,
        data_class: job_envelope.data_class,
        disclosure_tier: lokai_inference::DisclosureTier::Auditable,
        audit_envelope: None,
        circle_id: None,
        consumer_peer_id: None,
        policy_epoch: envelope.revocation_epoch,
        turn_affinity: None,
    };

    let wants_stream = accept.contains("application/x-ndjson")
        || accept.contains("text/event-stream")
        || job.options.stream.unwrap_or(false);

    if wants_stream {
        return chat_ndjson(state, job, job_envelope).await;
    }

    chat_json(state, job, job_envelope).await
}

pub(super) async fn chat(state: Arc<FabricState>, req: Request<Incoming>) -> Response<FabricBody> {
    let accept = req
        .headers()
        .get(hyper::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    let Ok(job): Result<FabricJob, _> = serde_json::from_slice(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "invalid json"}),
        );
    };

    if !worker_accepts_data_class(job.data_class) {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({"ok": false, "error": "secret data class rejected on worker"}),
        );
    }

    if let Some(resp) = reject_stale_policy_epoch(&state, job.policy_epoch).await {
        return resp;
    }

    let envelope = match fabric_job_to_envelope(&job) {
        Ok(e) => e,
        Err(e) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"ok": false, "error": e.to_string(), "code": "invalid_job_envelope"}),
            );
        }
    };
    match state.job_ingress.accept(&envelope).await {
        Ok(ProtocolIngressDecision::ReplayExisting(rec)) => {
            if let Some(cached) = rec.cached_response_json {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(
                        Full::new(Bytes::from(cached))
                            .map_err(|never: Infallible| match never {})
                            .boxed(),
                    )
                    .unwrap();
            }
            return json_response(
                StatusCode::CONFLICT,
                json!({
                    "ok": false,
                    "error": "terminal job missing cached response for replay",
                    "code": "replay_missing_cache",
                }),
            );
        }
        Ok(ProtocolIngressDecision::DuplicateInFlight) => {
            return json_response(
                StatusCode::CONFLICT,
                json!({
                    "ok": false,
                    "error": "duplicate job delivery in progress",
                    "code": "duplicate_in_flight",
                }),
            );
        }
        Ok(ProtocolIngressDecision::AcceptNew) => {}
        Err(e) => {
            let status = if e.code == lokai_fabric_protocol::FabricErrorCode::InternalFailure {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::CONFLICT
            };
            return json_response(
                status,
                json!({
                    "ok": false,
                    "error": e.message,
                    "code": format!("{:?}", e.code),
                }),
            );
        }
    }

    let wants_stream = accept.contains("application/x-ndjson")
        || accept.contains("text/event-stream")
        || job.options.stream.unwrap_or(false);

    if wants_stream {
        return chat_ndjson(state, job, envelope).await;
    }

    chat_json(state, job, envelope).await
}

async fn chat_json(
    state: Arc<FabricState>,
    job: FabricJob,
    envelope: lokai_fabric_protocol::JobEnvelope,
) -> Response<FabricBody> {
    let job_id = job.job_id.clone();
    let _attempt_id = job.attempt_id.clone();
    let queue_started = std::time::Instant::now();
    let cancel_rx = match state.scheduler.acquire(&job_id, job.priority).await {
        Ok(rx) => rx,
        Err(err) => return scheduler_refuse(&state, &envelope, err).await,
    };

    let queue_ms = queue_started.elapsed().as_millis() as u64;
    let chat_req = ChatRequest {
        model: job.model.clone(),
        model_digest: None,
        messages: job.messages,
        tools: job.tools,
        temperature: job.options.temperature,
        num_ctx: job.options.num_ctx,
        keep_alive: Some("10m".into()),
        fabric: None,
        response_format: job.options.response_format.clone(),
        outbound_scan: OutboundScan::from_scan(false),
        ..Default::default()
    };

    let started = std::time::Instant::now();
    let assembled = Arc::new(std::sync::Mutex::new(String::new()));
    let asm = assembled.clone();
    let run = run_cancellable_chat(state.inference.clone(), chat_req, cancel_rx, move |t| {
        if let Ok(mut s) = asm.lock() {
            s.push_str(t);
        }
    })
    .await;
    state.scheduler.release(&job_id).await;
    if let Some(aid) = &_attempt_id {
        let mut map = state.attempt_jobs.lock().await;
        map.remove(aid);
        let mut leases = state.leases.lock().await;
        leases.remove(aid);
    }

    match run {
        Ok(mut resp) => {
            let text = assembled.lock().map(|s| s.clone()).unwrap_or_default();
            if resp.message.content.is_empty() && !text.is_empty() {
                resp.message.content = text;
            }
            let result = seal_result(
                &state,
                &envelope,
                resp.message,
                GenUsageSerde::from(&resp.usage),
                JobStatus::Ok,
                None,
                Some(local_durations(queue_ms, started)),
            );
            let body = json!({
                "ok": true,
                "result": result,
                "result_signing_public_key": hex::encode(&state.result_public_key),
                "duration_ms": started.elapsed().as_millis(),
            });
            respond_terminal(
                &state,
                &envelope,
                TerminalOutcome::Completed,
                StatusCode::OK,
                body,
            )
            .await
        }
        Err(ChatRunError::Preempted) => {
            let result = seal_result(
                &state,
                &envelope,
                Message::assistant(""),
                GenUsageSerde::default(),
                JobStatus::Preempted,
                Some("owner_preempt".into()),
                Some(local_durations(queue_ms, started)),
            );
            let body = json!({
                "ok": false,
                "error": "owner_preempt",
                "result": result,
                "result_signing_public_key": hex::encode(&state.result_public_key),
            });
            respond_terminal(
                &state,
                &envelope,
                TerminalOutcome::Failed("owner_preempt".into()),
                StatusCode::OK,
                body,
            )
            .await
        }
        Err(ChatRunError::Inference(e)) => {
            let result = seal_result(
                &state,
                &envelope,
                Message::assistant(""),
                GenUsageSerde::default(),
                JobStatus::Error,
                Some(e.to_string()),
                Some(local_durations(queue_ms, started)),
            );
            let body = json!({
                "ok": false,
                "error": e.to_string(),
                "result": result,
                "result_signing_public_key": hex::encode(&state.result_public_key),
            });
            respond_terminal(
                &state,
                &envelope,
                TerminalOutcome::Failed(e.to_string()),
                StatusCode::OK,
                body,
            )
            .await
        }
    }
}

async fn chat_ndjson(
    state: Arc<FabricState>,
    job: FabricJob,
    envelope: lokai_fabric_protocol::JobEnvelope,
) -> Response<FabricBody> {
    let job_id = job.job_id.clone();
    let attempt_id = job.attempt_id.clone();
    let queue_started = std::time::Instant::now();
    let cancel_rx = match state.scheduler.acquire(&job_id, job.priority).await {
        Ok(rx) => rx,
        Err(err) => return scheduler_refuse(&state, &envelope, err).await,
    };

    let queue_ms = queue_started.elapsed().as_millis() as u64;
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
    let state2 = state.clone();
    let envelope2 = envelope.clone();
    tokio::spawn(async move {
        let chat_req = ChatRequest {
            model: job.model.clone(),
            model_digest: None,
            messages: job.messages,
            tools: job.tools,
            temperature: job.options.temperature,
            num_ctx: job.options.num_ctx,
            keep_alive: Some("10m".into()),
            fabric: None,
            response_format: job.options.response_format.clone(),
            outbound_scan: OutboundScan::from_scan(false),
            ..Default::default()
        };
        let job_id_run = job_id.clone();
        let attempt_id_run = attempt_id.clone();
        let token_tx = tx.clone();
        let started = std::time::Instant::now();
        let run = run_cancellable_chat(
            state2.inference.clone(),
            chat_req,
            cancel_rx,
            move |delta| {
                let line = json!({
                    "event": "token",
                    "job_id": job_id_run,
                    "attempt_id": attempt_id_run,
                    "delta": delta
                })
                .to_string()
                    + "\n";
                let _ = token_tx.try_send(line);
            },
        )
        .await;
        state2.scheduler.release(&job_id).await;
        if let Some(aid) = &attempt_id {
            let mut map = state2.attempt_jobs.lock().await;
            map.remove(aid);
            let mut leases = state2.leases.lock().await;
            leases.remove(aid);
        }
        let line = match run {
            Ok(resp) => {
                let result = seal_result(
                    &state2,
                    &envelope2,
                    resp.message,
                    GenUsageSerde::from(&resp.usage),
                    JobStatus::Ok,
                    None,
                    Some(local_durations(queue_ms, started)),
                );
                let cache = json!({
                    "ok": true,
                    "result": &result,
                    "result_signing_public_key": hex::encode(&state2.result_public_key),
                    "duration_ms": started.elapsed().as_millis(),
                });
                mark_stream_terminal(&state2, &envelope2, TerminalOutcome::Completed, cache).await;
                json!({
                    "event": "done",
                    "ok": true,
                    "job_id": job_id,
                    "attempt_id": attempt_id,
                    "result": result,
                    "result_signing_public_key": hex::encode(&state2.result_public_key),
                    "duration_ms": started.elapsed().as_millis(),
                })
            }
            Err(ChatRunError::Preempted) => {
                let result = seal_result(
                    &state2,
                    &envelope2,
                    Message::assistant(""),
                    GenUsageSerde::default(),
                    JobStatus::Preempted,
                    Some("owner_preempt".into()),
                    Some(local_durations(queue_ms, started)),
                );
                let cache = json!({
                    "ok": false,
                    "error": "owner_preempt",
                    "result": result,
                    "result_signing_public_key": hex::encode(&state2.result_public_key),
                });
                mark_stream_terminal(
                    &state2,
                    &envelope2,
                    TerminalOutcome::Failed("owner_preempt".into()),
                    cache,
                )
                .await;
                json!({
                    "event": "done",
                    "ok": false,
                    "job_id": job_id,
                    "error": "owner_preempt",
                    "result": result,
                    "result_signing_public_key": hex::encode(&state2.result_public_key),
                })
            }
            Err(ChatRunError::Inference(e)) => {
                let result = seal_result(
                    &state2,
                    &envelope2,
                    Message::assistant(""),
                    GenUsageSerde::default(),
                    JobStatus::Error,
                    Some(e.to_string()),
                    Some(local_durations(queue_ms, started)),
                );
                let cache = json!({
                    "ok": false,
                    "error": e.to_string(),
                    "result": result,
                    "result_signing_public_key": hex::encode(&state2.result_public_key),
                });
                mark_stream_terminal(
                    &state2,
                    &envelope2,
                    TerminalOutcome::Failed(e.to_string()),
                    cache,
                )
                .await;
                json!({
                    "event": "done",
                    "ok": false,
                    "job_id": job_id,
                    "error": e.to_string(),
                    "result": result,
                    "result_signing_public_key": hex::encode(&state2.result_public_key),
                })
            }
        };
        let _ = tx.send(line.to_string() + "\n").await;
    });

    let body = StreamBody::new(NdjsonLineStream { rx });
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-ndjson")
        .body(body.map_err(|never: Infallible| match never {}).boxed())
        .unwrap()
}

struct NdjsonLineStream {
    rx: tokio::sync::mpsc::Receiver<String>,
}

impl Stream for NdjsonLineStream {
    type Item = Result<Frame<Bytes>, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(line)) => Poll::Ready(Some(Ok(Frame::data(Bytes::from(line))))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

enum ChatRunError {
    Preempted,
    Inference(InferenceError),
}

async fn run_cancellable_chat(
    inference: Arc<dyn InferenceProvider + Send + Sync>,
    chat_req: ChatRequest,
    mut cancel_rx: watch::Receiver<bool>,
    mut on_token: impl FnMut(&str) + Send,
) -> Result<lokai_inference::ChatResponse, ChatRunError> {
    let cancel_rx2 = cancel_rx.clone();
    let chat = async move {
        InferenceProvider::chat(inference.as_ref(), chat_req, &mut |t| {
            if *cancel_rx2.borrow() {
                return;
            }
            on_token(t);
        })
        .await
    };

    tokio::pin!(chat);

    loop {
        tokio::select! {
            res = &mut chat => {
                return match res {
                    Ok(resp) => Ok(resp),
                    Err(e) => Err(ChatRunError::Inference(e)),
                };
            }
            changed = cancel_rx.changed() => {
                if changed.is_ok() && *cancel_rx.borrow() {
                    return Err(ChatRunError::Preempted);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use http_body_util::BodyExt;
    use hyper::StatusCode;
    use lokai_inference::{
        ChatRequest, ChatResponse, DataClass, FabricJob, GenUsage, InferenceError,
        InferenceProvenance, InferenceProvider, Message, TokenSink,
    };

    use crate::fabric::{default_ollama, FabricState};
    use crate::job_ingress::JobIngressManager;
    use crate::scheduler::WorkerScheduler;
    use crate::trust::TrustStore;

    use super::chat_json;

    struct MockInference;

    #[async_trait]
    impl InferenceProvider for MockInference {
        async fn chat(
            &self,
            _req: ChatRequest,
            on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            on_token("mock");
            Ok(ChatResponse {
                message: Message::assistant("mock-reply"),
                usage: GenUsage::default(),
                provenance: InferenceProvenance {
                    provider_kind: "mock".into(),
                    worker_id: None,
                    model: "mock".into(),
                    job_id: None,
                    attempt_id: None,
                    prompt_redacted: false,
                    ..Default::default()
                },
            })
        }
    }

    fn test_state(inference: Arc<dyn InferenceProvider + Send + Sync>) -> Arc<FabricState> {
        let ollama = default_ollama("http://127.0.0.1:1");
        let db_path = std::env::temp_dir().join(format!(
            "lokai_node_test_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let result_signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let result_public_key = result_signing_key.verifying_key().to_bytes().to_vec();
        let result_key_id = lokai_fabric_client::key_id_from_public(&result_public_key);
        Arc::new(FabricState {
            peer_id: "test_peer".into(),
            coordinator_pk: [1u8; 32],
            inference,
            ollama,
            ollama_base: "http://127.0.0.1:1".into(),
            trust: Arc::new(
                TrustStore::from_pinned_pubkeys(std::iter::once([1u8; 32].to_vec())).unwrap(),
            ),
            worker_db_path: db_path.clone(),
            estate_id: "estate".into(),
            scheduler: Arc::new(WorkerScheduler::new()),
            job_ingress: JobIngressManager::load(db_path),
            boot_id: "boot_test".into(),
            capability_revision: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            inventory_fingerprint: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            result_signing_key: std::sync::Arc::new(result_signing_key),
            result_key_id,
            result_public_key,
            cancel_state: Arc::new(tokio::sync::Mutex::new(
                lokai_fabric_protocol::CancellationState::default(),
            )),
            attempt_jobs: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            leases: Arc::new(tokio::sync::Mutex::new(
                crate::lease_table::LeaseTable::default(),
            )),
        })
    }

    #[tokio::test]
    async fn jobs_cancel_signals_scheduler_for_mapped_attempt() {
        use lokai_domain::ids::{AttemptId, LeaseId, RunId, TaskId};
        use lokai_fabric_protocol::{
            CancellationAcknowledged, CancellationRequest, IdempotencyKey,
        };

        let state = test_state(Arc::new(MockInference));
        let job_id = "job_cancel_1";
        let attempt_id = "att_cancel_1";
        {
            let mut map = state.attempt_jobs.lock().await;
            map.insert(attempt_id.into(), job_id.into());
        }
        let mut rx = state
            .scheduler
            .acquire(job_id, lokai_inference::JobPriority::OwnerInteractive)
            .await
            .unwrap();

        // Production cancel path: attempt_jobs maps CancellationRequest.attempt_id → scheduler job_id.
        let mapped = {
            let map = state.attempt_jobs.lock().await;
            map.get(attempt_id).cloned()
        };
        assert_eq!(mapped.as_deref(), Some(job_id));
        assert!(state.scheduler.cancel_job(mapped.as_deref().unwrap()).await);
        assert!(*rx.borrow_and_update());

        let cancel = CancellationRequest {
            run_id: RunId::new("run"),
            task_id: TaskId::new(attempt_id),
            attempt_id: AttemptId::new(attempt_id),
            lease_id: LeaseId::new(attempt_id),
            lease_epoch: 1,
            reason: "test".into(),
            deadline: chrono::Utc::now(),
        };
        let mut cs = state.cancel_state.lock().await;
        assert!(cs
            .acknowledge(
                &cancel,
                CancellationAcknowledged {
                    context: lokai_fabric_protocol::LifecycleContext {
                        job_id: lokai_domain::ids::JobId::new(job_id),
                        task_id: TaskId::new(attempt_id),
                        attempt_id: AttemptId::new(attempt_id),
                        lease_id: LeaseId::new(attempt_id),
                        lease_epoch: 1,
                        worker_id: lokai_domain::ids::WorkerId::new("test_peer"),
                        sequence_number: 1,
                        protocol_version: lokai_fabric_protocol::ProtocolVersion(1),
                        idempotency_key: IdempotencyKey("c".into()),
                    },
                },
            )
            .unwrap());
        assert!(cs.can_renew_lease(&AttemptId::new(attempt_id), 1).is_err());
    }

    #[tokio::test]
    async fn lease_expiry_without_renewal_cancels_running_job() {
        std::env::set_var("LOKAI_FABRIC_LEASE_TTL_MS", "200");
        let state = test_state(Arc::new(MockInference));
        let job_id = "job_lease_exp";
        let attempt_id = "att_lease_exp";
        {
            let mut map = state.attempt_jobs.lock().await;
            map.insert(attempt_id.into(), job_id.into());
            let mut leases = state.leases.lock().await;
            leases.insert(crate::lease_table::ActiveLease {
                job_id: job_id.into(),
                attempt_id: attempt_id.into(),
                lease_epoch: 1,
                expires_at: std::time::Instant::now() + std::time::Duration::from_millis(200),
            });
        }
        let rx = state
            .scheduler
            .acquire(job_id, lokai_inference::JobPriority::OwnerInteractive)
            .await
            .unwrap();
        super::spawn_lease_expiry_watch(state.clone(), job_id.into(), attempt_id.into());
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(
            *rx.borrow(),
            "lease expiry must signal cancel on the scheduler watch channel"
        );
        std::env::remove_var("LOKAI_FABRIC_LEASE_TTL_MS");
    }

    #[tokio::test]
    async fn chat_json_uses_mock_inference() {
        let state = test_state(Arc::new(MockInference));
        let job = FabricJob {
            job_id: "job_1".into(),
            attempt_id: None,
            estate_id: "estate".into(),
            session_id: None,
            agent_id: "a".into(),
            step_index: 0,
            model: "m".into(),
            tier: None,
            messages: vec![],
            tools: vec![],
            options: Default::default(),
            priority: lokai_inference::JobPriority::OwnerInteractive,
            data_class: DataClass::RepositorySource,
            disclosure_tier: Default::default(),
            audit_envelope: None,
            circle_id: None,
            consumer_peer_id: None,
            policy_epoch: 0,
            turn_affinity: None,
        };
        let envelope = lokai_fabric_client::fabric_job_to_envelope(&job).unwrap();
        let _ = state.job_ingress.accept(&envelope).await.unwrap();
        let resp = chat_json(state, job, envelope).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["result"]["message"]["content"], "mock-reply");
    }

    #[tokio::test]
    async fn scheduler_refuse_after_accept_is_not_duplicate_in_flight() {
        let state = test_state(Arc::new(MockInference));
        state.scheduler.signal_owner_activity_default().await;
        let job = FabricJob {
            job_id: "job_circle".into(),
            attempt_id: Some("att_circle".into()),
            estate_id: "estate".into(),
            session_id: None,
            agent_id: "a".into(),
            step_index: 0,
            model: "m".into(),
            tier: None,
            messages: vec![],
            tools: vec![],
            options: Default::default(),
            priority: lokai_inference::JobPriority::CircleBackground,
            data_class: DataClass::RepositorySource,
            disclosure_tier: Default::default(),
            audit_envelope: None,
            circle_id: None,
            consumer_peer_id: None,
            policy_epoch: 0,
            turn_affinity: None,
        };
        let envelope = lokai_fabric_client::fabric_job_to_envelope(&job).unwrap();
        assert_eq!(
            state.job_ingress.accept(&envelope).await.unwrap(),
            lokai_fabric_protocol::IngressDecision::AcceptNew
        );
        let resp = chat_json(state.clone(), job, envelope.clone()).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            !matches!(
                state.job_ingress.accept(&envelope).await.unwrap(),
                lokai_fabric_protocol::IngressDecision::DuplicateInFlight
            ),
            "scheduler refuse must mark_terminal"
        );
    }
}
