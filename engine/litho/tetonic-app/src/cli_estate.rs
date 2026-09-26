//! Estate and Capacity facade methods for Application.
//!
//! Handles worker enrollment querying, worker trust, capacity diagnosis,
//! Ollama memory inspections, and egress controls.

use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tetonic_capacity::{
    format_capacity_report, CapacityDoctorStatus, OllamaInferenceClient, OptimizeOutcome,
    WorkerCapacityWire, LOCAL_NODE_ID,
};
use tetonic_domain::WorkerTrust;
use tetonic_egress::{Action, EgressEvent, EgressGuard};
use tetonic_fabric_client::fabric_request;
use tetonic_inference::OllamaProvider;

use crate::commands::{CapacityDoctorCommand, CapacityStatusCommand, RunOptimizeCommand};
use crate::errors::AppError;
use crate::Application;

#[derive(Debug, Clone)]
pub struct WorkerEnrollmentSummary {
    pub id: String,
    pub label: String,
    pub host: String,
    pub ip: String,
    pub fabric_port: u16,
    pub enrolled_at: String,
}

#[derive(Debug, Clone)]
pub struct WorkerTrustAuditSummary {
    pub trust: String,
    pub policy_epoch: u64,
    pub source: String,
    pub recorded_at: String,
}

impl Application {
    // --- Estate Fleet & Worker Trust ---

    pub fn list_worker_enrollments(&self) -> Result<Vec<WorkerEnrollmentSummary>, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        store
            .read_sync(|db| {
                let workers = db
                    .list_worker_enrollments()
                    .map_err(AppError::hide_store_failure)?;
                Ok(workers
                    .into_iter()
                    .map(|w| WorkerEnrollmentSummary {
                        id: w.id,
                        label: w.label,
                        host: w.host,
                        ip: w.ip,
                        fabric_port: w.fabric_port,
                        enrolled_at: w.enrolled_at,
                    })
                    .collect())
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn set_worker_trust(
        &self,
        id_or_label: &str,
        trust: &str,
    ) -> Result<(String, String, String, u64), AppError> {
        let parsed = WorkerTrust::parse(trust).ok_or_else(|| {
            AppError::InvalidRequest(format!(
                "unknown trust tier `{trust}` — use local_machine, owner_controlled_estate, administratively_managed, or external_untrusted"
            ))
        })?;
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let id_or_lbl = id_or_label.to_string();
        store
            .write_sync(move |db| {
                let row = db
                    .find_worker_enrollment(&id_or_lbl)
                    .map_err(AppError::hide_store_failure)?
                    .ok_or_else(|| {
                        AppError::InvalidRequest(format!("worker not found: {id_or_lbl}"))
                    })?;
                let epoch = db
                    .max_worker_trust_policy_epoch()
                    .unwrap_or(0)
                    .saturating_add(1);
                if !db
                    .set_worker_trust(&row.id, parsed.as_str(), epoch, "cli")
                    .map_err(AppError::hide_store_failure)?
                {
                    return Err(AppError::InvalidRequest(format!(
                        "worker not found: {id_or_lbl}"
                    )));
                }
                Ok((row.id, row.label, parsed.as_str().to_string(), epoch))
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn get_worker_trust(
        &self,
        id_or_label: &str,
    ) -> Result<(String, String, String, Vec<WorkerTrustAuditSummary>), AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let id_or_lbl = id_or_label.to_string();
        store
            .read_sync(move |db| {
                let row = db
                    .find_worker_enrollment(&id_or_lbl)
                    .map_err(AppError::hide_store_failure)?
                    .ok_or_else(|| {
                        AppError::InvalidRequest(format!("worker not found: {id_or_lbl}"))
                    })?;
                let trust = db
                    .worker_trust(&row.id)
                    .map_err(AppError::hide_store_failure)?
                    .unwrap_or_else(|| "owner_controlled_estate".into());
                let audit = db
                    .list_worker_trust_audit(&row.id)
                    .map_err(AppError::hide_store_failure)?
                    .into_iter()
                    .map(|a| WorkerTrustAuditSummary {
                        trust: a.trust,
                        policy_epoch: a.policy_epoch,
                        source: a.source,
                        recorded_at: a.recorded_at,
                    })
                    .collect();
                Ok((row.id, row.label, trust, audit))
            })
            .map_err(AppError::hide_store_failure)?
    }

    // --- Egress Persistent Rules ---

    pub fn upsert_egress_allow_rule(
        &self,
        label: &str,
        ip: &str,
        port: Option<u16>,
    ) -> Result<(), AppError> {
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            self.turn.guard().allow_node(label, ip_addr, port);
        }
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let l = label.to_string();
        let i = ip.to_string();
        store
            .write_sync(move |db| {
                db.upsert_egress_allow_rule(&l, &i, port)
                    .map_err(AppError::hide_store_failure)
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn remove_egress_allow_rule(&self, label: &str) -> Result<bool, AppError> {
        self.turn.guard().remove_allow_label(label);
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let l = label.to_string();
        store
            .write_sync(move |db| {
                db.remove_egress_allow_rule(&l)
                    .map_err(AppError::hide_store_failure)
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn egress_allow(&self, label: &str, ip: IpAddr, port: Option<u16>) {
        self.turn.guard().allow_node(label, ip, port);
    }

    pub fn egress_allow_cidr(
        &self,
        label: &str,
        cidr: &str,
        port: Option<u16>,
    ) -> Result<(), AppError> {
        // Parse IP or fallback
        if let Ok(ip) = cidr.parse::<IpAddr>() {
            self.turn.guard().allow_node(label, ip, port);
            return Ok(());
        }
        Err(AppError::InvalidRequest(format!(
            "invalid IP address: {cidr}"
        )))
    }

    pub fn reload_enrollment_egress(&self) -> Result<(), AppError> {
        if let Some(store) = self.store() {
            let guard = self.turn.guard();
            store
                .read_sync(move |db| crate::estate_enrollment::reload_enrollment_egress(db, &guard))
                .map_err(AppError::hide_store_failure)?
        } else {
            Ok(())
        }
    }

    pub fn egress_activity_log(&self) -> Vec<EgressEvent> {
        self.turn.guard().activity_log()
    }

    pub fn record_egress_and_consolidate(&self, session_id: &str) -> Result<(), AppError> {
        let log = self.egress_activity_log();
        if let Some(store) = self.store() {
            let sid = session_id.to_string();
            let sid_c = session_id.to_string();
            store
                .write_sync(move |db| {
                    for ev in log {
                        let decision = match ev.decision {
                            Action::Allow => "allow",
                            Action::Deny => "deny",
                        };
                        let _ = db.record_egress(
                            Some(&sid),
                            &ev.ts,
                            &ev.initiator,
                            &ev.host,
                            ev.resolved_ip.as_deref(),
                            ev.port,
                            decision,
                            Some(&ev.reason),
                        );
                    }
                    let _ = db.consolidate_session(&sid_c);
                    Ok::<(), anyhow::Error>(())
                })
                .map_err(AppError::hide_store_failure)?
                .map_err(AppError::hide_store_failure)?;
        }
        Ok(())
    }

    // --- Capacity Reporting and Diagnosis ---

    pub async fn fetch_worker_capacity(
        &self,
        worker_id: &str,
    ) -> Result<WorkerCapacityWire, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let wid = worker_id.to_string();
        let (w, cert, coordinator_kp) = store
            .read_sync(move |db| {
                let workers = db
                    .list_worker_enrollments()
                    .map_err(AppError::hide_store_failure)?;
                let w = workers
                    .into_iter()
                    .find(|x| x.id == wid || x.label == wid)
                    .ok_or_else(|| AppError::InvalidRequest(format!("unknown worker `{wid}`")))?;
                let cert = w
                    .fabric_tls_cert
                    .as_ref()
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| {
                        AppError::InvalidRequest(format!(
                            "worker `{}` has no fabric TLS cert",
                            w.label
                        ))
                    })?
                    .clone();
                let dir = directories::ProjectDirs::from("", "", "lokai")
                    .ok_or_else(|| AppError::InvalidRequest("no data dir".into()))?
                    .data_dir()
                    .to_path_buf();
                let kp = crate::estate_enrollment::load_or_create_coordinator(db, &dir)?;
                Ok::<_, AppError>((w, cert, kp))
            })
            .map_err(AppError::hide_store_failure)??;

        let coordinator = Arc::new(coordinator_kp);
        let guard = Arc::new(EgressGuard::new());
        if let Some(s) = self.store() {
            let g = guard.clone();
            let _ =
                s.read_sync(move |db| crate::estate_enrollment::reload_enrollment_egress(db, &g));
        }
        let ip: IpAddr = w
            .ip
            .parse()
            .map_err(|e| AppError::InvalidRequest(format!("invalid worker ip {}: {e}", w.ip)))?;
        let resp = fabric_request(
            &guard,
            ip,
            w.fabric_port,
            &cert,
            &coordinator,
            "GET",
            "/v1/capacity/status",
            None,
            "capacity:worker:status",
        )
        .await
        .map_err(|e| AppError::InvalidRequest(format!("fabric request: {e}")))?;

        if resp.status != 200 {
            return Err(AppError::InvalidRequest(format!(
                "worker capacity/status HTTP {} — {}",
                resp.status,
                resp.body.trim()
            )));
        }
        serde_json::from_str(&resp.body)
            .map_err(|e| AppError::InvalidRequest(format!("parse worker capacity JSON: {e}")))
    }

    pub async fn get_capacity_status_report(
        &self,
        worker: Option<&str>,
        session_model: Option<&str>,
    ) -> Result<String, AppError> {
        if let Some(wid) = worker {
            let wire = self.fetch_worker_capacity(wid).await?;
            let mut out = format!(
                "Worker `{wid}` capacity:\n  completed: {}\n  doctor:    {}\n  stale:     {}\n  gates_ok:  {}\n",
                wire.completed, wire.doctor, wire.stale, wire.gates_ok
            );
            if let Some(ref id) = wire.active_profile_id {
                out.push_str(&format!("  profile:   {id}\n"));
            }
            if let Some(ref label) = wire.active_profile_label {
                out.push_str(&format!("  label:     {label}\n"));
            }
            if let Some(ref hw) = wire.hardware_summary {
                out.push_str(&format!("  hardware:  {hw}\n"));
            }
            return Ok(out);
        }

        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = Arc::new(OllamaProvider::new(&base, guard));
        let client: Arc<dyn tetonic_capacity::InferenceClient> =
            Arc::new(OllamaInferenceClient::new(&base, provider));
        let ver = client.version().await;
        let result = self
            .capacity
            .get_capacity_status(CapacityStatusCommand {
                client,
                node_id: LOCAL_NODE_ID.to_string(),
                ollama_version: ver,
            })
            .await?;
        Ok(format_capacity_report(&result.status, None, session_model))
    }

    pub async fn get_capacity_doctor_report(
        &self,
        worker: Option<&str>,
        session_model: Option<&str>,
    ) -> Result<(String, bool), AppError> {
        if let Some(wid) = worker {
            let wire = self.fetch_worker_capacity(wid).await?;
            let mut out = format!(
                "Worker `{wid}` capacity:\n  completed: {}\n  doctor:    {}\n  stale:     {}\n  gates_ok:  {}\n",
                wire.completed, wire.doctor, wire.stale, wire.gates_ok
            );
            if let Some(ref id) = wire.active_profile_id {
                out.push_str(&format!("  profile:   {id}\n"));
            }
            out.push_str(&format!(
                "\nRemote capacity doctor: {} (gates_ok={})\n",
                wire.doctor, wire.gates_ok
            ));
            let ok = wire.doctor != "degraded" && wire.gates_ok;
            return Ok((out, ok));
        }

        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = Arc::new(OllamaProvider::new(&base, guard));
        let client: Arc<dyn tetonic_capacity::InferenceClient> =
            Arc::new(OllamaInferenceClient::new(&base, provider));
        let ver = client.version().await;
        let result = self
            .capacity
            .get_capacity_doctor(CapacityDoctorCommand {
                client,
                node_id: LOCAL_NODE_ID.to_string(),
                ollama_version: ver,
            })
            .await?;
        let ok = result.diagnosis.status != CapacityDoctorStatus::Degraded;
        let text = format_capacity_report(&result.status, Some(&result.diagnosis), session_model);
        Ok((text, ok))
    }

    pub async fn run_capacity_optimize(&self, depth: &str) -> Result<OptimizeOutcome, AppError> {
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = Arc::new(OllamaProvider::new(&base, guard));
        let client: Arc<dyn tetonic_capacity::InferenceClient> =
            Arc::new(OllamaInferenceClient::new(&base, provider));
        let cancel = AtomicBool::new(false);
        self.capacity
            .run_optimize(
                RunOptimizeCommand {
                    sessions_busy: false,
                    capacity_busy: false,
                    depth: depth.to_string(),
                    auto_apply: true,
                    client,
                    prebegin: None,
                },
                &cancel,
            )
            .await
    }

    // --- Ollama PS / Evict ---

    pub async fn ollama_ps(&self) -> Result<String, AppError> {
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let url = format!("{}/api/ps", base.trim_end_matches('/'));
        let v: serde_json::Value = guard
            .get_json(&url, "inference:ollama:ps")
            .await
            .map_err(|e| AppError::InvalidRequest(format!("Failed to query Ollama: {e}")))?;
        Ok(format_ps(&v))
    }

    pub async fn ollama_evict(&self) -> Result<String, AppError> {
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let ps_url = format!("{}/api/ps", base.trim_end_matches('/'));
        let v: serde_json::Value = guard
            .get_json(&ps_url, "inference:ollama:ps")
            .await
            .map_err(|e| AppError::InvalidRequest(format!("Failed to query Ollama: {e}")))?;
        let Some(models) = v.get("models").and_then(|m| m.as_array()) else {
            return Err(AppError::InvalidRequest(
                "Ollama residency status unavailable".into(),
            ));
        };
        if models.is_empty() {
            return Ok("No models to evict.\n".into());
        }
        let mut report = String::new();
        let provider = tetonic_inference::OllamaProvider::new(base.trim_end_matches('/'), guard);
        let mut failed = 0;
        for m in models {
            let Some(name) = m.get("name").and_then(|v| v.as_str()) else {
                failed += 1;
                continue;
            };
            match provider.unload_model(name).await {
                Ok(()) => report.push_str(&format!("Unloaded {name} (release verified).\n")),
                Err(e) => {
                    failed += 1;
                    report.push_str(&format!("evict {name} failed: {e}\n"));
                }
            }
        }
        if failed == 0 {
            report.push_str("Eviction complete.\n");
        } else {
            report.push_str(&format!(
                "Eviction incomplete: {failed} runner(s) could not be released.\n"
            ));
        }
        Ok(report)
    }
}

fn format_ps(v: &serde_json::Value) -> String {
    let Some(models) = v.get("models").and_then(|m| m.as_array()) else {
        return "Ollama residency status unavailable.\n".into();
    };
    if models.is_empty() {
        return "No models currently loaded in Ollama.\n".into();
    }
    let mut display = format!(
        "{:<25} {:<10} {:<10} {:<7} {:<10} {}\n",
        "NAME", "SIZE", "VRAM", "GPU%", "CONTEXT", "UNTIL"
    );
    display.push_str(&"-".repeat(60));
    display.push('\n');
    for m in models {
        let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
        let size = m.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let vram = m.get("size_vram").and_then(|v| v.as_u64()).unwrap_or(0);
        let expires = m
            .get("expires_at")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let gpu_pct = tetonic_capacity::gpu_residency_pct_from_ps(size, vram)
            .map(|pct| format!("{pct:.0}%"))
            .unwrap_or_else(|| "unknown".into());
        let context = m
            .get("context_length")
            .and_then(serde_json::Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".into());
        display.push_str(&format!(
            "{:<25} {:<10} {:<10} {:<7} {:<10} {}\n",
            name,
            format!("{}MB", size / 1024 / 1024),
            format!("{}MB", vram / 1024 / 1024),
            gpu_pct,
            context,
            expires
        ));
    }
    display
}

#[cfg(test)]
mod residency_tests {
    use super::*;
    #[test]
    fn ps_distinguishes_unknown_from_empty_and_shows_context_and_gpu_share() {
        assert!(format_ps(&serde_json::json!({"error":"unavailable"})).contains("unavailable"));
        assert!(format_ps(&serde_json::json!({"models":[]})).contains("No models"));
        let report = format_ps(&serde_json::json!({"models":[{
            "name":"model", "size":100, "size_vram":25, "context_length":262144
        }]}));
        assert!(report.contains("25%"));
        assert!(report.contains("262144"));
    }
}
