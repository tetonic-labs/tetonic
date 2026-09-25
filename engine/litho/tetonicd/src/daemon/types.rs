//! Daemon session and engine state types.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tetonic_app::{Application, CapacityStatus, InferenceDefaults};

/// Synthetic session for capacity progress notifications (not a user session).
pub const CAPACITY_SESSION: &str = "__capacity__";
pub const CAPACITY_AGENT: &str = "__capacity__";

/// Exclusive inference lease while a capacity optimize job runs.
pub struct CapacityJobRuntime {
    pub busy: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
    pub job_id: Mutex<Option<String>>,
    pub pending_reload: Arc<Mutex<Option<InferenceDefaults>>>,
}

impl Default for CapacityJobRuntime {
    fn default() -> Self {
        Self {
            busy: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            job_id: Mutex::new(None),
            pending_reload: Arc::new(Mutex::new(None)),
        }
    }
}

/// The hot, shared application handles, created once at `initialize`.
pub struct EngineServices {
    pub app: Arc<Application>,
    pub workspace_root: String,
    pub model: String,
    pub model_hard: String,
    pub models: Vec<String>,
    pub tool_capable: bool,
    #[allow(dead_code)]
    pub tools: Vec<String>,
    pub fabric_pooled: bool,
    pub num_ctx: u32,
    #[allow(dead_code)]
    pub ollama_base: String,
    pub capacity: Option<CapacityStatus>,
}
