//! Bridges inference dispatch placement reports to JSON-RPC notifications.

use lokai_app::{data_class_name, DispatchPlacementReport, DispatchPlacementSink, ROOT_AGENT};
use lokai_rpc::protocol::events;
use lokai_rpc::Notifier;
use serde_json::json;

pub struct DaemonPlacementSink {
    notifier: Notifier,
}

impl DaemonPlacementSink {
    pub fn new(notifier: Notifier) -> Self {
        Self { notifier }
    }
}

impl DispatchPlacementSink for DaemonPlacementSink {
    fn report(&self, report: DispatchPlacementReport) {
        let session_id = report
            .session_id
            .clone()
            .unwrap_or_else(|| "unknown".into());
        let agent_id = report.agent_id.as_deref().unwrap_or(ROOT_AGENT);
        let mut payload = json!({
            "target": report.target,
            "decision": report.decision_str(),
            "redacted": report.redacted,
        });
        if let Some(code) = report.reason_code {
            payload["reason_code"] = json!(code);
        }
        if let Some(reason) = &report.reason {
            payload["reason"] = json!(reason);
        }
        if let Some(class) = report.effective_class {
            payload["data_class"] = json!(data_class_name(class));
        }
        if let Some(summary) = &report.classification {
            payload["classification"] = json!({
                "data_class": data_class_name(summary.class),
                "sources": summary.sources,
                "policy_version": summary.policy_version,
            });
        }
        if let Some(trust) = report.worker_trust {
            payload["worker_trust"] = json!(trust.as_str());
        }
        self.notifier
            .notify(&session_id, agent_id, events::DISPATCH_PLACEMENT, payload);
    }
}
