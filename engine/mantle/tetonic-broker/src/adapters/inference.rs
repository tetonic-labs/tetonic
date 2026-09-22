//! PooledProvider adapter — preserves existing inference behavior (M6-1).
//!
//! H1-1: `BrokerInferenceProvider` is the single outbound Infer scan chokepoint.
//! Local hops redact and continue. Remote hops fail closed on high-confidence
//! findings (`OutboundScan::blocks_remote`) before `PooledProvider` sends bytes.

use std::sync::Arc;

use async_trait::async_trait;
use tetonic_domain::secrets::{
    OutboundRedaction, OutboundRedactionSink, ScanContext, SecretScanner,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, InferenceError, InferenceProvider, OutboundScan,
    PooledProvider, TokenSink,
};

use crate::broker::DefaultComputeBroker;

/// Thin adapter: ComputeBroker dispatches Infer jobs here; does not reimplement HTTP.
pub struct InferenceTargetAdapter {
    pooled: Arc<PooledProvider>,
}

impl InferenceTargetAdapter {
    pub fn new(pooled: Arc<PooledProvider>) -> Self {
        Self { pooled }
    }

    pub fn pooled(&self) -> &Arc<PooledProvider> {
        &self.pooled
    }

    pub fn cancel_session_jobs(&self, session_id: &str) {
        self.pooled.cancel_session_jobs(session_id);
    }
}

#[async_trait]
impl InferenceProvider for InferenceTargetAdapter {
    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.pooled.chat(req, on_token).await
    }

    async fn fabric_snapshot(&self) -> FabricSnapshot {
        self.pooled.fabric_snapshot().await
    }
}

/// InferenceProvider facade for existing agent loops.
pub struct BrokerInferenceProvider {
    broker: Arc<DefaultComputeBroker>,
    scanner: Option<Arc<dyn SecretScanner>>,
    redaction_sink: Option<Arc<dyn OutboundRedactionSink>>,
}

impl BrokerInferenceProvider {
    pub fn new(broker: Arc<DefaultComputeBroker>) -> Self {
        Self {
            broker,
            scanner: None,
            redaction_sink: None,
        }
    }

    pub fn with_outbound_scanner(
        mut self,
        scanner: Arc<dyn SecretScanner>,
        sink: Arc<dyn OutboundRedactionSink>,
    ) -> Self {
        self.scanner = Some(scanner);
        self.redaction_sink = Some(sink);
        self
    }

    pub fn has_secret_scanner(&self) -> bool {
        self.scanner.is_some() && self.redaction_sink.is_some()
    }

    pub fn broker(&self) -> &Arc<DefaultComputeBroker> {
        &self.broker
    }

    /// Scan and redact Infer messages. Must run before any local or remote dispatch.
    pub async fn scan_outbound(&self, req: ChatRequest) -> Result<ChatRequest, InferenceError> {
        let scanner = self
            .scanner
            .as_deref()
            .ok_or_else(|| InferenceError::SecretScanFailed {
                reason: "outbound secret scanner is not attached".into(),
            })?;
        let sink =
            self.redaction_sink
                .as_deref()
                .ok_or_else(|| InferenceError::SecretScanFailed {
                    reason: "outbound redaction audit sink is not attached".into(),
                })?;
        redact_outbound(scanner, sink, req).await
    }
}

/// Policy: local destinations redact (or omit unredactable spans) and continue.
/// Remote destinations are stamped `high_confidence` so `PooledProvider` refuses
/// the hop with [`InferenceError::RemoteSecretDenied`] and sends nothing.
pub async fn redact_outbound(
    scanner: &dyn SecretScanner,
    sink: &dyn OutboundRedactionSink,
    mut req: ChatRequest,
) -> Result<ChatRequest, InferenceError> {
    let mut high_confidence = false;
    let session_id = req.fabric.as_ref().and_then(|f| f.session_id.clone());
    for (message_index, msg) in req.messages.iter_mut().enumerate() {
        let path_owned = if msg.role == "tool" {
            msg.tool_name.clone()
        } else {
            None
        };
        let role = msg.role.clone();
        let ctx = ScanField {
            scanner,
            sink,
            session_id: &session_id,
            model: &req.model,
            role: &role,
            message_index,
            path: path_owned.as_deref(),
        };
        if !msg.content.is_empty() {
            msg.content = scan_text_field(ctx, &msg.content.clone(), &mut high_confidence).await?;
        }
        if let Some(calls) = msg.tool_calls.as_mut() {
            for call in calls {
                call.function.name =
                    scan_text_field(ctx, &call.function.name.clone(), &mut high_confidence).await?;
                let raw = serde_json::to_string(&call.function.arguments).unwrap_or_default();
                if raw.is_empty() || raw == "null" {
                    continue;
                }
                let scanned = scan_text_field(ctx, &raw, &mut high_confidence).await?;
                if scanned != raw {
                    call.function.arguments = serde_json::from_str(&scanned)
                        .unwrap_or(serde_json::Value::String(scanned));
                }
            }
        }
    }
    req.outbound_scan = OutboundScan::from_scan(high_confidence);
    Ok(req)
}

#[derive(Clone, Copy)]
struct ScanField<'a> {
    scanner: &'a dyn SecretScanner,
    sink: &'a dyn OutboundRedactionSink,
    session_id: &'a Option<String>,
    model: &'a str,
    role: &'a str,
    message_index: usize,
    path: Option<&'a str>,
}

async fn scan_text_field(
    ctx: ScanField<'_>,
    text: &str,
    high_confidence: &mut bool,
) -> Result<String, InferenceError> {
    match ctx
        .scanner
        .scan_and_redact_in_context(
            text,
            ctx.path,
            ScanContext {
                session_id: ctx.session_id.as_deref(),
                project_id: None,
            },
        )
        .await
    {
        Ok(None) => Ok(text.to_string()),
        Ok(Some((records, redacted))) => {
            *high_confidence = true;
            let omitted = redacted.is_empty();
            let event = OutboundRedaction {
                session_id: ctx.session_id.clone(),
                model: ctx.model.to_string(),
                role: ctx.role.to_string(),
                message_index: ctx.message_index,
                omitted,
                records,
            };
            ctx.sink
                .record(&event)
                .map_err(|reason| InferenceError::SecretScanFailed {
                    reason: format!("redaction audit write failed: {reason}"),
                })?;
            Ok(if omitted {
                "[omitted — secret material withheld]".into()
            } else {
                redacted
            })
        }
        Err(reason) => Err(InferenceError::SecretScanFailed { reason }),
    }
}

#[async_trait]
impl InferenceProvider for BrokerInferenceProvider {
    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let timer = tetonic_telemetry::StageTimer::start_visible(
            tetonic_telemetry::PerfStage::InferenceScan,
        );
        let scanned = self.scan_outbound(req).await;
        timer.finish(scanned.is_ok());
        let req = scanned?;
        self.broker.chat_admitted(req, on_token).await
    }

    async fn fabric_snapshot(&self) -> FabricSnapshot {
        if let Some(adapter) = self.broker.inference_adapter() {
            adapter.fabric_snapshot().await
        } else {
            FabricSnapshot::empty()
        }
    }
}

#[cfg(test)]
mod outbound_redaction_tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;
    use tetonic_domain::secrets::{RedactionRecordReference, ScanOutcome};
    use tetonic_domain::workspace::ContentDigest;
    use tetonic_inference::{FabricCallMeta, Message, ToolCall};
    use tetonic_secrets::scanner::ScannerEngine;

    struct RecordingSink {
        events: Mutex<Vec<OutboundRedaction>>,
        fail: bool,
    }

    impl OutboundRedactionSink for RecordingSink {
        fn record(&self, event: &OutboundRedaction) -> Result<(), String> {
            if self.fail {
                return Err("audit store unavailable".into());
            }
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    struct FailingScanner;

    #[async_trait]
    impl SecretScanner for FailingScanner {
        async fn scan_and_redact(
            &self,
            _content: &str,
            _path: Option<&str>,
        ) -> Result<ScanOutcome, String> {
            Err("scanner exploded".into())
        }
    }

    fn pem_tool_request() -> ChatRequest {
        ChatRequest {
            model: "qwen:7b".into(),
            messages: vec![Message::tool(
                "read_file",
                "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----",
            )],
            fabric: Some(FabricCallMeta {
                session_id: Some("sess_scan".into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn outbound_exception_is_bound_to_request_session() {
        let scanner = ScannerEngine::default_engine();
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let fingerprint = tetonic_secrets::detectors::default_detectors()[1].scan(
            secret,
            None,
            &scanner.hmac_key,
        )[0]
        .fingerprint
        .0
        .clone();
        scanner.grant_override(
            &fingerprint,
            tetonic_secrets::OverrideScope::Session("allowed".into()),
        );
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        for (session, should_redact) in [("allowed", false), ("denied", true), ("allowed", false)] {
            let mut req = pem_tool_request();
            req.fabric.as_mut().unwrap().session_id = Some(session.into());
            req.messages = vec![Message::user(secret)];
            let out = redact_outbound(&scanner, &sink, req).await.unwrap();
            assert_eq!(out.outbound_scan.high_confidence(), should_redact);
            assert_eq!(out.messages[0].content.contains(secret), !should_redact);
        }
    }

    #[tokio::test]
    async fn pem_in_read_file_is_redacted_and_audited() {
        let scanner = ScannerEngine::default_engine();
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        let out = redact_outbound(&scanner, &sink, pem_tool_request())
            .await
            .expect("scan");
        assert!(out.outbound_scan.is_scanned());
        assert!(out.outbound_scan.high_confidence());
        assert!(out.outbound_scan.blocks_remote());
        let body = &out.messages[0].content;
        assert!(
            !body.contains("BEGIN RSA PRIVATE KEY"),
            "remote/local body must not contain key material: {body}"
        );
        assert!(body.contains("[REDACTED:pem-private-key]") || body.contains("omitted"));
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].role, "tool");
        assert_eq!(events[0].session_id.as_deref(), Some("sess_scan"));
        assert!(!events[0].records.is_empty());
    }

    #[tokio::test]
    async fn high_confidence_marks_remote_refusal_and_does_not_keep_key() {
        let scanner = ScannerEngine::default_engine();
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        let out = redact_outbound(&scanner, &sink, pem_tool_request())
            .await
            .unwrap();
        assert!(out.outbound_scan.blocks_remote());
        assert!(!out.messages[0].content.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[tokio::test]
    async fn scanner_error_fails_closed() {
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        let err = redact_outbound(&FailingScanner, &sink, pem_tool_request())
            .await
            .unwrap_err();
        assert!(matches!(err, InferenceError::SecretScanFailed { .. }));
        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn audit_write_failure_fails_closed() {
        let scanner = ScannerEngine::default_engine();
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: true,
        };
        let err = redact_outbound(&scanner, &sink, pem_tool_request())
            .await
            .unwrap_err();
        assert!(
            matches!(err, InferenceError::SecretScanFailed { reason } if reason.contains("audit"))
        );
    }

    #[tokio::test]
    async fn clean_content_is_unchanged() {
        let scanner = ScannerEngine::default_engine();
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        let req = ChatRequest {
            model: "qwen:7b".into(),
            messages: vec![Message::user("summarize README")],
            ..Default::default()
        };
        let out = redact_outbound(&scanner, &sink, req).await.unwrap();
        assert!(out.outbound_scan.is_scanned());
        assert!(!out.outbound_scan.high_confidence());
        assert!(!out.outbound_scan.blocks_remote());
        assert_eq!(out.messages[0].content, "summarize README");
        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unredactable_content_is_omitted_not_sent() {
        struct OmitScanner;
        #[async_trait]
        impl SecretScanner for OmitScanner {
            async fn scan_and_redact(
                &self,
                _content: &str,
                _path: Option<&str>,
            ) -> Result<ScanOutcome, String> {
                Ok(Some((
                    vec![RedactionRecordReference {
                        original_digest: ContentDigest("sha256:orig".into()),
                        redacted_digest: ContentDigest("sha256:empty".into()),
                    }],
                    String::new(),
                )))
            }
        }
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        let req = ChatRequest {
            model: "qwen:7b".into(),
            messages: vec![Message::tool("read_file", "AKIAIOSFODNN7EXAMPLE")],
            ..Default::default()
        };
        let out = redact_outbound(&OmitScanner, &sink, req).await.unwrap();
        assert!(!out.messages[0].content.contains("AKIA"));
        assert!(sink.events.lock().unwrap()[0].omitted);
    }

    #[tokio::test]
    async fn secret_only_in_tool_call_arguments_is_redacted() {
        let scanner = ScannerEngine::default_engine();
        let sink = RecordingSink {
            events: Mutex::new(Vec::new()),
            fail: false,
        };
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let req = ChatRequest {
            model: "qwen:7b".into(),
            messages: vec![Message::assistant("").with_tool_calls(vec![ToolCall {
                function: tetonic_inference::FunctionCall {
                    name: "write_file".into(),
                    arguments: json!({ "body": secret }),
                },
            }])],
            ..Default::default()
        };
        let out = redact_outbound(&scanner, &sink, req).await.expect("scan");
        assert!(out.outbound_scan.is_scanned());
        let args = &out.messages[0].tool_calls.as_ref().unwrap()[0]
            .function
            .arguments;
        let rendered = args.to_string();
        assert!(
            !rendered.contains(secret),
            "tool_calls.arguments must not keep the secret: {rendered}"
        );
    }
}
