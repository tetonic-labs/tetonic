mod bind;
mod conn;
mod event;
mod fabric;
mod fabric_chat;
mod job_ingress;
mod lease_table;
mod limits;
mod revoke;
mod scheduler;
mod server;
mod tls;
mod trust;

pub use bind::{resolve_listen_host, DEFAULT_BIND_HOST};
pub use event::{IngressDecision, IngressEvent, IngressLog};
pub use limits::{MAX_FABRIC_BODY_BYTES, MAX_FABRIC_CONNECTIONS};
pub use revoke::{push_revoke, RevokeError, RevokeRequest, RevokeResponse};
pub use scheduler::{CancelReason, WorkerScheduler};
pub use server::{FabricListenConfig, FabricServer};
pub use tetonic_enroll::DEFAULT_FABRIC_PORT;
pub use tls::{build_client_config, client_cert_from_keypair, issue_self_signed_cert};
pub use trust::{TrustError, TrustStore};

mod tls_identity;
pub use tls_identity::{
    load_or_create_tls_identity, revoke_tls_identity, rotate_tls_identity, TlsIdentity,
};

pub mod role;
pub use role::{
    FailoverEvent, KeeperError, KeeperRegistry, NodeCapabilities, NodeLifecycle, NodeRole,
    RunnerClient, RunnerRegistration, RunnerStatus,
};
