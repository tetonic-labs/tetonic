//! Validate current authority and the exact approved settings for both launch and rebuild.
use super::*;

#[derive(Default)]
pub(super) struct RegisteredJobContext {
    pub work: Option<(String, String)>,
    pub parent: Option<tetonic_run::managed::DelegationParent>,
    pub restore: Option<tetonic_run::managed::ActivationReceipt>,
}

pub(super) enum HarnessPreparation {
    Existing(tetonic_run::managed::ActivationReceipt),
    Ready(Box<RegisteredHarnessPlan>),
}

/// Ephemeral composition, never a second run record or permission grant.
pub(super) struct RegisteredHarnessPlan {
    pub prepared: super::super::activation::PreparedRegisteredJob,
    pub settings: RegisteredExecutionSettings,
    pub workspace: Option<tetonic_tools::Workspace>,
    pub root: Option<std::path::PathBuf>,
    pub data_class: tetonic_domain::DataClass,
    pub work: Option<(String, String)>,
    pub approved_environment: bool,
    pub store: tetonic_memory::SharedStore,
    pub restore: Option<tetonic_run::managed::ActivationReceipt>,
}

impl crate::Application {
    pub(super) async fn prepare_registered_harness(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        mut settings: RegisteredExecutionSettings,
        context: RegisteredJobContext,
    ) -> Result<HarnessPreparation, AppError> {
        let RegisteredJobContext {
            work,
            parent,
            restore,
        } = context;
        if let Some(mcp) = &settings.mcp {
            settings
                .allowed_tools
                .extend(mcp.tool_names().into_iter().filter(|tool| {
                    settings.hosted.as_ref().is_none_or(|h| {
                        h.tool_disclosure
                            .as_ref()
                            .is_some_and(|d| d.tools.contains(tool))
                    })
                }));
        }
        let wants_dispatch = settings
            .allowed_tools
            .contains(super::super::plan_dispatch::DISPATCH);
        let bound_dispatch = settings
            .plan_dispatch
            .as_ref()
            .is_some_and(|d| !d.assignment_keys.is_empty());
        let bound_human = settings
            .plan_dispatch
            .as_ref()
            .is_some_and(|d| d.human.is_some());
        let director = settings
            .plan_dispatch
            .as_ref()
            .and_then(|d| d.director.as_ref());
        let bound_director = director.is_some();
        let durable_wait = settings
            .plan_dispatch
            .as_ref()
            .and_then(|d| d.human.as_ref())
            .and_then(|h| h.durable_wait_seconds);
        if durable_wait.is_some_and(|seconds| {
            !(1..=604_800).contains(&seconds)
                || parent.is_some()
                || bound_dispatch
                || bound_director
        }) {
            return Err(AppError::PolicyDenied(
                "Durable human waits require an independent registered root.".into(),
            ));
        }
        if settings.limits.work_director != bound_director
            || settings
                .allowed_tools
                .contains(super::super::work_director::CONTROL)
                != bound_director
            || (bound_director
                && (parent.is_some()
                    || bound_dispatch
                    || bound_human
                    || settings.response_schema.is_some()
                    || settings.workspace_root.is_some()
                    || settings.mcp.is_some()
                    || settings
                        .allowed_tools
                        .iter()
                        .any(|t| t != super::super::work_director::CONTROL && t != "finish")))
            || director.is_some_and(|d| work.as_ref().is_none_or(|(_, w)| w != &d.turn))
        {
            return Err(AppError::PolicyDenied(
                "Conversation planning requires its trusted, output-scoped binding.".into(),
            ));
        }
        if wants_dispatch != bound_dispatch
            || settings
                .allowed_tools
                .contains(super::super::plan_dispatch::ASK_HUMAN)
                != bound_human
            || settings.limits.human_handoff != bound_human
            || (parent.is_some() && bound_dispatch)
            || settings
                .plan_dispatch
                .as_ref()
                .is_some_and(|d| work.as_ref().is_none_or(|(_, w)| w != &d.binding))
        {
            return Err(AppError::PolicyDenied(
                "Plan dispatch requires its trusted root binding.".into(),
            ));
        }
        if settings.model.is_empty()
            || settings.max_elapsed_seconds == 0
            || settings.max_elapsed_seconds > 86_400
            || settings
                .model
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || settings.num_ctx <= 1024
            || settings.num_ctx > u32::MAX as usize
        {
            return Err(AppError::InvalidRequest(
                "invalid registered execution settings".into(),
            ));
        }
        let deadline = u64::try_from(chrono::Utc::now().timestamp())
            .ok()
            .and_then(|now| now.checked_add(settings.max_elapsed_seconds))
            .ok_or_else(|| AppError::InvalidRequest("invalid execution deadline".into()))?;
        let request_id = request.request_id.clone();
        let preparation_limits = (
            settings.limits.max_steps,
            settings.limits.max_input_bytes,
            settings.limits.human_handoff,
            settings.limits.work_director,
        );
        let environment_settings = settings.clone();
        let mut approved_environment = false;
        let mut prepared = self
            .run_manager
            .prepare_registered_job_with_parent(
                credential,
                verifier,
                request,
                settings.limits.clone(),
                parent.clone(),
            )
            .await?;
        prepared.deadline = Some(deadline);
        if let Some(parent) = &parent {
            let (team, work) = work
                .clone()
                .ok_or_else(|| resource_error(ResourceError::Denied))?;
            let (binding, scope, job, request, grant) = (
                parent.binding().clone(),
                prepared.authorization.scope.clone(),
                prepared.command.job_spec.clone(),
                request_id.clone(),
                prepared
                    .authorization
                    .grant_id
                    .clone()
                    .ok_or_else(|| resource_error(ResourceError::Denied))?,
            );
            let lineage = self
                .run_manager
                .managed()
                .store()
                .ok_or_else(|| resource_error(ResourceError::StorageRequired))?
                .read(move |db| {
                    db.require_delegated_execution_binding(
                        tetonic_memory::DelegatedExecutionBinding {
                            id: &grant,
                            scope: &scope,
                            job: &job,
                            parent_run: &binding.run_id.0,
                            parent_attempt: &binding.attempt_id.0,
                            request: &request,
                            now: chrono::Utc::now().timestamp(),
                        },
                    )
                })
                .await
                .map_err(|_| resource_error(ResourceError::Storage))?
                .map_err(|_| resource_error(ResourceError::Denied))?;
            if lineage.team_id != team || lineage.child_work_id != work {
                return Err(resource_error(ResourceError::Denied));
            }
            if let Some(binding) = lineage.approved_environment {
                if binding
                    != environment_settings
                        .environment_binding(&prepared.command.job_spec.capability_bindings)?
                {
                    return Err(AppError::PolicyDenied(
                        "The delegated agent environment differs from its approved configuration."
                            .into(),
                    ));
                }
                approved_environment = true;
            }
        }
        // Initial governed children consume explicit shared-context input. File
        // roots and hosted egress need a durable inherited environment contract
        // before they can be delegated; a tool-name grant alone is insufficient.
        if parent.is_some()
            && !approved_environment
            && (work.is_none()
                || settings.hosted.is_some()
                || settings.workspace_root.is_some()
                || settings.allowed_tools.iter().any(|tool| {
                    tool != "finish"
                        && !(bound_human && tool == super::super::plan_dispatch::ASK_HUMAN)
                })
                || prepared
                    .command
                    .job_spec
                    .capability_bindings
                    .iter()
                    .any(|tool| {
                        tool != "finish"
                            && !(bound_human && tool == super::super::plan_dispatch::ASK_HUMAN)
                    })
                || !prepared.command.job_spec.artifact_bindings.is_empty())
        {
            return Err(AppError::PolicyDenied(
                "Delegated execution currently supports explicit input and local inference only."
                    .into(),
            ));
        }
        if let Some(schema) = &settings.response_schema {
            if !schema.is_object()
                || schema.to_string().len() > 64_000
                || !prepared.command.invocation.explain_turn
                || prepared
                    .command
                    .job_spec
                    .capability_bindings
                    .iter()
                    .any(|tool| tool != "finish")
            {
                return Err(AppError::InvalidRequest(
                    "Structured answers require a bounded schema and an output-only invocation."
                        .into(),
                ));
            }
        }
        if prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .any(|tool| tool != "finish" && !settings.allowed_tools.contains(tool))
        {
            return Err(AppError::PolicyDenied(
                "requested tools exceed host grant".into(),
            ));
        }
        if let Some(tool) = prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .find(|tool| {
                !supported_registered_tool(tool)
                    && !settings.mcp.as_ref().is_some_and(|mcp| mcp.contains(tool))
            })
        {
            return Err(AppError::PolicyDenied(format!(
                "{tool} is not a supported registered isolation profile"
            )));
        }
        if prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .any(|tool| tool == "run_shell")
            && !approved_environment
            && !work.as_ref().is_some_and(|(team, _)| {
                tetonic_memory::team_participation_context_id(
                    &prepared.authorization.scope.organization_id,
                    team,
                    &prepared.authorization.scope.principal_id,
                )
                .ok()
                .as_deref()
                    == Some(prepared.authorization.scope.information_context_id.as_str())
            })
        {
            return Err(AppError::PolicyDenied(
                "run_shell requires an owner-scoped work approval profile.".into(),
            ));
        }
        let repository_requested = settings
            .allowed_tools
            .iter()
            .chain(prepared.command.job_spec.capability_bindings.iter())
            .any(|tool| {
                tool != "finish"
                    && !settings.mcp.as_ref().is_some_and(|mcp| mcp.contains(tool))
                    && tool != "recall"
                    && tool != super::super::plan_dispatch::DISPATCH
                    && tool != super::super::plan_dispatch::ASK_HUMAN
                    && tool != super::super::work_director::CONTROL
            });
        if repository_requested && settings.workspace_root.is_none() {
            return Err(AppError::WorkspaceUnavailable);
        }
        let workspace = match &settings.workspace_root {
            Some(path) => Some(
                tetonic_tools::Workspace::new(path).map_err(|_| AppError::WorkspaceUnavailable)?,
            ),
            None => None,
        };
        let root = workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf());
        let root_key = root
            .as_ref()
            .map(|path| {
                path.to_str()
                    .map(str::to_string)
                    .ok_or(AppError::WorkspaceUnavailable)
            })
            .transpose()?;
        let mut ceiling: Vec<_> = settings.allowed_tools.iter().collect();
        ceiling.sort();
        let context_id = prepared.authorization.scope.information_context_id.clone();
        let kind_store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or_else(|| resource_error(ResourceError::StorageRequired))?;
        let context_kind = kind_store
            .read(move |db| db.information_context_kind(&context_id))
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(|_| resource_error(ResourceError::Storage))?;
        // Governed private and team prompts stay on this machine. The operator's
        // requested class remains part of the fingerprint so two host classes
        // do not alias to the same activation.
        let data_class = if let Some(hosted) = &settings.hosted {
            // Data disclosure cannot broaden execution authority. The exact selected
            // tools and folder must fit both this consent and the prepared grants.
            // Recall, delegated team context and artifacts remain separately scoped.
            let capabilities = &prepared.command.job_spec.capability_bindings;
            // Internal controls expose only their host-bound work scope. Dispatch
            // selects pre-authorized assignments; it cannot grant arbitrary tools.
            // Hosted context consent never replaces a child's execution grant.
            let internal_control = |tool: &str| {
                tool == "finish"
                    || (bound_dispatch && tool == super::super::plan_dispatch::DISPATCH)
                    || (bound_human && tool == super::super::plan_dispatch::ASK_HUMAN)
                    || (bound_director && tool == super::super::work_director::CONTROL)
            };
            let valid = match &hosted.tool_disclosure {
                Some(disclosure) => {
                    disclosure.version == 1
                        && root_key == disclosure.workspace
                        && settings
                            .allowed_tools
                            .iter()
                            .all(|tool| internal_control(tool) || disclosure.tools.contains(tool))
                        && capabilities.iter().all(|tool| {
                            tool == "finish"
                                || internal_control(tool)
                                || disclosure.tools.contains(tool)
                        })
                }
                None => {
                    root_key.is_none()
                        && settings
                            .allowed_tools
                            .iter()
                            .all(|tool| internal_control(tool))
                        && capabilities
                            .iter()
                            .all(|tool| tool == "finish" || internal_control(tool))
                }
            };
            if !valid || !prepared.command.job_spec.artifact_bindings.is_empty() {
                return Err(AppError::PolicyDenied(
                    "Hosted execution exceeds the approved prompt and workspace disclosure scope."
                        .into(),
                ));
            }
            settings.data_class
        } else {
            floor_governed_context(context_kind.as_deref(), settings.data_class)
        };
        // The fingerprint covers effective host settings as well as the exact
        // granted job. Absolute time and a new audit UUID are not request inputs.
        let mut fingerprint = serde_json::json!({
            "version": 1, "scope": prepared.authorization.scope,
            "grant_id": prepared.authorization.grant_id, "job": prepared.command.job_spec,
            "workspace": root_key, "model": settings.model, "num_ctx": settings.num_ctx,
            "requested_data_class": settings.data_class, "data_class": data_class, "tools": ceiling,
            "preparation_limits": preparation_limits, "max_elapsed_seconds": settings.max_elapsed_seconds,
            "reported_token_ceiling": settings.reported_token_ceiling,
        });
        if let Some(seconds) = durable_wait {
            let human = settings
                .plan_dispatch
                .as_mut()
                .and_then(|d| d.human.as_mut())
                .ok_or_else(|| AppError::PolicyDenied("Missing human host".into()))?;
            if human.actor != prepared.authorization.scope.principal_id
                || human.org != prepared.authorization.scope.organization_id
                || work.as_ref() != Some(&(human.team.clone(), human.work.clone()))
            {
                return Err(AppError::PolicyDenied(
                    "Human host must match the approved work scope.".into(),
                ));
            }
            let (org, team, work, identity) = (
                human.org.clone(),
                human.team.clone(),
                human.work.clone(),
                prepared.command.job_spec.identity_id.0.clone(),
            );
            let stop_binding = kind_store
                .read(move |db| db.work_human_stop_binding(&org, &team, &work, &identity))
                .await
                .map_err(|_| AppError::InferenceUnavailable)?
                .map_err(|_| {
                    AppError::PolicyDenied("Human wait is stopped or unavailable.".into())
                })?;
            human.prepared_stop_binding = Some(stop_binding.clone());
            fingerprint["human_wait_seconds"] = serde_json::json!(seconds);
            fingerprint["human_wait_stops"] = serde_json::json!(stop_binding);
        }
        if let Some(dispatch) = &settings.plan_dispatch {
            fingerprint["plan_dispatch"] = serde_json::json!(dispatch.binding);
            if let Some(director) = &dispatch.director {
                fingerprint["work_director"] = serde_json::json!([
                    director.source,
                    director.turn,
                    director.revision,
                    director.brief_revision
                ]);
            }
        }
        if let Some(binding) = &work {
            fingerprint["work_budget_binding"] = serde_json::json!(binding);
        }
        if let Some(parent) = &parent {
            let binding = parent.binding();
            fingerprint["delegation_parent"] =
                serde_json::json!([binding.run_id, binding.task_id, binding.attempt_id]);
        }
        if let Some(hosted) = &settings.hosted {
            fingerprint["hosted_binding"] = serde_json::json!(hosted.binding);
        }
        if let Some(schema) = &settings.response_schema {
            fingerprint["response_schema"] = schema.clone();
        }
        let request_bytes = serde_json::to_vec(&fingerprint)
            .map_err(|_| AppError::InvalidRequest("invalid activation settings".into()))?;
        use sha2::Digest;
        let mut activation = tetonic_domain::ActivationBinding {
            request_id,
            request_digest: format!("sha256:{:x}", sha2::Sha256::digest(request_bytes)),
            audit_session_id: format!("execution-audit-{}", uuid::Uuid::new_v4()),
        };
        let receipt = if let Some(parent) = &parent {
            self.run_manager
                .managed()
                .lookup_child_activation(
                    parent,
                    &prepared.authorization,
                    &activation,
                    &prepared.command.identity,
                    &prepared.command.job_spec,
                    None,
                )
                .await?
        } else {
            self.run_manager
                .managed()
                .lookup_activation(
                    &prepared.authorization,
                    &activation,
                    &prepared.command.identity,
                    &prepared.command.job_spec,
                    None,
                )
                .await?
        };
        if let Some(expected) = &restore {
            if parent.is_some() || receipt.as_ref() != Some(expected) {
                return Err(AppError::PolicyDenied(
                    "Saved execution does not match the approved activation.".into(),
                ));
            }
            activation.audit_session_id = expected.audit_session_id.clone();
            // No refreshed deadline: managed restoration owns the saved allowance.
            prepared.deadline = None;
        } else if let Some(receipt) = receipt {
            return Ok(HarnessPreparation::Existing(receipt));
        }
        prepared.activation = Some(activation);
        Ok(HarnessPreparation::Ready(Box::new(RegisteredHarnessPlan {
            prepared,
            settings,
            workspace,
            root,
            data_class,
            work,
            approved_environment,
            store: kind_store,
            restore,
        })))
    }
}
