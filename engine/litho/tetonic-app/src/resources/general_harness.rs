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
    /// Optional owner preferences; only a host may apply these within its ceilings.
    #[serde(default)]
    preferences: Option<GeneralAgentPreferences>,
    #[serde(default)]
    explain_turn: Option<bool>,
}

/// Durable agent configuration, resolved within host ceilings. These preferences
/// do not themselves grant execution authority or access to a host's resources.
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralAgentPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_disclosure: Option<super::ToolDisclosure>,
    /// Canonical owner-approved folder for hosted file-tool data, not authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosted_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hosted_consent: bool,
    pub display_name: String,
    pub model: String,
    pub max_elapsed_seconds: u64,
    pub reported_token_ceiling: u64,
}

/// Trusted host preparation ceilings, not user-supplied execution grants.
#[derive(Clone)]
pub struct HarnessPreparationLimits {
    /// Trusted host control capability, separately bound and authorized at admission.
    pub human_handoff: bool,
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
    let mut config = envelope.configuration;
    if limits.human_handoff
        && !config
            .requested_tools
            .iter()
            .any(|t| t == super::plan_dispatch::ASK_HUMAN)
    {
        config
            .requested_tools
            .push(super::plan_dispatch::ASK_HUMAN.into());
    }
    if let Some(prefs) = &config.preferences {
        if prefs.display_name.trim().is_empty()
            || prefs.display_name.len() > 256
            || prefs.model.trim().is_empty()
            || prefs.model.len() > 256
            || prefs.display_name.chars().any(char::is_control)
            || prefs.model.chars().any(char::is_whitespace)
            || prefs.max_elapsed_seconds == 0
            || prefs.reported_token_ceiling == 0
        {
            return Err(ResourceError::Invalid);
        }
    }
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
    // A prompt-only worker may answer naturally after a human clarification.
    // Keep dispatchers and effectful jobs on explicit completion so prose cannot
    // bypass their host completion guards. This changes no tool authority.
    let explain_turn = if limits.human_handoff {
        config
            .requested_tools
            .iter()
            .all(|tool| tool == "finish" || tool == super::plan_dispatch::ASK_HUMAN)
            && config.explain_turn != Some(false)
    } else {
        config
            .explain_turn
            .unwrap_or_else(|| config.requested_tools.is_empty() || is_explain_turn(&input))
    };
    Ok(PreparedAgentRevision {
        identity: revision.identity,
        invocation: tetonic_domain::AgentInvocation {
            instructions: config.instructions,
            user_input: input,
            max_steps: steps,
            explain_turn,
            empty_tool_nudge: false,
            completion_tool: "finish".into(),
            discipline: tetonic_domain::LoopDiscipline {
                handoff_tool: limits
                    .human_handoff
                    .then(|| super::plan_dispatch::ASK_HUMAN.into()),
                spawn_tool: config
                    .requested_tools
                    .iter()
                    .any(|t| t == super::plan_dispatch::DISPATCH)
                    .then(|| super::plan_dispatch::DISPATCH.into()),
                ..Default::default()
            },
        },
        requested_tools: config.requested_tools,
    })
}

fn is_explain_turn(input: &str) -> bool {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();

    const READ_ONLY: &[&str] = &[
        "read only",
        "read-only",
        "do not edit",
        "don't edit",
        "dont edit",
        "without editing",
        "no edits",
        "not edit",
    ];
    if READ_ONLY.iter().any(|k| lower.contains(k)) {
        return true;
    }

    const ACTION: &[&str] = &[
        "implement",
        "fix",
        "edit",
        "refactor",
        "create",
        "write",
        "add ",
        "change",
        "update",
        "replace",
        "build ",
        "correct",
        "bug",
        "todo",
        "notimplemented",
    ];
    if ACTION.iter().any(|kw| lower.contains(kw)) {
        return false;
    }

    if trimmed.ends_with('?') {
        return true;
    }

    const PREFIXES: &[&str] = &[
        "can ",
        "could ",
        "what ",
        "whats ",
        "what's ",
        "how ",
        "why ",
        "where ",
        "which ",
        "who ",
        "when ",
        "show ",
        "list ",
        "tell ",
        "explain ",
        "describe ",
        "summarize ",
        "summary ",
        "walk me through",
        "help me understand",
        "is ",
        "are ",
        "do ",
        "does ",
        "will ",
        "would ",
        "should ",
    ];
    if PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }

    lower.contains("tell me") || lower.contains("about")
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
                    human_handoff: false,
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
            human_handoff: false,
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

    #[test]
    fn test_is_explain_turn_detection() {
        assert!(is_explain_turn("can we show which tools are available"));
        assert!(is_explain_turn("what tools do you have?"));
        assert!(is_explain_turn("list all tools"));
        assert!(is_explain_turn("explain the architecture"));
        assert!(is_explain_turn("describe how this works"));
        assert!(is_explain_turn("how does the parser work?"));
        assert!(is_explain_turn("do not edit, just explain"));
        assert!(!is_explain_turn("fix the bug in parser.rs"));
        assert!(!is_explain_turn("implement user authentication"));
        assert!(!is_explain_turn("edit src/main.rs"));
        assert!(!is_explain_turn("user task"));
    }

    #[tokio::test]
    async fn human_handoff_accepts_plain_worker_answers_without_bypassing_completion_guards() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let credential = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        for (i, config, conversational) in [
            (
                0,
                serde_json::json!({"instructions":"Compare options","requested_tools":["finish"]}),
                true,
            ),
            (
                1,
                serde_json::json!({"instructions":"Coordinate","requested_tools":["finish",super::super::plan_dispatch::DISPATCH],"explain_turn":true}),
                false,
            ),
            (
                2,
                serde_json::json!({"instructions":"Work","requested_tools":["finish","run_shell"],"explain_turn":true}),
                false,
            ),
            (
                3,
                serde_json::json!({"instructions":"Explicit completion required","requested_tools":["finish"],"explain_turn":false}),
                false,
            ),
        ] {
            let revision = local
                .resources()
                .register_agent(
                    credential.expose_secret(),
                    "org".into(),
                    format!("agent-{i}"),
                    "general".into(),
                    config,
                )
                .await
                .unwrap();
            let prepared = prepare(
                revision,
                "Please do the assignment".into(),
                HarnessPreparationLimits {
                    human_handoff: true,
                    max_steps: 8,
                    max_input_bytes: 1024,
                },
            )
            .unwrap();
            assert_eq!(prepared.invocation.explain_turn, conversational);
            assert_eq!(
                prepared.invocation.discipline.handoff_tool.as_deref(),
                Some(super::super::plan_dispatch::ASK_HUMAN)
            );
            assert!(prepared
                .requested_tools
                .contains(&super::super::plan_dispatch::ASK_HUMAN.into()));
        }
    }
}
