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
    pub identity: tetonic_memory::AgentIdentityRow,
    pub invocation: tetonic_domain::AgentInvocation,
    pub requested_tools: Vec<String>,
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
