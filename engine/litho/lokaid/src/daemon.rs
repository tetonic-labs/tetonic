//! Daemon state + RPC dispatch. Handlers live under `daemon/handlers/`; integration
//! tests under `daemon/tests.rs` (mock provider, no Ollama).

mod config;
mod events;
mod handlers;
mod helpers;
mod placement;
mod rpc;
mod types;

#[cfg(test)]
mod tests;

use config::{
    rpc_auth_disabled, rpc_control_plane_mutations_allowed, rpc_token_from_env_or_generate,
};
use types::{CapacityJobRuntime, EngineServices};

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::Value;

use tetonic_rpc::protocol::*;
use tetonic_rpc::Notifier;

/// Outcome of one RPC dispatch. `Stop` means authenticated shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    Continue,
    Stop,
}

pub struct Daemon {
    notifier: Notifier,
    services: Option<EngineServices>,
    capacity: CapacityJobRuntime,
    /// Reject new chat turns; cancel in-flight work on shutdown (AR1-1).
    draining: Arc<AtomicBool>,
    in_flight: Arc<AtomicU32>,
    /// Combined mode: stops fabric accept loop on shutdown (SEC2-E2-007).
    fabric_shutdown: Option<Arc<AtomicBool>>,
    /// Random token required on `initialize` (SEC-003).
    rpc_token: String,
    rpc_authenticated: AtomicBool,
    scanner: Option<Arc<tetonic_app::ScannerEngine>>,
}

impl Daemon {
    pub fn new(
        notifier: Notifier,
        fabric_shutdown: Option<Arc<AtomicBool>>,
        scanner: Option<Arc<tetonic_app::ScannerEngine>>,
    ) -> Self {
        Self {
            notifier,
            services: None,
            capacity: CapacityJobRuntime::default(),
            draining: Arc::new(AtomicBool::new(false)),
            in_flight: Arc::new(AtomicU32::new(0)),
            fabric_shutdown,
            rpc_token: rpc_token_from_env_or_generate(),
            rpc_authenticated: AtomicBool::new(rpc_auth_disabled()),
            scanner,
        }
    }

    /// RPC session token for editor handoff (SEC-003). Tests assert it; production does not print it.
    #[cfg(test)]
    pub fn rpc_token(&self) -> &str {
        &self.rpc_token
    }

    /// Best-effort drain of in-flight turns before process exit (AR1-1).
    pub async fn shutdown(&self) {
        self.draining.store(true, Ordering::Relaxed);
        if let Some(stop) = &self.fabric_shutdown {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(services) = &self.services {
            services.app.sessions.cancel_all().await;
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.in_flight.load(Ordering::Relaxed) > 0 {
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    remaining = self.in_flight.load(Ordering::Relaxed),
                    "shutdown drain timeout — exiting with in-flight turn(s)"
                );
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// Dispatch one request and send its response. `chat/send` returns
    /// immediately; its work streams as notifications.
    pub async fn handle(&mut self, msg: Incoming, id: Value) -> Dispatch {
        self.apply_pending_capacity_reload();
        if !rpc_auth_disabled() {
            if msg.method == methods::INITIALIZE {
                // validated inside initialize()
            } else if !self.rpc_authenticated.load(Ordering::Relaxed) {
                self.notifier.respond(&Response::err(
                    id,
                    RpcError::new(ErrorCode::NotReady, "call initialize with rpc_token first"),
                ));
                return Dispatch::Continue;
            }
        }
        if msg.method == methods::SHUTDOWN {
            self.notifier
                .respond(&Response::ok(id, serde_json::json!({ "ok": true })));
            return Dispatch::Stop;
        }
        let result = match msg.method.as_str() {
            methods::INITIALIZE => self.initialize(msg.params).await,
            methods::SESSION_START => self.session_start(msg.params).await,
            methods::SESSION_RECLASSIFY => self.session_reclassify(msg.params).await,
            methods::SESSION_INFERENCE => self.session_inference(msg.params, false),
            methods::SESSION_MODELS => self.session_models(msg.params),
            methods::SESSION_SELECT_MODEL => self.session_select_model(msg.params),
            methods::SESSION_SET_INFERENCE => self.session_inference(msg.params, true),
            methods::SESSION_END => self.session_end(msg.params),
            methods::CHAT_SEND => self.chat_send(msg.params),
            methods::SESSION_CANCEL => self.session_cancel(msg.params).await,
            methods::APPROVAL_RESPOND => self.approval_respond(msg.params),
            methods::MODEL_LIST => self.model_list(),
            methods::EGRESS_POLICY_GET => self.egress_policy_get(),
            methods::EGRESS_POLICY_SET => self.egress_policy_set(msg.params),
            methods::POLICY_GET => self.policy_get().await,
            methods::POLICY_SET => self.policy_set(msg.params).await,
            methods::ESTATE_STATUS => self.estate_status(),
            methods::ESTATE_CAPACITY_STATUS => self.estate_capacity_status().await,
            methods::ESTATE_CAPACITY_DOCTOR => self.estate_capacity_doctor().await,
            methods::ESTATE_CAPACITY_OPTIMIZE => self.estate_capacity_optimize(msg.params).await,
            methods::ESTATE_CAPACITY_CANCEL | methods::ESTATE_CAPACITY_JOBS_CANCEL => {
                self.estate_capacity_cancel(msg.params)
            }
            methods::ESTATE_CAPACITY_PROFILES_LIST => {
                self.estate_capacity_profiles_list(msg.params)
            }
            methods::ESTATE_CAPACITY_PROFILES_ACTIVATE => {
                self.estate_capacity_profiles_activate(msg.params).await
            }
            methods::ESTATE_CAPACITY_PROFILES_ROLLBACK => {
                self.estate_capacity_profiles_rollback(msg.params).await
            }
            methods::ESTATE_CAPACITY_PROFILES_EXPORT => {
                self.estate_capacity_profiles_export(msg.params)
            }
            methods::ESTATE_CAPACITY_JOBS_GET => self.estate_capacity_jobs_get(msg.params),
            methods::FABRIC_STATUS => self.fabric_status().await,
            methods::FABRIC_WORKER_TRUST_SET => self.fabric_worker_trust_set(msg.params).await,
            methods::FABRIC_WORKER_TRUST_GET => self.fabric_worker_trust_get(msg.params),
            methods::AGENT_SPAWN => self.agent_spawn(msg.params),
            methods::PROJECT_CONSOLIDATE => self.project_consolidate(msg.params),
            methods::RUN_SNAPSHOT => self.run_snapshot(msg.params).await,
            methods::RUN_RESUME => self.run_resume(msg.params).await,
            methods::RUN_CANCEL => self.run_cancel(msg.params).await,
            methods::SECRET_RULE_ADD => {
                if !rpc_control_plane_mutations_allowed() {
                    Err(RpcError::new(
                        ErrorCode::InvalidRequest,
                        "secret/rule.add is disabled when LOKAI_STRICT_RPC is on — use lokai CLI",
                    ))
                } else if let (Some(scanner), Ok(services)) = (&self.scanner, self.services()) {
                    match serde_json::from_value::<tetonic_rpc::protocol::AddSecretRuleRequest>(
                        msg.params,
                    ) {
                        Ok(req) => handlers::secrets::handle_add_secret_rule(
                            services.app.as_ref(),
                            scanner.as_ref(),
                            req,
                        ),
                        Err(_) => Err(RpcError::new(ErrorCode::InvalidParams, "Invalid request")),
                    }
                } else {
                    Err(RpcError::new(
                        ErrorCode::InternalError,
                        "Scanner not configured",
                    ))
                }
            }
            methods::SECRET_FINGERPRINT_ALLOW => {
                if !rpc_control_plane_mutations_allowed() {
                    Err(RpcError::new(
                        ErrorCode::InvalidRequest,
                        "secret/fingerprint.allow is disabled when LOKAI_STRICT_RPC is on",
                    ))
                } else if let (Some(scanner), Ok(services)) = (&self.scanner, self.services()) {
                    match serde_json::from_value::<
                        tetonic_rpc::protocol::AllowSecretFingerprintRequest,
                    >(msg.params)
                    {
                        Ok(req) => {
                            handlers::secrets::handle_allow_fingerprint(
                                services.app.as_ref(),
                                scanner.as_ref(),
                                req,
                            )
                            .await
                        }
                        Err(_) => Err(RpcError::new(ErrorCode::InvalidParams, "Invalid request")),
                    }
                } else {
                    Err(RpcError::new(
                        ErrorCode::InternalError,
                        "Scanner not configured",
                    ))
                }
            }
            methods::SECRET_FINGERPRINT_REVOKE => {
                if !rpc_control_plane_mutations_allowed() {
                    Err(RpcError::new(
                        ErrorCode::InvalidRequest,
                        "secret/fingerprint.revoke is disabled when LOKAI_STRICT_RPC is on",
                    ))
                } else if let (Some(scanner), Ok(services)) = (&self.scanner, self.services()) {
                    match serde_json::from_value::<
                        tetonic_rpc::protocol::RevokeSecretFingerprintRequest,
                    >(msg.params)
                    {
                        Ok(req) => {
                            handlers::secrets::handle_revoke_fingerprint(
                                services.app.as_ref(),
                                scanner.as_ref(),
                                req,
                            )
                            .await
                        }
                        Err(_) => Err(RpcError::new(ErrorCode::InvalidParams, "Invalid request")),
                    }
                } else {
                    Err(RpcError::new(
                        ErrorCode::InternalError,
                        "Scanner not configured",
                    ))
                }
            }
            other => Err(RpcError::new(
                ErrorCode::MethodNotFound,
                format!("unknown method '{other}'"),
            )),
        };
        match result {
            Ok(value) => self.notifier.respond(&Response::ok(id, value)),
            Err(e) => self.notifier.respond(&Response::err(id, e)),
        }
        Dispatch::Continue
    }
}
