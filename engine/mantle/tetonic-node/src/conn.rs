//! Shared fabric connection handler (production + tests).

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tracing::warn;

use crate::event::{IngressEvent, IngressLog};
use crate::fabric::{self, FabricState};

/// Serve one mTLS fabric HTTP/1 connection until close or error.
pub async fn serve_fabric_connection<S>(
    io: S,
    remote: SocketAddr,
    state: Arc<FabricState>,
    log: IngressLog,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req| {
        let state = state.clone();
        let log = log.clone();
        let remote = remote;
        async move {
            let outcome = fabric::handle_with_audit(state, req).await;
            log.record(IngressEvent::route_outcome(
                remote,
                outcome.peer_id,
                outcome.route,
                outcome.decision,
                outcome.reason,
            ));
            Ok::<_, Infallible>(outcome.response)
        }
    });

    let io = TokioIo::new(io);
    if let Err(e) = hyper::server::conn::http1::Builder::new()
        .keep_alive(false)
        .timer(TokioTimer::new())
        .header_read_timeout(crate::limits::HEADER_TIMEOUT)
        .serve_connection(io, service)
        .await
    {
        warn!("fabric connection from {remote}: {e}");
    }
}
