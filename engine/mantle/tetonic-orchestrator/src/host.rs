//! Session host — lifecycle hooks between RPC and the agent loop (D3).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tetonic_domain::{CodeIndexOpen, DataClass, DisclosureTier, LspSessionOpen};
use tetonic_policy::{
    apply_data_class_floor, classify_session, data_class_name, default_disclosure_tier,
    parse_data_class, PolicyEngine, PolicyMode,
};
use tracing::info;

use crate::briefing::{build_session_briefing, BriefingInput, BriefingOptions};

#[derive(Debug, Clone)]
pub struct SessionStartPlan {
    pub data_class: DataClass,
    pub disclosure_tier: DisclosureTier,
    pub briefing: Option<String>,
    /// Layer-1 project digest + notes (D4).
    pub project_context: Option<String>,
    pub verify_cmd: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TurnHooks {
    /// Reserved for post-turn consolidation (D4).
    pub note: Option<String>,
}

/// Function or closure resolving verify command for workspace and override.
pub type VerifyResolver = Arc<dyn Fn(&Path, Option<&str>) -> Option<String> + Send + Sync>;

/// Orchestrator shell: policy + briefing + session lifecycle (D3/D5).
pub struct SessionHost {
    workspace_root: PathBuf,
    policy: Arc<PolicyEngine>,
    briefing_enabled: bool,
    briefing_token_budget: usize,
    code_index: Option<Arc<dyn CodeIndexOpen>>,
    lsp_open: Option<Arc<dyn LspSessionOpen>>,
    verify_resolver: Option<VerifyResolver>,
}

impl SessionHost {
    pub fn new(workspace_root: impl Into<PathBuf>, policy: Arc<PolicyEngine>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            policy,
            briefing_enabled: true,
            briefing_token_budget: BriefingOptions::default().token_budget,
            code_index: None,
            lsp_open: None,
            verify_resolver: None,
        }
    }

    pub fn policy(&self) -> &Arc<PolicyEngine> {
        &self.policy
    }

    pub fn with_code_index(mut self, opener: Arc<dyn CodeIndexOpen>) -> Self {
        self.code_index = Some(opener);
        self
    }

    pub fn with_lsp_open(mut self, opener: Arc<dyn LspSessionOpen>) -> Self {
        self.lsp_open = Some(opener);
        self
    }

    pub fn with_verify_resolver(mut self, resolver: VerifyResolver) -> Self {
        self.verify_resolver = Some(resolver);
        self
    }

    pub fn with_briefing(mut self, enabled: bool) -> Self {
        self.briefing_enabled = enabled;
        self
    }

    pub fn with_briefing_budget(mut self, tokens: usize) -> Self {
        self.briefing_token_budget = tokens.max(100);
        self
    }

    /// `on_session_start`: classify data, resolve verify, build briefing.
    #[allow(clippy::too_many_arguments)]
    pub fn on_session_start(
        &self,
        session_id: &str,
        goal: Option<&str>,
        data_class_override: Option<&str>,
        verify_override: Option<&str>,
        briefing_enabled: bool,
        fabric_hint: Option<&str>,
        store: Option<&tetonic_memory::Store>,
        index_db: Option<&Path>,
    ) -> SessionStartPlan {
        let floor = classify_session(&self.workspace_root, goal);
        let data_class =
            apply_data_class_floor(floor.class, data_class_override.and_then(parse_data_class));

        // A private or team discussion must not receive the legacy project
        // briefing, and this host must not write onto that session.
        if store.is_some_and(|s| s.require_legacy_session(session_id).is_err()) {
            let verify_cmd = if let Some(resolver) = &self.verify_resolver {
                resolver(&self.workspace_root, verify_override)
            } else {
                verify_override.map(str::to_string)
            };
            return SessionStartPlan {
                data_class,
                disclosure_tier: default_disclosure_tier(data_class),
                briefing: None,
                project_context: None,
                verify_cmd,
            };
        }

        if let Some(s) = store {
            if s.set_session_data_class(session_id, data_class_name(data_class))
                .is_err()
            {
                tracing::warn!("session data_class persist failed");
            }
            if let Ok(pid) = s.ensure_project(&self.workspace_root) {
                if s.link_session_project(session_id, &pid).is_err() {
                    tracing::warn!("session project link failed");
                }
            }
        }

        let project_context = store.and_then(|s| {
            s.load_project_context(&self.workspace_root, 1_500)
                .ok()
                .filter(|t| !t.trim().is_empty())
        });

        let verify_cmd = if let Some(resolver) = &self.verify_resolver {
            resolver(&self.workspace_root, verify_override)
        } else {
            verify_override.map(str::to_string)
        };

        let briefing = if briefing_enabled && self.briefing_enabled {
            build_session_briefing(
                BriefingInput {
                    workspace_root: &self.workspace_root,
                    session_id,
                    verify_cmd: verify_cmd.as_deref(),
                    store,
                    index_db,
                    code_index: self.code_index.as_deref(),
                    lsp_open: self.lsp_open.as_deref(),
                    fabric_hint,
                },
                BriefingOptions {
                    token_budget: self.briefing_token_budget,
                    ..Default::default()
                },
            )
        } else {
            None
        };

        if briefing.is_some() {
            info!(session_id, "session briefing prepared");
        }

        if project_context.is_some() {
            info!(session_id, "project memory loaded");
        }

        SessionStartPlan {
            data_class,
            disclosure_tier: default_disclosure_tier(data_class),
            briefing,
            project_context,
            verify_cmd,
        }
    }

    pub fn on_turn_begin(&self, _session_id: &str, _turn_index: u32) -> TurnHooks {
        TurnHooks::default()
    }

    pub fn on_turn_end(
        &self,
        _session_id: &str,
        _store: Option<&tetonic_memory::Store>,
    ) -> TurnHooks {
        // Digest consolidation deferred to session end or explicit project/consolidate (SEC2-E2-017).
        TurnHooks::default()
    }

    /// Merge session outcome into project digest when the session reaches a terminal state.
    pub fn on_session_end(&self, session_id: &str, store: Option<&tetonic_memory::Store>) {
        if let Some(s) = store {
            match s.consolidate_session(session_id) {
                Ok(()) => {
                    if let Ok(payload) = serde_json::to_string(
                        &serde_json::json!({"text":"project digest merge attempted"}),
                    ) {
                        let _ = s.append_event(session_id, "consolidate", "system", &payload);
                    }
                }
                Err(_) => tracing::warn!("session consolidate failed"),
            }
        }
    }

    pub fn policy_mode(&self) -> PolicyMode {
        self.policy.mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_policy::PolicyMode;

    #[test]
    fn private_session_does_not_receive_legacy_project_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = tetonic_memory::Store::open(dir.path().join("host.db")).unwrap();
        store.bootstrap_control("admin", "org", "Org").unwrap();
        store
            .create_information_context(
                "admin",
                "private",
                &tetonic_memory::ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .unwrap();
        store
            .insert_open_discussion("admin", "private", "private-notes")
            .unwrap();
        store
            .append_message(
                "private-notes",
                "user",
                "",
                "searchword PRIVATECANARY",
                None,
            )
            .unwrap();
        store
            .add_project_note(dir.path(), "LEGACYNOTE for the local operator", "user")
            .unwrap();
        let host = SessionHost::new(
            dir.path(),
            Arc::new(PolicyEngine::new(PolicyMode::EstateStub)),
        );
        let private_plan = host.on_session_start(
            "private-notes",
            None,
            None,
            None,
            true,
            None,
            Some(&store),
            None,
        );
        let private_text = format!(
            "{}\n{}",
            private_plan.briefing.unwrap_or_default(),
            private_plan.project_context.unwrap_or_default()
        );
        assert!(
            !private_text.contains("PRIVATECANARY"),
            "private history entered the start plan: {private_text}"
        );
        assert!(
            !private_text.contains("LEGACYNOTE"),
            "legacy project memory was briefed into a private session: {private_text}"
        );
        let legacy = store
            .start_session(dir.path().to_str().unwrap(), "single-agent", "mock")
            .unwrap();
        let legacy_plan = host.on_session_start(
            &legacy,
            None,
            None,
            None,
            true,
            None,
            Some(&store),
            None,
        );
        let legacy_text = legacy_plan.project_context.unwrap_or_default();
        assert!(
            legacy_text.contains("LEGACYNOTE"),
            "legacy session lost its project note: {legacy_text}"
        );
        assert!(!legacy_text.contains("PRIVATECANARY"));
    }
}
