mod perceptive_brain;
mod context_budget;
mod experience;
mod observability;
use anyhow::{ensure, Context};
use clap::Parser;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tetonic_core::{agent::EmptyToolHost, Agent, AgentConfig};
use tetonic_domain::{Affordance, WorldManifest};
use tetonic_egress::EgressGuard;
use tetonic_inference::OllamaProvider;
use tetonic_runtime::{websocket_adapter::WebSocketWorldAdapter, SingleModelBrain};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser)]
#[command(version, about = "Run a configured Tetonic agent against a live world")]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    node: Node,
    inference: Inference,
    agent: AgentSettings,
    world: World,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    mode: String,
    #[serde(default)]
    observability: bool,
    bind: std::net::SocketAddr,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inference {
    thinking: Option<bool>,
    endpoint: String,
    model: String,
    timeout_secs: u64,
    context_tokens: usize,
    #[serde(default = "default_completion_tokens")]
    completion_tokens: u32,
    #[serde(default = "default_context_margin")]
    context_margin: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentSettings {
    id: String,
    name: String,
    charter: String,
    decision_interval_ms: u64,
    #[serde(default = "default_idle_interval")]
    idle_interval_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct World {
    url: String,
    instructions: String,
    allowed_actions: Vec<String>,
}

fn default_idle_interval() -> u64 { 30000 }
fn default_completion_tokens() -> u32 { 384 }
fn default_context_margin() -> usize { 512 }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let cfg: Config =
        toml::from_str(&std::fs::read_to_string(&args.config).context("read config")?)
            .context("parse config")?;
    ensure!(
        cfg.node.mode == "standalone",
        "this release supports standalone mode only"
    );
    ensure!(
        cfg.node.bind.ip().is_loopback(),
        "local experiment requires a loopback bind"
    );
    ensure!(
        cfg.agent.decision_interval_ms >= 1000,
        "decision interval must be at least 1000ms"
    );
    ensure!(
        !cfg.agent.id.is_empty() && !cfg.world.allowed_actions.is_empty(),
        "agent id and allowed actions are required"
    );
    ensure!(
        cfg.inference.timeout_secs > 0 && cfg.inference.context_tokens >= 1024,
        "invalid inference limits"
    );
    ensure!(cfg.inference.completion_tokens > 0 && cfg.inference.context_tokens.saturating_sub(cfg.inference.context_margin) > cfg.inference.completion_tokens as usize, "context must leave space for input, completion and safety margin");
    let inference_url = url::Url::parse(&cfg.inference.endpoint)?;
    ensure!(
        inference_url.scheme() == "http"
            && inference_url
                .host_str()
                .is_some_and(|s| s == "127.0.0.1" || s == "localhost" || s == "[::1]"),
        "local experiment requires loopback HTTP inference"
    );
    let guard = Arc::new(EgressGuard::loopback_inference(
        inference_url.port_or_known_default().unwrap_or(11434),
    ));
    let provider = Arc::new(
        OllamaProvider::new(cfg.inference.endpoint.trim_end_matches('/'), guard)
            .with_thinking(cfg.inference.thinking),
    );
    let mut world_url = url::Url::parse(&cfg.world.url)?;
    ensure!(
        world_url.scheme() == "ws"
            && world_url
                .host_str()
                .is_some_and(|s| s == "127.0.0.1" || s == "localhost" || s == "[::1]"),
        "local experiment requires loopback WebSocket world"
    );
    world_url
        .query_pairs_mut()
        .append_pair("agent_id", &cfg.agent.id)
        .append_pair("name", &cfg.agent.name)
        .append_pair("event_protocol", "1");
    let manifest = cfg
        .world
        .allowed_actions
        .iter()
        .fold(WorldManifest::new("configured-world", "1"), |m, kind| {
            m.with_affordance(Affordance::instant(kind, "configured action"))
        });
    let cadence = Duration::from_millis(cfg.agent.decision_interval_ms);
    let listener = tokio::net::TcpListener::bind(cfg.node.bind)
        .await
        .context("bind health endpoint")?;
    let adapter = WebSocketWorldAdapter::connect(
        world_url.to_string(),
        cfg.agent.id.clone(),
        manifest,
        cadence,
    );
    let trace = Arc::new(observability::TraceStore::new(cfg.node.observability));
    let observer_trace = trace.clone();
    let observer = Arc::new(move |id: &str, stage: &str, data: serde_json::Value| observer_trace.record(id,stage,data));
    adapter.set_observer(observer.clone());
    let ack_adapter=adapter.clone();
    let event_ack=Arc::new(move |session:&str, ids:&[String], decision:&str|ack_adapter.acknowledge_events(session,ids,decision));
    let brain = Arc::new(perceptive_brain::PerceptiveBrain::new(
        SingleModelBrain::new(
            provider.clone(),
            &cfg.inference.model,
            cfg.inference.context_tokens,
        ).with_observer(observer),
        format!("{}\n\n{}", cfg.agent.charter, cfg.world.instructions),
        cfg.world.allowed_actions,
        cadence,
        Duration::from_secs(cfg.inference.timeout_secs),
        trace.clone(),
        context_budget::ContextBudget {context:cfg.inference.context_tokens,completion:cfg.inference.completion_tokens,margin:cfg.inference.context_margin},
    ).with_event_acknowledger(event_ack).with_idle_interval(Duration::from_millis(cfg.agent.idle_interval_ms)));
    let agent = Agent::new(
        provider,
        EmptyToolHost,
        AgentConfig {
            agent_id: cfg.agent.id.clone(),
            model: cfg.inference.model.clone(),
            ..Default::default()
        },
    )
    .with_brain(brain);
    let health_adapter = adapter.clone();
    let agent_id = cfg.agent.id.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let connected = health_adapter.is_connected();
            let id = agent_id.clone();
            let trace = trace.clone();
            tokio::spawn(async move {
                let mut request = [0u8; 1024];
                if tokio::time::timeout(Duration::from_secs(2), socket.read(&mut request))
                    .await
                    .is_err()
                {
                    return;
                }
                let line=String::from_utf8_lossy(&request);
                let target=line.split_whitespace().nth(1).unwrap_or("/");
                let (status, body) = if target.starts_with("/debug/trace") {
                    let after=target.split("after=").nth(1).and_then(|s|s.split('&').next()).and_then(|s|s.parse::<u64>().ok()).unwrap_or(0);
                    if trace.enabled { ("200 OK", serde_json::json!({"agent_id":id,"trace":trace.since(after)}).to_string()) }
                    else { ("404 Not Found", "{\"error\":\"observability disabled\"}".into()) }
                } else { ("200 OK",serde_json::json!({"service":"tetonic-server","agent_id":id,"world_connected":connected,"mode":"standalone","decision":trace.health()}).to_string()) };
                let response = format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",status,body.len(),body);
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    tracing::info!(agent=%cfg.agent.id, model=%cfg.inference.model, bind=%cfg.node.bind, "tetonic-server started");
    tokio::select! {
        result = agent.run_in_world(adapter) => result.context("agent world loop")?,
        _ = tokio::signal::ctrl_c() => tracing::info!("shutdown requested"),
    }
    Ok(())
}
