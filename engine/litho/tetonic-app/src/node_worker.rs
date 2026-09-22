//! Worker node enrollment and serving runtime.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, Result};
use tetonic_enroll::{
    advertise_host, plaintext_enrollment_permitted, run_enrollment_server, EnrollmentCode,
    EnrollmentServerConfig, EnrollmentServerOutcome, KeyPair, DEFAULT_ENROLL_PORT,
    DEFAULT_ENROLL_TTL, DEFAULT_FABRIC_PORT,
};
use tetonic_memory::WorkerStore;
use tetonic_node::{
    load_or_create_tls_identity, resolve_listen_host, FabricListenConfig, FabricServer,
};
use tracing_subscriber::EnvFilter;

use crate::Application;

struct EnrollmentCodeFile(PathBuf);

impl Drop for EnrollmentCodeFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0) {
            tracing::warn!("could not remove expired enrollment code file: {error}");
        }
    }
}

pub fn data_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "lokai")
        .context("could not resolve lokai data directory")?;
    Ok(dirs.data_dir().to_path_buf())
}

impl Application {
    pub async fn run_node_enroll() -> Result<()> {
        let mode = if std::env::var("LOKAI_DIAGNOSTIC_RAW_PAYLOADS").is_ok() {
            tetonic_telemetry::DiagnosticMode::UnsafeRawPayloads
        } else {
            tetonic_telemetry::DiagnosticMode::Safe
        };

        let subscriber = tetonic_telemetry::init_subscriber(mode);
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set telemetry subscriber");

        let worker_keys = KeyPair::generate();
        let audit_keys = KeyPair::generate();
        let host = advertise_host();
        let enroll_port = std::env::var("LOKAI_ENROLL_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_ENROLL_PORT);
        let fabric_port = std::env::var("LOKAI_FABRIC_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_FABRIC_PORT);

        let worker_db = data_dir()?.join("worker.db");
        let worker_store = WorkerStore::open(&worker_db)?;
        let identity = load_or_create_tls_identity(
            &worker_store,
            &tetonic_secrets::key_storage::PlatformKeyStorage,
        )?;
        let fabric_tls_cert = identity.certificate;
        let fabric_tls_key = identity.private_key.as_ref().to_vec();

        let code = EnrollmentCode::new(
            &worker_keys,
            &audit_keys,
            &host,
            enroll_port,
            fabric_port,
            DEFAULT_ENROLL_TTL,
            Some(&fabric_tls_cert),
        );

        let code_path = data_dir()?.join(format!("enrollment-{}.code", uuid::Uuid::new_v4()));
        let display = code.display_string().context("encode enrollment code")?;
        tetonic_secrets::private_file::write_new(&code_path, format!("{display}\n").as_bytes())
            .context("write private enrollment code")?;
        let _code_file = EnrollmentCodeFile(code_path.clone());

        let (bind_host, bind_warn) = resolve_listen_host();
        if let Some(w) = bind_warn {
            tracing::warn!("{w}");
        }
        if let Err(reason) = plaintext_enrollment_permitted(&bind_host) {
            anyhow::bail!("{reason}");
        }

        tracing::info!("Worker enrollment (expires {}):", code.expires_at);
        tracing::info!("Code file: {}", code_path.display());
        tracing::info!("Enrollment listener: {bind_host}:{}", code.enroll_port);
        tracing::info!(
            "On the coordinator (PowerShell): .\\engine\\target\\debug\\lokai.exe estate worker add --code-file \"{}\" --label gpu-box",
            code_path.display()
        );
        if host == "127.0.0.1" {
            tracing::info!("Tip: set LOKAI_ADVERTISE_HOST to this machine's LAN IP so the coordinator can reach it.");
        }

        let cfg = EnrollmentServerConfig {
            code: code.clone(),
            ttl: DEFAULT_ENROLL_TTL,
            bind_host,
            fabric_tls_cert: Some(fabric_tls_cert),
            fabric_tls_key: Some(fabric_tls_key),
        };

        match run_enrollment_server(cfg).await {
            EnrollmentServerOutcome::Completed {
                coordinator_pubkey,
                label,
            } => {
                let worker_db = data_dir()?.join("worker.db");
                let store = WorkerStore::open(&worker_db)?;
                store.upsert_coordinator_pin("estate_local", &coordinator_pubkey, &label)?;
                tracing::info!("Enrollment succeeded for coordinator label `{label}`.");
                tracing::info!("Coordinator pinned; enrollment listener closed.");
                tracing::info!("Start fabric serving with: lokaid --node");
                Ok(())
            }
            EnrollmentServerOutcome::Expired => {
                anyhow::bail!("enrollment code expired without a successful handshake");
            }
            EnrollmentServerOutcome::Failed(msg) => {
                anyhow::bail!("enrollment failed: {msg}");
            }
        }
    }

    pub async fn run_node_serve() -> Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
            .with_target(false)
            .compact()
            .init();

        let worker_db = data_dir()?.join("worker.db");
        let store = WorkerStore::open(&worker_db)?;
        let pins = store.list_coordinator_pins()?;
        if pins.is_empty() {
            anyhow::bail!("no coordinator pinned — run `lokaid --node --enroll` first");
        }

        let (bind_host, bind_warn) = resolve_listen_host();
        if let Some(w) = bind_warn {
            tracing::warn!("{w}");
        }

        let port = std::env::var("LOKAI_FABRIC_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_FABRIC_PORT);
        let ollama =
            std::env::var("LOKAI_OLLAMA").unwrap_or_else(|_| "http://localhost:11434".into());

        tracing::info!("Fabric serve mode (mTLS, owner coordinators only)");
        tracing::info!("  bind: {bind_host}:{port}");
        tracing::info!("  coordinators pinned: {}", pins.len());

        let guard = std::sync::Arc::new(tetonic_egress::EgressGuard::pinned_to_inference_url(
            &ollama,
        ));
        let client = Arc::new(tetonic_capacity::OllamaInferenceClient::new(
            &ollama,
            Arc::new(tetonic_inference::OllamaProvider::new(&ollama, guard)),
        ));
        let status =
            tetonic_capacity::capacity_status_for_worker_path(worker_db.clone(), client, None)
                .await;
        tracing::info!(
            "  capacity: doctor={:?} gates_ok={} profile={:?}",
            status.0.doctor,
            status.0.gates_ok,
            status.0.active_profile_label
        );
        if status.0.doctor == tetonic_capacity::CapacityDoctorStatus::Degraded {
            tracing::warn!(
                "  capacity doctor degraded — run optimize or import a profile on this worker"
            );
        } else if !status.0.completed {
            tracing::warn!("  no capacity profile — import with `lokai estate capacity profiles import` on this machine");
        }

        tracing::info!("Press Ctrl+C to stop.");

        let cfg = FabricListenConfig {
            bind_host,
            port,
            ollama_base: ollama,
        };
        FabricServer::new()
            .run(&store, cfg, Arc::new(AtomicBool::new(false)))
            .await
    }

    pub fn spawn_combined_fabric() -> Result<Arc<AtomicBool>> {
        let fabric_server = std::sync::Arc::new(FabricServer::new());
        let fabric_shutdown = Arc::new(AtomicBool::new(false));
        let worker_db = data_dir()?.join("worker.db");
        let store = WorkerStore::open(&worker_db)?;
        let (bind_host, bind_warn) = resolve_listen_host();
        if let Some(w) = bind_warn {
            tracing::warn!("{w}");
        }
        let port = std::env::var("LOKAI_FABRIC_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_FABRIC_PORT);
        let ollama =
            std::env::var("LOKAI_OLLAMA").unwrap_or_else(|_| "http://localhost:11434".into());
        let cfg = FabricListenConfig {
            bind_host: bind_host.clone(),
            port,
            ollama_base: ollama,
        };
        let server = fabric_server.clone();
        let stop = fabric_shutdown.clone();
        tokio::task::spawn_local(async move {
            if let Err(e) = server.run(&store, cfg, stop).await {
                tracing::error!("combined fabric listener: {e}");
            }
        });
        tracing::info!("combined mode: coordinator stdio + fabric on {bind_host}:{port}");
        Ok(fabric_shutdown)
    }
}
