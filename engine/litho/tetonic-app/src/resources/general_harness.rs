//! Host preparation for the general harness. Produces the existing neutral loop
//! invocation; does not admit work, construct a tool host, or grant capabilities.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    harness: String,
    configuration: GeneralConfiguration,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneralConfiguration {
    instructions: String,
    #[serde(default)]
    requested_tools: Vec<String>,
    #[serde(default)]
    max_steps: Option<usize>,
}

/// Trusted host preparation ceilings, not user-supplied execution grants.
pub struct HarnessPreparationLimits {
    pub max_steps: usize,
    pub max_input_bytes: usize,
}

/// Configuration prepared for later authorized admission. Requested tools must
/// still be resolved against effective grants and installed tool capabilities.
pub struct PreparedAgentRevision {
    identity: tetonic_memory::AgentIdentityRow,
    invocation: tetonic_domain::AgentInvocation,
    requested_tools: Vec<String>,
}

impl PreparedAgentRevision {
    pub fn identity(&self) -> &tetonic_memory::AgentIdentityRow {
        &self.identity
    }

    pub fn invocation(&self) -> &tetonic_domain::AgentInvocation {
        &self.invocation
    }

    fn domain_identity(&self) -> Result<tetonic_domain::AgentIdentity, ResourceError> {
        let row = &self.identity;
        let identity = tetonic_domain::AgentIdentity {
            id: tetonic_domain::IdentityId::new(row.identity_id.clone()),
            owning_application: row.owning_application.clone(),
            bound_definition_digest: row.bound_definition_digest.clone(),
            privilege_class: row.privilege_class.clone(),
            toolset_subscriptions: serde_json::from_str(&row.toolset_subscriptions_json)
                .map_err(|_| ResourceError::Invalid)?,
            context_bindings: serde_json::from_str(&row.context_bindings_json)
                .map_err(|_| ResourceError::Invalid)?,
            recovery_id: row.recovery_id.clone(),
        };
        Ok(identity)
    }

    /// Pins the exact prepared input/revision and requested capability names.
    /// This is a job description, not permission to execute it.
    pub fn start_command(
        &self,
        recovery_id: String,
    ) -> Result<tetonic_run::StartIdentityJobCommand, ResourceError> {
        if recovery_id.trim().is_empty() || recovery_id.len() > 256 || recovery_id.contains('\0') {
            return Err(ResourceError::Invalid);
        }
        let identity = self.domain_identity()?;
        Ok(tetonic_run::StartIdentityJobCommand {
            job_spec: tetonic_domain::AgentJobSpec {
                identity_id: identity.id.clone(),
                definition_digest: identity.bound_definition_digest.clone(),
                input_digest: tetonic_run::job_input_digest(&self.invocation.user_input),
                capability_bindings: self.requested_tools.clone(),
                artifact_bindings: vec![],
                recovery_id,
            },
            identity,
            invocation: self.invocation.clone(),
        })
    }

    pub fn requested_tools(&self) -> &[String] {
        &self.requested_tools
    }

    /// Definition conformance only, never an execution authorization. The host
    /// must additionally authorize the initiating principal, context and every
    /// requested capability before admission and protected effects.
    /// No role overlays, artifacts or extra executable tools are implicit.
    pub fn execution_policy(&self) -> Result<tetonic_run::ExecutionPolicy, ResourceError> {
        let expected_identity = self.domain_identity()?;
        let expected_invocation = self.invocation.clone();
        let requested = self.requested_tools.clone();
        let mut host_tools = requested.clone();
        if !host_tools.contains(&expected_invocation.completion_tool) {
            host_tools.push(expected_invocation.completion_tool.clone());
        }
        Ok(Arc::new(
            move |identity, spec, role, advertised, invocation| {
                if identity != Some(&expected_identity)
                    || spec.identity_id != expected_identity.id
                    || spec.definition_digest != expected_identity.bound_definition_digest
                    || spec.input_digest
                        != tetonic_run::job_input_digest(&expected_invocation.user_input)
                    || invocation != &expected_invocation
                    || role.is_some()
                    || !spec.artifact_bindings.is_empty()
                    || !same_names(&spec.capability_bindings, &requested)
                    || !same_names(advertised, &host_tools)
                {
                    return Err("execution does not match prepared general revision".into());
                }
                Ok(())
            },
        ))
    }
}

fn same_names(actual: &[String], expected: &[String]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == actual.len()
        && actual.iter().all(|name| expected.contains(name))
}

impl ResourceService {
    /// Read authorization allows preparation only. This is not permission to run.
    pub async fn prepare_general_revision(
        &self,
        credential: &str,
        org: String,
        key: String,
        digest: String,
        input: String,
        limits: HarnessPreparationLimits,
    ) -> Result<PreparedAgentRevision, ResourceError> {
        let revision = self
            .get_agent_revision(credential, org, key, digest)
            .await?
            .ok_or(ResourceError::Invalid)?;
        prepare(revision, input, limits)
    }
}

fn prepare(
    revision: tetonic_memory::RegisteredAgent,
    input: String,
    limits: HarnessPreparationLimits,
) -> Result<PreparedAgentRevision, ResourceError> {
    let calculated = format!(
        "sha256:{:x}",
        Sha256::digest(revision.definition_json.as_bytes())
    );
    if calculated != revision.identity.bound_definition_digest
        || revision.definition_json.len() > 65536
    {
        return Err(ResourceError::Invalid);
    }
    let envelope: Envelope =
        serde_json::from_str(&revision.definition_json).map_err(|_| ResourceError::Invalid)?;
    if envelope.schema_version != 1
        || envelope.harness != "general"
        || revision.identity.owning_application != envelope.harness
    {
        return Err(ResourceError::Invalid);
    }
    let config = envelope.configuration;
    let steps = config.max_steps.unwrap_or(limits.max_steps);
    if config.instructions.trim().is_empty()
        || limits.max_steps == 0
        || steps == 0
        || steps > limits.max_steps
        || input.trim().is_empty()
        || input.len() > limits.max_input_bytes
        || config.requested_tools.len() > 128
    {
        return Err(ResourceError::Invalid);
    }
    let mut names = std::collections::HashSet::new();
    for tool in &config.requested_tools {
        if tool.is_empty()
            || tool.len() > 256
            || tool.chars().any(|c| c.is_control() || c.is_whitespace())
            || !names.insert(tool)
        {
            return Err(ResourceError::Invalid);
        }
    }
    Ok(PreparedAgentRevision {
        identity: revision.identity,
        invocation: tetonic_domain::AgentInvocation {
            instructions: config.instructions,
            user_input: input,
            max_steps: steps,
            explain_turn: false,
            empty_tool_nudge: false,
            completion_tool: "finish".into(),
            discipline: Default::default(),
        },
        requested_tools: config.requested_tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn preparation_pins_stored_instructions_without_granting_requested_tools() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let key = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        let service = local.resources();
        let first = service.register_agent(key.expose_secret(),"org".into(),"agent".into(),"general".into(),serde_json::json!({"instructions":"original","requested_tools":["recall"],"max_steps":4})).await.unwrap();
        service
            .publish_agent_revision(
                key.expose_secret(),
                "org".into(),
                "agent".into(),
                "general".into(),
                serde_json::json!({"instructions":"revised"}),
            )
            .await
            .unwrap();
        let prepared = service
            .prepare_general_revision(
                key.expose_secret(),
                "org".into(),
                "agent".into(),
                first.identity.bound_definition_digest.clone(),
                "user task".into(),
                HarnessPreparationLimits {
                    max_steps: 8,
                    max_input_bytes: 1024,
                },
            )
            .await
            .unwrap();
        assert_eq!(prepared.invocation.instructions, "original");
        assert_eq!(prepared.invocation.user_input, "user task");
        assert_eq!(prepared.invocation.max_steps, 4);
        assert_eq!(
            prepared.invocation.discipline,
            tetonic_domain::LoopDiscipline::default()
        );
        assert_eq!(prepared.requested_tools, vec!["recall"]);
        assert_eq!(prepared.identity.toolset_subscriptions_json, "[]");
        assert_eq!(prepared.identity.privilege_class, "unconfigured");
        let policy = prepared.execution_policy().unwrap();
        let row = prepared.identity();
        let identity = tetonic_domain::AgentIdentity {
            id: tetonic_domain::IdentityId::new(row.identity_id.clone()),
            owning_application: row.owning_application.clone(),
            bound_definition_digest: row.bound_definition_digest.clone(),
            privilege_class: row.privilege_class.clone(),
            toolset_subscriptions: vec![],
            context_bindings: vec![],
            recovery_id: row.recovery_id.clone(),
        };
        let spec = tetonic_domain::AgentJobSpec {
            identity_id: identity.id.clone(),
            definition_digest: identity.bound_definition_digest.clone(),
            input_digest: tetonic_run::job_input_digest(&prepared.invocation().user_input),
            capability_bindings: prepared.requested_tools().to_vec(),
            artifact_bindings: vec![],
            recovery_id: "test-job".into(),
        };
        let advertised = vec!["finish".into(), "recall".into()];
        assert!(policy(
            Some(&identity),
            &spec,
            None,
            &advertised,
            prepared.invocation()
        )
        .is_ok());
        for names in [
            vec!["finish".into()],
            vec!["finish".into(), "run_shell".into()],
            vec!["finish".into(), "recall".into(), "run_shell".into()],
            vec!["recall".into(), "recall".into()],
        ] {
            assert!(policy(Some(&identity), &spec, None, &names, prepared.invocation()).is_err());
        }
        let mut changed = prepared.invocation().clone();
        changed.instructions = "replacement".into();
        assert!(policy(Some(&identity), &spec, None, &advertised, &changed).is_err());
        assert!(policy(None, &spec, None, &advertised, prepared.invocation()).is_err());
        assert!(policy(
            Some(&identity),
            &spec,
            Some("coder"),
            &advertised,
            prepared.invocation()
        )
        .is_err());
        let mut changed_spec = spec.clone();
        changed_spec.capability_bindings.clear();
        assert!(policy(
            Some(&identity),
            &changed_spec,
            None,
            &advertised,
            prepared.invocation()
        )
        .is_err());
        changed_spec = spec.clone();
        changed_spec.input_digest = "other-task".into();
        assert!(policy(
            Some(&identity),
            &changed_spec,
            None,
            &advertised,
            prepared.invocation()
        )
        .is_err());
        let mut changed_identity = identity.clone();
        changed_identity.privilege_class = "admin".into();
        assert!(policy(
            Some(&changed_identity),
            &spec,
            None,
            &advertised,
            prepared.invocation()
        )
        .is_err());
        let limits = || HarnessPreparationLimits {
            max_steps: 8,
            max_input_bytes: 1024,
        };
        let mut corrupted = first.clone();
        corrupted.definition_json.push(' ');
        assert!(prepare(corrupted, "task".into(), limits()).is_err());
        for config in [
            serde_json::json!({"instructions":"x","max_steps":9}),
            serde_json::json!({"instructions":"x","max_steps":0}),
            serde_json::json!({"instructions":"x","unexpected":true}),
            serde_json::json!({"instructions":"x","requested_tools":["recall","recall"]}),
        ] {
            let revision = service
                .publish_agent_revision(
                    key.expose_secret(),
                    "org".into(),
                    "agent".into(),
                    "general".into(),
                    config,
                )
                .await
                .unwrap();
            assert!(prepare(revision, "task".into(), limits()).is_err());
        }
        local
            .credentials()
            .revoke(key.credential_id.clone())
            .await
            .unwrap();
        assert!(service
            .prepare_general_revision(
                key.expose_secret(),
                "org".into(),
                "agent".into(),
                first.identity.bound_definition_digest,
                "task".into(),
                limits()
            )
            .await
            .is_err());
    }
}
