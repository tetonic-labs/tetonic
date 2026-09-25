//! Characterization probes: passing means the audited defect is reproduced.
//! These are NOT desired-behavior regression tests. Invert expectations when fixing.
#[cfg(test)]
mod tests {
    use tetonic_domain::{
        Affordance, AgentId, AgentStateCheckpoint, BrainPathway, IntentCharter,
        OperationalBoundary, WorldAction, WorldManifest,
    };

    fn checkpoint(agent: &str, id: &str) -> AgentStateCheckpoint {
        AgentStateCheckpoint::new(
            id,
            AgentId::new(agent),
            None,
            Some(IntentCharter::new("charter", "observe")),
            "digest",
            "memory",
            vec![],
        )
    }

    #[test]
    fn checkpoint_accepts_changed_charter_without_rehash() {
        let mut c = checkpoint("a", "1");
        c.charter_snapshot = Some(IntentCharter::new("different", "change everything"));
        assert!(c.verify_integrity().is_ok());
    }

    #[test]
    fn checkpoint_hash_has_field_boundary_collision() {
        let mut a = checkpoint("a", "1");
        a.working_memory_digest = "ab".into();
        a.working_memory_buffer = "c".into();
        let mut b = a.clone();
        b.working_memory_digest = "a".into();
        b.working_memory_buffer = "bc".into();
        assert_eq!(a.compute_checksum(), b.compute_checksum());
    }

    #[test]
    fn checkpoint_lookup_crosses_agent_prefix_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let manager = tetonic_core::checkpoint::CheckpointManager::new(dir.path());
        manager
            .save_checkpoint(&checkpoint("alice-child", "1"))
            .unwrap();
        let recovered = manager
            .load_latest_checkpoint(&AgentId::new("alice"))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.agent_id.0, "alice-child");
    }

    #[test]
    fn checkpoint_latest_uses_lexical_not_numeric_order() {
        let dir = tempfile::tempdir().unwrap();
        let manager = tetonic_core::checkpoint::CheckpointManager::new(dir.path());
        manager.save_checkpoint(&checkpoint("alice", "9")).unwrap();
        manager.save_checkpoint(&checkpoint("alice", "10")).unwrap();
        let recovered = manager
            .load_latest_checkpoint(&AgentId::new("alice"))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.checkpoint_id, "9");
    }

    #[test]
    fn manifest_accepts_wrong_parameter_type() {
        let manifest = WorldManifest::new("audit", "1").with_affordance(Affordance::new(
            "move",
            "move",
            serde_json::json!({"type":"object", "required":["x"],
                "properties":{"x":{"type":"integer"}}}),
            false,
        ));
        let action = WorldAction::with_payload(
            "move",
            serde_json::json!({"x":"wrong"}),
            BrainPathway::Single {
                model: "audit".into(),
            },
        );
        assert!(manifest.validate_action(&action).is_ok());
    }

    #[test]
    fn namespace_allowlist_accepts_unnamespaced_action() {
        let charter = IntentCharter::new("c", "observe").with_boundary(
            OperationalBoundary::NamespaceAllowlist {
                allowed: vec!["read".into()],
            },
        );
        let action = WorldAction::bare(
            "delete",
            BrainPathway::Single {
                model: "audit".into(),
            },
        );
        assert!(charter.evaluate_action(&action).is_ok());
    }

    #[test]
    fn keeper_restart_reissues_identical_proof() {
        let mut first = tetonic_node::KeeperRegistry::new();
        let caps = tetonic_node::NodeRole::Runner.capabilities();
        let p1 = first
            .register_runner("runner".into(), "local".into(), caps)
            .unwrap();
        let mut second = tetonic_node::KeeperRegistry::new();
        let p2 = second
            .register_runner("runner".into(), "local".into(), caps)
            .unwrap();
        assert_eq!(p1.lease_id, p2.lease_id);
        assert_eq!(p1.lease_epoch, p2.lease_epoch);
        assert!(second.record_heartbeat("runner", &p1).is_ok());
    }

    #[test]
    fn keeper_heartbeat_does_not_validate_holder() {
        let mut registry = tetonic_node::KeeperRegistry::new();
        let mut proof = registry
            .register_runner(
                "runner".into(),
                "local".into(),
                tetonic_node::NodeRole::Runner.capabilities(),
            )
            .unwrap();
        proof.holder = tetonic_domain::ExecutionTargetId::worker("different-runner");
        assert!(registry.record_heartbeat("runner", &proof).is_ok());
    }

    #[test]
    fn fleet_individual_resume_ignores_global_stop() {
        let fleet = tetonic_orchestrator::FleetSupervisor::new();
        let id = AgentId::new("a");
        fleet.register_agent(id.clone(), None, None, None);
        fleet.emergency_stop_fleet("audit");
        fleet.resume_agent(&id).unwrap();
        assert!(fleet.is_fleet_estopped());
        assert_eq!(fleet.fleet_snapshot().running_agents, 1);
    }

    #[test]
    fn fleet_registration_ignores_global_stop() {
        let fleet = tetonic_orchestrator::FleetSupervisor::new();
        fleet.emergency_stop_fleet("audit");
        fleet.register_agent(AgentId::new("new"), None, None, None);
        assert!(fleet.is_fleet_estopped());
        assert_eq!(fleet.fleet_snapshot().running_agents, 1);
    }
}
