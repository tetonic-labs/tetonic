//! Loopback-only adapter for the owner-controlled local workspace.
mod contract;
use clap::Parser;
use contract::{application_error, error};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    body::{Bytes, Incoming},
    Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::Deserialize;
use std::{convert::Infallible, net::Ipv4Addr, path::PathBuf, rc::Rc, time::Duration};
use tetonic_app::local_workspace::{
    AmendPlanAssignment, AnswerPlanQuestion, BudgetSettingsRequest, ContinuePlan, CreateLocalAgent,
    CreateWorkItemRequest, ImportSkill, LocalWorkspace, PlanCommand, RemoveProviderKey,
    ResolveApprovalRequest, RevokeSkill, SaveProviderKey, SaveWorkBrief, StartPlan,
    UpdateLocalAgent, WorkPurpose,
};

#[derive(Parser)]
#[command(
    name = "tetonic ui",
    about = "Connect the web UI to a bounded local assistant"
)]
pub struct UiCli {
    /// Operator JSON configuration for storage, sanitized logging and telemetry.
    #[arg(long)]
    host_config: Option<PathBuf>,
    /// Dedicated local workspace database. Filesystem ownership is administrative authority.
    #[arg(long)]
    database: PathBuf,
    /// An already installed local Ollama model. No model is downloaded.
    #[arg(long)]
    model: String,
    #[arg(long, default_value = "http://127.0.0.1:11434")]
    ollama: String,
    #[arg(long, default_value_t = 3000)]
    port: u16,
    #[arg(long, default_value = "http://127.0.0.1:5173")]
    ui_origin: String,
    /// Optional owner-local connection URL file. Treat its contents as a credential.
    #[arg(long)]
    connection_file: Option<PathBuf>,
    /// Explicit folder grant for file tools. Omit to grant no filesystem tools.
    #[arg(long)]
    workspace_root: Option<PathBuf>,
    /// Operator-approved local HTTP MCP servers and exact vetted read tool names.
    #[arg(long)]
    mcp_config: Option<PathBuf>,
}

struct State {
    workspace: Rc<LocalWorkspace>,
    token: String,
    origin: String,
    host: String,
}

pub async fn dispatch(args: UiCli) -> anyhow::Result<()> {
    let configuration = match args.host_config {
        Some(path) => {
            let path = tokio::fs::canonicalize(path).await?;
            let mut configuration =
                tetonic_app::host::HostConfiguration::from_json(&tokio::fs::read(&path).await?)
                    .map_err(|e| anyhow::anyhow!(e.employee_message()))?;
            if let Some(base) = path.parent() {
                configuration.resolve_relative_paths(base);
            }
            configuration
        }
        None => Default::default(),
    };
    let _diagnostics = tetonic_app::tetonic_telemetry::host::install_host_diagnostics(
        &configuration.logging,
        &configuration.telemetry,
    )
    .map_err(|e| anyhow::anyhow!("host diagnostics: {e}"))?;
    let origin = local_origin(&args.ui_origin)?;
    let ollama = local_origin(&args.ollama)?;
    if let Some(parent) = args.database.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await?;
    }
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, args.port)).await?;
    let address = listener.local_addr()?;
    // Opening a UI is not permission to grant the process working directory.
    let workspace_root = args.workspace_root;
    let mut workspace = LocalWorkspace::open_with_configuration(
        args.database,
        args.model,
        ollama,
        workspace_root,
        configuration,
    )
    .await
    .map_err(|error| anyhow::anyhow!(error.employee_message()))?;
    if let Some(path) = args.mcp_config {
        let bytes = tokio::fs::read(path).await?;
        workspace = workspace
            .with_mcp_config(&bytes)
            .map_err(|e| anyhow::anyhow!(e.employee_message()))?;
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let connection = format!("{origin}/?engine=local#connect={token}");
    if let Some(path) = args.connection_file {
        tokio::fs::write(path, &connection).await?;
        println!(
            "Local engine listening on {address}. Connection URL saved to the requested file."
        );
    } else {
        println!("Local engine listening on {address}. Open this owner-only connection URL:\n{connection}");
    }
    tokio::task::LocalSet::new()
        .run_until(async move {
            let state = Rc::new(State {
                workspace: Rc::new(workspace),
                token,
                origin,
                host: address.to_string(),
            });
            let connections = std::sync::Arc::new(tokio::sync::Semaphore::new(32));
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (stream, _) = result?;
                        let Ok(permit) = connections.clone().try_acquire_owned() else { continue; };
                        let state = state.clone();
                        tokio::task::spawn_local(async move {
                            let _permit = permit;
                            let service = hyper::service::service_fn(move |request| {
                                let state = state.clone();
                                async move { Ok::<_, Infallible>(handle(&state, request).await) }
                            });
                            let _ = hyper::server::conn::http1::Builder::new()
                                .keep_alive(false).timer(TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(5))
                                .max_buf_size(16 * 1024)
                                .serve_connection(TokioIo::new(stream), service).await;
                        });
                    }
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
}

fn local_origin(value: &str) -> anyhow::Result<String> {
    let url = url::Url::parse(value)?;
    anyhow::ensure!(
        url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "expected a loopback HTTP origin"
    );
    Ok(url.origin().ascii_serialization())
}

fn authorized(headers: &hyper::HeaderMap, token: &str, origin: &str, host: &str) -> bool {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let same_secret = bearer.len() == token.len()
        && bearer
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0;
    same_secret
        && headers.get("host").and_then(|v| v.to_str().ok()) == Some(host)
        && headers
            .get("origin")
            .is_none_or(|v| v.to_str().ok() == Some(origin))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submit {
    #[serde(default)]
    work_team: Option<tetonic_app::local_workspace::WorkTeamSelection>,
    #[serde(default)]
    purpose: WorkPurpose,
    #[serde(default)]
    parent_id: Option<String>,
    request_id: String,
    input: String,
    #[serde(default)]
    agent_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateWorkItemRequest {
    #[serde(default)]
    notes: Option<Vec<String>>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    lead_id: Option<String>,
    #[serde(default)]
    agent_ids: Option<Vec<String>>,
}

async fn handle(state: &State, request: Request<Incoming>) -> Response<Full<Bytes>> {
    if !authorized(request.headers(), &state.token, &state.origin, &state.host) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Reconnect using the local engine's connection link.",
        );
    }
    if let Some(failure) = contract::check_version(request.headers()) {
        return failure;
    }
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if request.uri().query().is_some() {
        return error(StatusCode::BAD_REQUEST, "Unexpected query parameters.");
    }
    let result = if method == hyper::Method::GET && path == "/api/local/workspace" {
        state
            .workspace
            .snapshot()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path == "/api/local/agent-catalog" {
        state
            .workspace
            .agent_catalog()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path.starts_with("/api/local/skills/") {
        state
            .workspace
            .workspace_skill_content(&path["/api/local/skills/".len()..])
            .map(|content| serde_json::json!({ "content": content }))
    } else if method == hyper::Method::POST && path.starts_with("/api/local/mcp-discover/") {
        state
            .workspace
            .discover_mcp(&path["/api/local/mcp-discover/".len()..])
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path.starts_with("/api/local/provider-models/") {
        state
            .workspace
            .provider_models(&path["/api/local/provider-models/".len()..])
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path == "/api/local/work-items" {
        state
            .workspace
            .work_items()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path == "/api/local/approvals" {
        state
            .workspace
            .approvals()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path == "/api/local/capability-policies" {
        state
            .workspace
            .capability_policies()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path == "/api/local/work-teams" {
        state
            .workspace
            .work_teams()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::GET && path == "/api/local/teams" {
        state
            .workspace
            .teams()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::POST && path == "/api/local/digest" {
        state
            .workspace
            .digest()
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else if method == hyper::Method::POST
        && matches!(
            path.as_str(),
            "/api/local/tasks"
                | "/api/local/work-teams"
                | "/api/local/capability-policies"
                | "/api/local/agents"
                | "/api/local/agents/update"
                | "/api/local/skills"
                | "/api/local/mcp-connections"
                | "/api/local/skills/revoke"
                | "/api/local/work-items"
                | "/api/local/provider-key"
                | "/api/local/provider-key/remove"
                | "/api/local/budget-settings"
                | "/api/local/blackboard/query"
        )
    {
        if request
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
        {
            return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON is required.");
        }
        let body = match tokio::time::timeout(
            Duration::from_secs(5),
            Limited::new(request.into_body(), 65_536).collect(),
        )
        .await
        {
            Ok(Ok(body)) => body.to_bytes(),
            _ => {
                return error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Request body is too large or incomplete.",
                )
            }
        };
        if path == "/api/local/blackboard/query" {
            let Ok(query) =
                serde_json::from_slice::<tetonic_app::local_workspace::BlackboardQuery>(&body)
            else {
                return error(StatusCode::BAD_REQUEST, "Invalid Blackboard query.");
            };
            state
                .workspace
                .blackboard(query)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/mcp-connections" {
            let Ok(payload) =
                serde_json::from_slice::<tetonic_app::local_workspace::SaveMcpConnection>(&body)
            else {
                return error(StatusCode::BAD_REQUEST, "Invalid connection settings.");
            };
            state
                .workspace
                .save_mcp_connection(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/capability-policies" {
            let Ok(payload) =
                serde_json::from_slice::<tetonic_app::local_workspace::SaveCapabilityPolicy>(&body)
            else {
                return error(StatusCode::BAD_REQUEST, "Invalid capability permissions.");
            };
            state
                .workspace
                .save_capability_policy(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/work-teams" {
            let Ok(payload) =
                serde_json::from_slice::<tetonic_app::local_workspace::SaveWorkTeam>(&body)
            else {
                return error(StatusCode::BAD_REQUEST, "Invalid team settings.");
            };
            state
                .workspace
                .save_work_team(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/skills" {
            let Ok(payload) = serde_json::from_slice::<ImportSkill>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid skill import.");
            };
            state
                .workspace
                .import_skill(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/skills/revoke" {
            let Ok(payload) = serde_json::from_slice::<RevokeSkill>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid skill revocation.");
            };
            state
                .workspace
                .revoke_skill(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/budget-settings" {
            let Ok(payload) = serde_json::from_slice::<BudgetSettingsRequest>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid budget settings.");
            };
            state
                .workspace
                .set_budget_settings(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/provider-key/remove" {
            let Ok(payload) = serde_json::from_slice::<RemoveProviderKey>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid provider request.");
            };
            state
                .workspace
                .remove_provider_key(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/provider-key" {
            let Ok(payload) = serde_json::from_slice::<SaveProviderKey>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid provider key request.");
            };
            state
                .workspace
                .save_provider_key(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/agents/update" {
            let Ok(payload) = serde_json::from_slice::<UpdateLocalAgent>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid agent edit.");
            };
            state
                .workspace
                .update_agent(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/agents" {
            let Ok(payload) = serde_json::from_slice::<CreateLocalAgent>(&body) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "Invalid agent request. Only supported local settings are accepted.",
                );
            };
            state
                .workspace
                .create_agent(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if path == "/api/local/work-items" {
            let Ok(payload) = serde_json::from_slice::<CreateWorkItemRequest>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid work item request.");
            };
            state
                .workspace
                .create_work_item(payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            let Ok(payload) = serde_json::from_slice::<Submit>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid task request.");
            };
            state
                .workspace
                .submit_to_team(
                    payload.request_id,
                    payload.input,
                    payload
                        .agent_key
                        .unwrap_or_else(|| "Local assistant".into()),
                    payload.parent_id,
                    payload.purpose,
                    payload.work_team,
                )
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        }
    } else if method == hyper::Method::POST
        && path.starts_with("/api/local/plans/")
        && (path.ends_with("/start") || path.ends_with("/continue"))
    {
        let continuation = path.ends_with("/continue");
        let id = path
            .trim_start_matches("/api/local/plans/")
            .trim_end_matches(if continuation { "/continue" } else { "/start" });
        if request
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
        {
            return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON is required.");
        }
        let body = match tokio::time::timeout(
            Duration::from_secs(5),
            Limited::new(request.into_body(), 4096).collect(),
        )
        .await
        {
            Ok(Ok(body)) => body.to_bytes(),
            _ => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "Invalid or incomplete plan start request.",
                )
            }
        };
        if continuation {
            let Ok(payload) = serde_json::from_slice::<ContinuePlan>(&body) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "Invalid plan continuation request.",
                );
            };
            state
                .workspace
                .continue_plan(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            let Ok(payload) = serde_json::from_slice::<StartPlan>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid plan start request.");
            };
            state
                .workspace
                .start_plan(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        }
    } else if method == hyper::Method::POST
        && ((path.starts_with("/api/local/plans/") && path.ends_with("/direction"))
            || (path.starts_with("/api/local/tasks/") && path.ends_with("/answer")))
    {
        if request
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
        {
            return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON is required.");
        }
        let body = match tokio::time::timeout(
            Duration::from_secs(5),
            Limited::new(request.into_body(), 16_384).collect(),
        )
        .await
        {
            Ok(Ok(body)) => body.to_bytes(),
            _ => {
                return error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Request is too large or incomplete.",
                )
            }
        };
        if let Some(id) = path
            .strip_prefix("/api/local/plans/")
            .and_then(|p| p.strip_suffix("/direction"))
        {
            let Ok(payload) = serde_json::from_slice::<AmendPlanAssignment>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid direction change.");
            };
            state
                .workspace
                .amend_plan_assignment(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            let id = path
                .strip_prefix("/api/local/tasks/")
                .and_then(|p| p.strip_suffix("/answer"))
                .unwrap();
            let Ok(payload) = serde_json::from_slice::<AnswerPlanQuestion>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid answer.");
            };
            state
                .workspace
                .answer_plan_question(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        }
    } else if let Some(id) = path.strip_prefix("/api/local/plans/") {
        if method == hyper::Method::GET {
            state
                .workspace
                .plan_view(id)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if method == hyper::Method::POST {
            if request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                != Some("application/json")
            {
                return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON is required.");
            }
            let body = match tokio::time::timeout(
                Duration::from_secs(5),
                Limited::new(request.into_body(), 65_536).collect(),
            )
            .await
            {
                Ok(Ok(body)) => body.to_bytes(),
                _ => {
                    return error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Plan is too large or incomplete.",
                    )
                }
            };
            let Ok(payload) = serde_json::from_slice::<PlanCommand>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid plan request.");
            };
            state
                .workspace
                .update_plan(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            return error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method.");
        }
    } else if let Some(id) = path.strip_prefix("/api/local/briefs/") {
        if method == hyper::Method::GET {
            state
                .workspace
                .work_briefs(id)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if method == hyper::Method::POST {
            if request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                != Some("application/json")
            {
                return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON is required.");
            }
            let body = match tokio::time::timeout(
                Duration::from_secs(5),
                Limited::new(request.into_body(), 65_536).collect(),
            )
            .await
            {
                Ok(Ok(body)) => body.to_bytes(),
                _ => {
                    return error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Brief is too large or incomplete.",
                    )
                }
            };
            let Ok(payload) = serde_json::from_slice::<SaveWorkBrief>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid brief request.");
            };
            state
                .workspace
                .save_work_brief(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            return error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method.");
        }
    } else if let Some(id) = path.strip_prefix("/api/local/tasks/") {
        if method == hyper::Method::POST {
            let Some(id) = id.strip_suffix("/cancel") else {
                return error(StatusCode::NOT_FOUND, "Unknown action.");
            };
            state
                .workspace
                .cancel(id)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if method == hyper::Method::GET {
            state
                .workspace
                .task(id)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            return error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method.");
        }
    } else if let Some(id) = path.strip_prefix("/api/local/approvals/") {
        if method == hyper::Method::POST {
            let Some(id) = id.strip_suffix("/resolve") else {
                return error(StatusCode::NOT_FOUND, "Unknown approval action.");
            };
            let body = match tokio::time::timeout(
                Duration::from_secs(5),
                Limited::new(request.into_body(), 8192).collect(),
            )
            .await
            {
                Ok(Ok(body)) => body.to_bytes(),
                _ => return error(StatusCode::BAD_REQUEST, "Incomplete approval request body."),
            };
            let Ok(payload) = serde_json::from_slice::<ResolveApprovalRequest>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid approval payload.");
            };
            state
                .workspace
                .resolve_approval(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            return error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method.");
        }
    } else if let Some(id) = path.strip_prefix("/api/local/work-items/") {
        if method == hyper::Method::PATCH || method == hyper::Method::POST {
            let body = match tokio::time::timeout(
                Duration::from_secs(5),
                Limited::new(request.into_body(), 65_536).collect(),
            )
            .await
            {
                Ok(Ok(body)) => body.to_bytes(),
                _ => return error(StatusCode::BAD_REQUEST, "Incomplete work item body."),
            };
            let Ok(payload) = serde_json::from_slice::<UpdateWorkItemRequest>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid work item update payload.");
            };
            if let Err(e) = state
                .workspace
                .set_local_work_data(
                    id,
                    payload.notes,
                    payload.status,
                    payload.lead_id,
                    payload.agent_ids,
                )
                .await
            {
                return application_error(&e);
            }
            return json(StatusCode::OK, serde_json::json!({ "ok": true }));
        } else {
            return error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method.");
        }
    } else {
        return error(StatusCode::NOT_FOUND, "Unknown endpoint.");
    };
    match result {
        Ok(value) => json(StatusCode::OK, value),
        Err(failure) => application_error(&failure),
    }
}

fn json(status: StatusCode, value: serde_json::Value) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(value.to_string())));
    *response.status_mut() = status;
    response.headers_mut().insert(
        "x-tetonic-api-version",
        hyper::header::HeaderValue::from_static("1"),
    );
    response.headers_mut().insert(
        "content-type",
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        "cache-control",
        hyper::header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        hyper::header::HeaderValue::from_static("nosniff"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_settings_cannot_supply_usage_or_execution_authority() {
        let command =
            serde_json::json!({"request_id":"id","expected_revision":0,"token_limit":1024});
        assert!(serde_json::from_value::<BudgetSettingsRequest>(command.clone()).is_ok());
        let mut reset = command.clone();
        reset["token_limit"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<BudgetSettingsRequest>(reset)
            .unwrap()
            .token_limit
            .is_none());
        let mut missing = command.clone();
        missing.as_object_mut().unwrap().remove("token_limit");
        assert!(serde_json::from_value::<BudgetSettingsRequest>(missing).is_err());
        for field in [
            "used_tokens",
            "released_tokens",
            "work_id",
            "team_id",
            "actor",
            "grant",
        ] {
            let mut invalid = command.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<BudgetSettingsRequest>(invalid).is_err());
        }
    }
    #[test]
    fn plan_commands_cannot_accept_execution_settings_or_dispatch_actions() {
        let command = serde_json::json!({"action":"generate","request_id":"id","expected_revision":0,"brief_revision":1});
        assert!(serde_json::from_value::<PlanCommand>(command.clone()).is_ok());
        for key in ["grant", "execute", "tools", "credential", "workspace_root"] {
            let mut bad = command.clone();
            bad[key] = serde_json::json!(true);
            assert!(serde_json::from_value::<PlanCommand>(bad).is_err());
        }
        assert!(serde_json::from_value::<PlanCommand>(
            serde_json::json!({"action":"dispatch","revision":1})
        )
        .is_err());
    }
    #[test]
    fn shaping_payloads_are_explicit_and_cannot_supply_execution_authority() {
        let mut value =
            serde_json::json!({"request_id":"id","input":"Explore this","agent_key":"The Guide"});
        assert_eq!(
            serde_json::from_value::<Submit>(value.clone())
                .unwrap()
                .purpose,
            WorkPurpose::Work
        );
        value["purpose"] = serde_json::json!("explore");
        assert_eq!(
            serde_json::from_value::<Submit>(value.clone())
                .unwrap()
                .purpose,
            WorkPurpose::Explore
        );
        value["purpose"] = serde_json::json!("anything");
        assert!(serde_json::from_value::<Submit>(value).is_err());
        let brief =
            serde_json::json!({"request_id":"id","expected_revision":0,"body":"Draft only"});
        assert!(serde_json::from_value::<SaveWorkBrief>(brief.clone()).is_ok());
        for field in ["dispatch", "grant", "team_id", "actor"] {
            let mut invalid = brief.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<SaveWorkBrief>(invalid).is_err());
        }
    }
    #[test]
    fn agent_requests_reject_unimplemented_authority_fields() {
        let value = serde_json::json!({"request_id":"id","name":"Analyst","purpose":"Think",
            "model":"installed","harness":"general","max_steps":2,"max_seconds":60,"max_tokens":1024});
        assert!(serde_json::from_value::<CreateLocalAgent>(value.clone()).is_ok());
        for field in [
            "tool_ids",
            "permissions",
            "workspace_path",
            "principal",
            "team_id",
        ] {
            let mut invalid = value.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<CreateLocalAgent>(invalid).is_err());
        }
    }
    #[test]
    fn blackboard_inspection_query_accepts_scope_but_not_caller_authority() {
        use tetonic_app::local_workspace::BlackboardQuery;
        let value = serde_json::json!({"offset":20,"work_ids":["work"]});
        let query = serde_json::from_value::<BlackboardQuery>(value.clone()).unwrap();
        assert!(query.validate().is_ok());
        for field in ["actor", "org", "team", "agent_id", "audience"] {
            let mut invalid = value.clone();
            invalid[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<BlackboardQuery>(invalid).is_err());
        }
        let query = serde_json::from_value::<BlackboardQuery>(serde_json::json!({"offset":100001}))
            .unwrap();
        assert!(query.validate().is_err());
    }
    #[test]
    fn only_loopback_origins_are_accepted() {
        assert!(local_origin("http://127.0.0.1:5173").is_ok());
        for origin in [
            "https://example.com",
            "http://0.0.0.0:3000",
            "http://user@localhost",
            "http://localhost/path",
            "http://localhost/?secret=x",
        ] {
            assert!(local_origin(origin).is_err(), "{origin}");
        }
    }

    #[test]
    fn every_request_requires_the_session_and_expected_host_and_origin() {
        let mut headers = hyper::HeaderMap::new();
        let origin = "http://127.0.0.1:5173";
        let host = "127.0.0.1:3000";
        headers.insert("host", host.parse().unwrap());
        assert!(!authorized(&headers, "secret", origin, host));
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        assert!(authorized(&headers, "secret", origin, host));
        headers.insert("origin", "http://evil.example".parse().unwrap());
        assert!(!authorized(&headers, "secret", origin, host));
        headers.insert("origin", origin.parse().unwrap());
        assert!(authorized(&headers, "secret", origin, host));
        headers.insert("host", "evil.example:3000".parse().unwrap());
        assert!(!authorized(&headers, "secret", origin, host));
        assert!(!authorized(&headers, "wrong", origin, host));
    }
}
