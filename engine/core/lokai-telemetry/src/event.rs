use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The canonical JSON schema for a tracing event emitted to the local sink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub event_name: String,
    pub schema_version: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    pub correlation_identifiers: super::TraceContext,
    pub component_name: String,
    pub operation_name: String,
    pub metrics: HashMap<String, f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_sensitivity: Option<String>,
}

impl Default for TraceEvent {
    fn default() -> Self {
        Self {
            event_name: "unknown".to_string(),
            schema_version: "v1.0.0".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            duration_ms: None,
            outcome: "success".to_string(),
            failure_class: None,
            correlation_identifiers: Default::default(),
            component_name: "lokai".to_string(),
            operation_name: "unknown".to_string(),
            metrics: HashMap::new(),
            data_sensitivity: None,
        }
    }
}
