use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tetonic_memory::WorkerStore;

use crate::limits::INGRESS_LOG_RETAIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IngressDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressEvent {
    pub ts: String,
    pub peer_id: Option<String>,
    pub remote_addr: String,
    pub decision: IngressDecision,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

impl IngressEvent {
    pub fn deny(remote: SocketAddr, reason: impl Into<String>) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            peer_id: None,
            remote_addr: remote.to_string(),
            decision: IngressDecision::Deny,
            reason: reason.into(),
            route: None,
            job_id: None,
        }
    }

    pub fn route_outcome(
        remote: SocketAddr,
        peer_id: String,
        route: String,
        decision: IngressDecision,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            peer_id: Some(peer_id),
            remote_addr: remote.to_string(),
            decision,
            reason: reason.into(),
            route: Some(route),
            job_id: None,
        }
    }
}

#[derive(Clone)]
pub struct IngressLog {
    inner: Arc<Mutex<Vec<IngressEvent>>>,
    db_path: Option<PathBuf>,
}

impl Default for IngressLog {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
            db_path: None,
        }
    }
}

impl IngressLog {
    /// Attach worker DB persistence (append + trim on record).
    pub fn with_db(&self, path: impl AsRef<Path>) -> Self {
        Self {
            inner: self.inner.clone(),
            db_path: Some(path.as_ref().to_path_buf()),
        }
    }

    pub fn record(&self, ev: IngressEvent) {
        tracing::info!(
            target: "ingress",
            remote = %ev.remote_addr,
            decision = ?ev.decision,
            reason = %ev.reason,
            route = ?ev.route,
            "ingress decision"
        );
        self.inner
            .lock()
            .expect("ingress log poisoned")
            .push(ev.clone());
        if let Some(path) = &self.db_path {
            let path = path.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || persist_event(&path, &ev)).await;
            });
        }
    }

    pub fn events(&self) -> Vec<IngressEvent> {
        self.inner.lock().expect("ingress log poisoned").clone()
    }
}

fn persist_event(path: &Path, ev: &IngressEvent) -> Result<(), tetonic_memory::WorkerStoreError> {
    let store = WorkerStore::open(path)?;
    let decision = match ev.decision {
        IngressDecision::Allow => "allow",
        IngressDecision::Deny => "deny",
    };
    store.append_ingress_event(
        &ev.ts,
        ev.peer_id.as_deref(),
        &ev.remote_addr,
        decision,
        &ev.reason,
        ev.route.as_deref(),
    )?;
    store.trim_ingress_log(INGRESS_LOG_RETAIN)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_outcome_carries_decision() {
        let remote: SocketAddr = "127.0.0.1:9471".parse().unwrap();
        let ev = IngressEvent::route_outcome(
            remote,
            "peer_a".into(),
            "/v1/chat".into(),
            IngressDecision::Deny,
            "policy_forbidden",
        );
        assert_eq!(ev.decision, IngressDecision::Deny);
        assert_eq!(ev.reason, "policy_forbidden");
    }
}
