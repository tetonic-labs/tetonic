//! Employee launch of a registered job. Host settings are supplied by the
//! operator composition, not by the job request. The receipt is a locator and,
//! when this process owns execution, the terminal outcome. It never reports Running.
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tetonic_domain::CandidateOutcome;
use tetonic_egress::EgressGuard;

use crate::commands::InitializeCommand;
use crate::errors::AppError;
use crate::events::NoopEventSink;
use crate::resources::{
    LocalControl, RegisteredAgentExecution, RegisteredAgentJob, RegisteredAgentSubmission,
    RegisteredExecutionSettings, TeamWorkLaunch,
};
use crate::Application;

pub struct RegisteredLaunchHost {
    pub database: PathBuf,
    pub audience: String,
    pub ollama: String,
    pub settings: RegisteredExecutionSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisteredLaunchReceipt {
    pub run_id: String,
    pub task_id: String,
    pub audit_session_id: String,
    /// True only for the winning launch. A retry returns locators and does not
    /// attach another execution owner.
    pub launched: bool,
    /// Terminal journal state. Absent while another owner may still be executing.
    /// Never reports running.
    pub outcome: Option<String>,
    /// Durable journal locators. Payloads stay in the run log. A retry returns the
    /// same sequence and digests and does not append another copy.
    pub events: Vec<RegisteredEventReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisteredEventReceipt {
    pub sequence: u64,
    pub event_type: String,
    pub payload_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSettingsFile {
    #[serde(default)]
    workspace: Option<PathBuf>,
    ollama: String,
    model: String,
    num_ctx: usize,
    max_elapsed_seconds: u64,
    #[serde(default)]
    reported_token_ceiling: Option<u64>,
    data_class: tetonic_domain::DataClass,
    allowed_tools: Vec<String>,
    max_steps: usize,
    max_input_bytes: usize,
}

/// Operator file only. Unknown fields are rejected so an employee payload cannot
/// smuggle a tool ceiling or workspace into this document.
pub fn host_settings_from_json(
    bytes: &[u8],
    database: PathBuf,
    audience: String,
) -> Result<RegisteredLaunchHost, AppError> {
    let file: HostSettingsFile = serde_json::from_slice(bytes)
        .map_err(|_| AppError::InvalidRequest("invalid host settings".into()))?;
    Ok(RegisteredLaunchHost {
        database,
        audience,
        ollama: file.ollama,
        settings: RegisteredExecutionSettings {
            max_elapsed_seconds: file.max_elapsed_seconds,
            reported_token_ceiling: file.reported_token_ceiling,
            workspace_root: file.workspace,
            model: file.model,
            num_ctx: file.num_ctx,
            data_class: file.data_class,
            allowed_tools: file.allowed_tools.into_iter().collect(),
            limits: crate::resources::HarnessPreparationLimits {
                max_steps: file.max_steps,
                max_input_bytes: file.max_input_bytes,
            },
        },
    })
}

pub async fn launch_registered_job(
    credential: &str,
    host: RegisteredLaunchHost,
    job: RegisteredAgentJob,
) -> Result<RegisteredLaunchReceipt, AppError> {
    let prepared = prepare_launch(credential, host).await?;
    tokio::task::LocalSet::new()
        .run_until(async move {
            let submission = prepared
                .app
                .submit_registered_job(
                    &prepared.credential,
                    prepared.verifier,
                    job,
                    prepared.settings,
                )
                .await?;
            finish_launch(&prepared.app, submission).await
        })
        .await
}

/// Activate a durable team work item through the same registered managed path.
pub async fn launch_team_work(
    credential: &str,
    host: RegisteredLaunchHost,
    launch: TeamWorkLaunch,
) -> Result<(tetonic_memory::TeamWorkItem, RegisteredLaunchReceipt), AppError> {
    let prepared = prepare_launch(credential, host).await?;
    tokio::task::LocalSet::new()
        .run_until(async move {
            let (work, submission) = prepared
                .app
                .activate_team_work(
                    &prepared.credential,
                    prepared.verifier,
                    launch,
                    prepared.settings,
                )
                .await?;
            let receipt = finish_launch(&prepared.app, submission).await?;
            Ok((work, receipt))
        })
        .await
}

struct PreparedLaunch {
    app: Arc<Application>,
    credential: String,
    verifier: Arc<dyn crate::resources::CredentialVerifier>,
    settings: RegisteredExecutionSettings,
}

async fn prepare_launch(
    credential: &str,
    host: RegisteredLaunchHost,
) -> Result<PreparedLaunch, AppError> {
    validate_host(&host)?;
    let local = LocalControl::open(host.database.clone(), host.audience)
        .await
        .map_err(|error| AppError::PersistenceFailed(error.to_string()))?;
    let store = tetonic_memory::SharedStore::open(&host.database, 2)
        .map_err(|error| AppError::PersistenceFailed(error.to_string()))?;
    let workspace = host.settings.workspace_root.clone().unwrap_or_else(|| {
        host.database
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir)
    });
    let (app, _, bootstrap) = Application::bootstrap(
        InitializeCommand {
            workspace_root: workspace.display().to_string(),
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        },
        Some(store.clone()),
        Arc::new(NoopEventSink),
        None,
        None,
    )
    .await?;
    let app = Arc::new(app);
    let guard = match loopback_port(&host.ollama) {
        Some(port) => Arc::new(EgressGuard::loopback_inference(port)),
        None => Arc::new(EgressGuard::new()),
    };
    let plane = crate::build_compute_plane(crate::ComputePlaneRequest {
        guard: guard.clone(),
        ollama_base: host.ollama.clone(),
        policy: bootstrap.policy.clone(),
        workspace_root: workspace,
        artifact_store: bootstrap.runtime.artifact_store().clone(),
        store: Some(store),
        coordinator: None,
        placement_sink: None,
        previous_pooled: None,
    })
    .await;
    app.install_compute_services(&plane);
    app.attach_egress(guard, host.ollama);
    Ok(PreparedLaunch {
        app,
        credential: credential.to_string(),
        verifier: local.credentials().clone(),
        settings: host.settings,
    })
}

async fn finish_launch(
    app: &Application,
    submission: RegisteredAgentSubmission,
) -> Result<RegisteredLaunchReceipt, AppError> {
    let run_id = submission.run_id.0.clone();
    let mut receipt = finish_submission(submission).await?;
    let snapshot = app
        .runs
        .inspect_run(crate::commands::InspectRunCommand {
            run_id: run_id.clone(),
        })
        .await?;
    if receipt.outcome.is_none() {
        receipt.outcome = terminal_run_outcome(snapshot.state);
    }
    receipt.events = match app
        .runs
        .resume_events(crate::commands::ResumeRunEventsCommand {
            run_id,
            after_sequence: 0,
            limit: Some(1024),
        })
        .await?
    {
        Ok(events) => event_receipts(&events),
        Err(_) => {
            return Err(AppError::PersistenceFailed(
                "registered event replay unavailable".into(),
            ))
        }
    };
    Ok(receipt)
}

fn validate_host(host: &RegisteredLaunchHost) -> Result<(), AppError> {
    let settings = &host.settings;
    if host.audience.trim().is_empty()
        || host.ollama.trim().is_empty()
        || settings.model.is_empty()
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
    Ok(())
}

fn loopback_port(ollama: &str) -> Option<u16> {
    let rest = ollama
        .strip_prefix("http://")
        .or_else(|| ollama.strip_prefix("https://"))?;
    let (host, port) = rest.split('/').next()?.rsplit_once(':')?;
    let ip = host.parse::<IpAddr>().ok();
    let loopback = host == "localhost" || ip.is_some_and(|ip| ip.is_loopback());
    loopback.then(|| port.parse().ok()).flatten()
}

fn event_receipts(events: &[tetonic_domain::RunEventEnvelope]) -> Vec<RegisteredEventReceipt> {
    let mut receipts: Vec<_> = events
        .iter()
        .map(|event| RegisteredEventReceipt {
            sequence: event.sequence,
            event_type: event_type_name(&event.event_type),
            payload_digest: event.payload_digest.0.clone(),
        })
        .collect();
    receipts.sort_by_key(|event| event.sequence);
    receipts
}

fn event_type_name(event: &tetonic_domain::EventType) -> String {
    match event {
        tetonic_domain::EventType::RunCreated => "run_created",
        tetonic_domain::EventType::TaskAdded => "task_added",
        tetonic_domain::EventType::DependencyAdded => "dependency_added",
        tetonic_domain::EventType::TaskTransitioned => "task_transitioned",
        tetonic_domain::EventType::AttemptLeased => "attempt_leased",
        tetonic_domain::EventType::AttemptCompleted => "attempt_completed",
        tetonic_domain::EventType::CancellationRequested => "cancellation_requested",
        tetonic_domain::EventType::ArtifactAccepted => "artifact_accepted",
        tetonic_domain::EventType::ApprovalRequested => "approval_requested",
        tetonic_domain::EventType::ApprovalResolved => "approval_resolved",
        tetonic_domain::EventType::WorkspaceTransactionCommitted => {
            "workspace_transaction_committed"
        }
        tetonic_domain::EventType::Other(name) if is_journal_event_name(name) => name.as_str(),
        tetonic_domain::EventType::Other(_) => "other",
    }
    .to_string()
}

/// Journal names are static identifiers such as `attempt.started`. Anything
/// else stays `other` so a payload cannot be repeated in the receipt.
fn is_journal_event_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

async fn finish_submission(
    submission: RegisteredAgentSubmission,
) -> Result<RegisteredLaunchReceipt, AppError> {
    let RegisteredAgentSubmission {
        run_id,
        task_id,
        audit_session_id,
        execution,
    } = submission;
    let launched = execution.is_some();
    let outcome = match execution {
        Some(RegisteredAgentExecution { completion, .. }) => {
            let result = completion
                .await
                .map_err(|_| AppError::PersistenceFailed("registered execution dropped".into()))?;
            Some(terminal_outcome(&result.outcome))
        }
        None => None,
    };
    Ok(RegisteredLaunchReceipt {
        run_id: run_id.0,
        task_id: task_id.0,
        audit_session_id,
        launched,
        outcome,
        events: Vec::new(),
    })
}

/// Terminal journal state only. Active, canceling, and recovery stay absent
/// rather than being reported as running.
fn terminal_run_outcome(state: tetonic_domain::RunState) -> Option<String> {
    match state {
        tetonic_domain::RunState::Succeeded => Some("completed".into()),
        tetonic_domain::RunState::Failed => Some("failed".into()),
        tetonic_domain::RunState::Canceled => Some("canceled".into()),
        tetonic_domain::RunState::Created
        | tetonic_domain::RunState::Active
        | tetonic_domain::RunState::Canceling
        | tetonic_domain::RunState::RecoveryRequired => None,
    }
}

fn terminal_outcome(outcome: &CandidateOutcome) -> String {
    match outcome {
        CandidateOutcome::Completed { .. } => "completed".into(),
        CandidateOutcome::Canceled { .. } => "canceled".into(),
        CandidateOutcome::Limited { .. } => "limited".into(),
        CandidateOutcome::Failed { .. } => "failed".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{ContextOwner, HarnessPreparationLimits};
    use tetonic_domain::ExecutionScope;

    #[test]
    fn activation_receipt_names_the_journal_event_and_hides_a_payload() {
        assert_eq!(
            event_type_name(&tetonic_domain::EventType::Other("attempt.started".into())),
            "attempt.started"
        );
        assert_eq!(
            event_type_name(&tetonic_domain::EventType::Other(
                "PRIVATECANARY in the tool body".into()
            )),
            "other"
        );
        assert_eq!(
            event_type_name(&tetonic_domain::EventType::RunCreated),
            "run_created"
        );
        let shown = event_type_name(&tetonic_domain::EventType::Other(
            "PRIVATECANARY in the tool body".into(),
        ));
        assert!(!shown.contains("PRIVATECANARY"));
        assert!(!shown.eq_ignore_ascii_case("running"));
    }

    #[tokio::test]
    async fn launch_reports_a_terminal_outcome_and_rejects_bad_host_settings() {
        let host = RegisteredLaunchHost {
            database: PathBuf::from("unused"),
            audience: "test".into(),
            ollama: "http://127.0.0.1:9".into(),
            settings: RegisteredExecutionSettings {
                max_elapsed_seconds: 0,
                reported_token_ceiling: None,
                workspace_root: Some(PathBuf::from(".")),
                model: "qwen3.5:latest".into(),
                num_ctx: 8192,
                data_class: tetonic_domain::DataClass::RepositorySource,
                allowed_tools: ["read_file".into()].into_iter().collect(),
                limits: HarnessPreparationLimits {
                    max_steps: 3,
                    max_input_bytes: 1024,
                },
            },
        };
        let job = RegisteredAgentJob {
            request_id: "request-1".into(),
            organization_id: "org".into(),
            information_context_id: "private".into(),
            agent_key: "agent".into(),
            definition_digest: "digest".into(),
            execution_grant_id: "grant".into(),
            input: "Read fixture.txt".into(),
            recovery_id: "job".into(),
        };
        assert!(matches!(
            launch_registered_job("credential", host, job).await,
            Err(AppError::InvalidRequest(_))
        ));
        assert!(matches!(
            host_settings_from_json(
                br#"{"workspace":".","ollama":"http://127.0.0.1:9","model":"m","num_ctx":8192,"max_elapsed_seconds":30,"data_class":"repository_source","allowed_tools":["read_file"],"max_steps":3,"max_input_bytes":1024,"allow_shell":true}"#,
                PathBuf::from("db"),
                "test".into(),
            ),
            Err(AppError::InvalidRequest(_))
        ));

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("fixture.txt"), "cobalt orchard").unwrap();
        let database = dir.path().join("control.db");
        let local = LocalControl::open(database.clone(), "test".into())
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
        let secret = credential.expose_secret();
        let resources = local.resources();
        let registered = resources.register_agent(secret, "org".into(), "agent".into(), "general".into(),
            serde_json::json!({"instructions":"Read the fixture","requested_tools":["read_file"],"max_steps":3})).await.unwrap();
        let limits = || HarnessPreparationLimits {
            max_steps: 3,
            max_input_bytes: 1024,
        };
        let prepared = resources
            .prepare_general_revision(
                secret,
                "org".into(),
                "agent".into(),
                registered.identity.bound_definition_digest.clone(),
                "Read fixture.txt".into(),
                limits(),
            )
            .await
            .unwrap();
        local
            .contexts()
            .create(
                secret,
                "private".into(),
                ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .await
            .unwrap();
        let command = prepared.start_command("job".into()).unwrap();
        resources
            .issue_execution_grant(
                secret,
                tetonic_memory::ExecutionGrant {
                    grant_id: "grant".into(),
                    scope: ExecutionScope {
                        principal_id: "admin".into(),
                        organization_id: "org".into(),
                        information_context_id: "private".into(),
                    },
                    job: command.job_spec,
                    expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            )
            .await
            .unwrap();
        let (url, _requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "read_file",
            serde_json::json!({"path":"fixture.txt"}),
            false,
        )
        .await;
        let digest = registered.identity.bound_definition_digest.clone();
        let job = || RegisteredAgentJob {
            request_id: "request-1".into(),
            organization_id: "org".into(),
            information_context_id: "private".into(),
            agent_key: "agent".into(),
            definition_digest: digest.clone(),
            execution_grant_id: "grant".into(),
            input: "Read fixture.txt".into(),
            recovery_id: "job".into(),
        };
        let host = || RegisteredLaunchHost {
            database: database.clone(),
            audience: "test".into(),
            ollama: url.clone(),
            settings: RegisteredExecutionSettings {
                max_elapsed_seconds: 30,
                reported_token_ceiling: None,
                workspace_root: Some(workspace.clone()),
                model: "qwen3.5:latest".into(),
                num_ctx: 8192,
                data_class: tetonic_domain::DataClass::RepositorySource,
                allowed_tools: ["read_file".into()].into_iter().collect(),
                limits: limits(),
            },
        };
        let receipt = launch_registered_job(secret, host(), job()).await.unwrap();
        let retry = launch_registered_job(secret, host(), job()).await.unwrap();
        server.abort();
        assert!(receipt.launched);
        assert_eq!(receipt.outcome.as_deref(), Some("completed"));
        assert!(!receipt.run_id.is_empty());
        assert!(!retry.launched);
        assert_eq!(retry.run_id, receipt.run_id);
        assert_eq!(retry.outcome.as_deref(), Some("completed"));
        assert!(!receipt.events.is_empty(), "a completed job must record events");
        assert_eq!(retry.events, receipt.events);
        for encoded in [
            serde_json::to_string(&receipt).unwrap(),
            serde_json::to_string(&retry).unwrap(),
        ] {
            assert!(!encoded.to_ascii_lowercase().contains("running"));
            assert!(
                !encoded.contains("Read fixture.txt"),
                "event receipt must not include the job input"
            );
        }
    }
}
