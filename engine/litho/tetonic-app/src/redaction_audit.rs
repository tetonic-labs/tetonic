//! Durable outbound-redaction audit (H1-1).
//!
//! Writes `outbound_redaction` rows to the session event log. A failed write
//! is returned to the Infer chokepoint so the turn fail-closes instead of
//! sending unredacted content.

use tetonic_domain::secrets::{OutboundRedaction, OutboundRedactionSink};
use tetonic_memory::SharedStore;

/// Session-store sink. Requires a real `sessions` row because `events.session_id`
/// is a foreign key.
pub struct StoreRedactionSink {
    store: SharedStore,
}

impl StoreRedactionSink {
    pub fn new(store: SharedStore) -> Self {
        Self { store }
    }
}

impl OutboundRedactionSink for StoreRedactionSink {
    fn record(&self, event: &OutboundRedaction) -> Result<(), String> {
        let session_id = event
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "outbound redaction audit requires a session id".to_string())?;
        let payload = serde_json::to_string(event).map_err(|e| e.to_string())?;
        self.store
            .write_sync({
                let session_id = session_id.to_string();
                let payload = payload.clone();
                move |db| db.append_event(&session_id, "outbound_redaction", "scanner", &payload)
            })
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Used when the compute plane has no SQLite store. Findings fail closed
/// because there is nowhere durable to record the redaction.
pub struct MissingStoreRedactionSink;

impl OutboundRedactionSink for MissingStoreRedactionSink {
    fn record(&self, _event: &OutboundRedaction) -> Result<(), String> {
        Err("outbound redaction audit requires a session store".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::secrets::RedactionRecordReference;
    use tetonic_domain::workspace::ContentDigest;

    fn sample(session_id: Option<String>) -> OutboundRedaction {
        OutboundRedaction {
            session_id,
            model: "qwen:7b".into(),
            role: "tool".into(),
            message_index: 0,
            omitted: false,
            records: vec![RedactionRecordReference {
                original_digest: ContentDigest("sha256:orig".into()),
                redacted_digest: ContentDigest("sha256:redacted".into()),
            }],
        }
    }

    #[test]
    fn store_sink_writes_outbound_redaction_event() {
        let db_path = std::env::temp_dir().join(format!(
            "lokai_test_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let shared = SharedStore::open(&db_path, 1).unwrap();
        let id = shared
            .write_sync(|db| {
                db.start_session("/tmp/ws-redact", "single-agent", "m")
                    .unwrap()
            })
            .unwrap();
        let sink = StoreRedactionSink::new(shared.clone());
        sink.record(&sample(Some(id.clone())))
            .expect("durable write");
        let count = shared
            .read_sync({
                let id = id.clone();
                move |db| db.event_count(&id, "outbound_redaction").unwrap()
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn store_sink_fails_closed_without_session() {
        let db_path = std::env::temp_dir().join(format!(
            "lokai_test_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SharedStore::open(&db_path, 1).unwrap();
        let sink = StoreRedactionSink::new(store);
        assert!(sink
            .record(&sample(None))
            .unwrap_err()
            .contains("session id"));
    }

    #[test]
    fn missing_store_sink_fails_closed_on_findings() {
        let sink = MissingStoreRedactionSink;
        assert!(sink.record(&sample(Some("sess".into()))).is_err());
    }
}
