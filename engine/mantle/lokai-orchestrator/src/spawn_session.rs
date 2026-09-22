//! Session-scoped spawn tree + rollback tracking (SEC2-E2-009 / E2-011 / E2-012).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::run::spawn_depth;
use crate::turn::ROOT_AGENT;
use lokai_memory::RecoverMutex;

/// Tracks live agent ids, authoritative spawn parent, and rolled-back spawn branches.
pub struct SpawnSessionTrack {
    spawn_anchor: Mutex<String>,
    known_agents: Mutex<HashSet<String>>,
    rolled_back: Mutex<HashSet<String>>,
}

impl SpawnSessionTrack {
    pub fn new() -> Arc<Self> {
        let mut known = HashSet::new();
        known.insert(ROOT_AGENT.to_string());
        Arc::new(Self {
            spawn_anchor: Mutex::new(ROOT_AGENT.to_string()),
            known_agents: Mutex::new(known),
            rolled_back: Mutex::new(HashSet::new()),
        })
    }

    pub fn register_agent(&self, agent_id: &str) {
        self.known_agents
            .lock_recover()
            .insert(agent_id.to_string());
        let cur = self.spawn_anchor.lock_recover().clone();
        if spawn_depth(agent_id) >= spawn_depth(&cur) {
            *self.spawn_anchor.lock_recover() = agent_id.to_string();
        }
    }

    /// Validate RPC `parent_agent_id` against the live spawn anchor (SEC2-E2-009).
    pub fn resolve_spawn_parent(&self, client_parent: Option<&str>) -> Result<String, String> {
        let anchor = self.spawn_anchor.lock_recover().clone();
        let parent = match client_parent {
            None => anchor.clone(),
            Some(p) if p == anchor => p.to_string(),
            Some(p) if self.known_agents.lock_recover().contains(p) => {
                return Err(format!(
                    "parent_agent_id `{p}` does not match active spawn anchor `{anchor}`"
                ));
            }
            Some(p) => {
                return Err(format!("unknown parent_agent_id `{p}`"));
            }
        };
        if !self.known_agents.lock_recover().contains(&parent) {
            return Err(format!("unknown parent_agent_id `{parent}`"));
        }
        Ok(parent)
    }

    pub fn record_spawn_rollback(&self, agent_id: &str) {
        self.rolled_back.lock_recover().insert(agent_id.to_string());
    }

    pub fn rolled_back_agents(&self) -> Vec<String> {
        self.rolled_back.lock_recover().iter().cloned().collect()
    }

    pub fn known_agents(&self) -> Vec<String> {
        self.known_agents.lock_recover().iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_spoofed_shallow_parent() {
        let track = SpawnSessionTrack::new();
        track.register_agent("a0_s0");
        assert_eq!(
            track.resolve_spawn_parent(Some("a0")).unwrap_err(),
            "parent_agent_id `a0` does not match active spawn anchor `a0_s0`"
        );
        assert!(track.resolve_spawn_parent(None).unwrap() == "a0_s0");
    }
}
