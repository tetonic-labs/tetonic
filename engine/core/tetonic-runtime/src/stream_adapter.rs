//! [`StreamWorldAdapter`] — bidirectional framed stream adapter for live external worlds.
//!
//! Enables agents to connect to external simulations, game loops, cloud control planes,
//! or robotics controllers over TCP or duplex streams using Newline-Delimited JSON (NDJSON) framing.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use tetonic_domain::{
    ActionResult, EstopSwitch, Perception, PerceptionReceiver, PerceptionSender,
    WorldAction, WorldAdapter, WorldError, WorldManifest,
};

/// Framing protocol message envelope for bidirectional stream communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum StreamMessage {
    /// Inbound perception tick from the world to the agent.
    Perception(Perception),
    /// Outbound action from the agent to the world.
    Action(WorldAction),
    /// Execution feedback from the world actuators.
    ActionResult(ActionResult),
    /// Emergency stop triggered remotely by the world.
    Estop { reason: String },
    /// Resume signal clearing a remote emergency stop.
    Resume,
    /// Keep-alive ping/pong to detect socket liveness.
    Heartbeat,
}

/// A concrete [`WorldAdapter`] that communicates over bidirectional streaming sockets.
pub struct StreamWorldAdapter {
    manifest: WorldManifest,
    estop: Arc<EstopSwitch>,
    description: String,
    action_tx: mpsc::Sender<WorldAction>,
    outbound_msg_tx: mpsc::Sender<StreamMessage>,
    perception_sender: PerceptionSender,
    perception_receiver: Mutex<Option<PerceptionReceiver>>,
    is_connected: Arc<AtomicBool>,
}

impl StreamWorldAdapter {
    /// Create a new stream adapter paired with an internal I/O dispatch session.
    pub fn new(
        manifest: WorldManifest,
        description: impl Into<String>,
    ) -> (Arc<Self>, StreamSession) {
        let (perception_tx, perception_rx) = mpsc::channel(128);
        let (action_tx, action_rx) = mpsc::channel(128);
        let (outbound_tx, outbound_rx) = mpsc::channel(128);
        let estop = Arc::new(EstopSwitch::new());
        let is_connected = Arc::new(AtomicBool::new(false));

        let adapter = Arc::new(Self {
            manifest,
            estop: estop.clone(),
            description: description.into(),
            action_tx,
            outbound_msg_tx: outbound_tx,
            perception_sender: perception_tx.clone(),
            perception_receiver: Mutex::new(Some(perception_rx)),
            is_connected: is_connected.clone(),
        });

        let session = StreamSession {
            perception_tx,
            action_rx,
            outbound_rx,
            estop,
            is_connected,
        };

        (adapter, session)
    }

    /// Connect directly to an in-process or mock duplex stream.
    pub fn from_duplex<S>(stream: S, manifest: WorldManifest) -> Arc<Self>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
    {
        let (adapter, mut session) = Self::new(manifest, "stream:duplex");
        tokio::spawn(async move {
            let (reader, writer) = tokio::io::split(stream);
            let _ = session.run_io(reader, writer).await;
        });
        adapter
    }

    /// Connect to an external TCP endpoint with automatic reconnection and backoff.
    pub fn connect_tcp(addr: SocketAddr, manifest: WorldManifest) -> Arc<Self> {
        let (adapter, mut session) = Self::new(manifest, format!("stream:tcp:{addr}"));
        tokio::spawn(async move {
            session.run_tcp_client(addr).await;
        });
        adapter
    }

    /// Check if the stream is currently connected to the remote world.
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WorldAdapter for StreamWorldAdapter {
    fn open(&self) -> (PerceptionSender, PerceptionReceiver) {
        let mut guard = self.perception_receiver.try_lock().expect("open called once");
        let rx = guard.take().expect("open called only once per adapter");
        (self.perception_sender.clone(), rx)
    }

    async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError> {
        // 1. Check E-Stop
        self.estop.check(&action.kind)?;

        // 2. Validate against advertised manifest
        self.manifest.validate_action(&action)?;

        // 3. Dispatch to stream writer
        self.action_tx
            .send(action)
            .await
            .map_err(|_| WorldError::NotConnected)?;

        Ok(ActionResult {
            success: true,
            feedback: Some("action queued for stream delivery".into()),
            state_changed: true,
        })
    }

    fn describe(&self) -> &str {
        &self.description
    }

    fn manifest(&self) -> WorldManifest {
        self.manifest.clone()
    }

    fn trigger_estop(&self, reason: String) -> Result<(), WorldError> {
        self.estop.trigger(reason.clone());
        let _ = self.outbound_msg_tx.try_send(StreamMessage::Estop { reason });
        Ok(())
    }

    fn resume(&self) -> Result<(), WorldError> {
        self.estop.resume();
        let _ = self.outbound_msg_tx.try_send(StreamMessage::Resume);
        Ok(())
    }

    fn is_estopped(&self) -> bool {
        self.estop.is_estopped()
    }
}

/// Internal session driver managing the read/write loops over an async stream.
pub struct StreamSession {
    perception_tx: PerceptionSender,
    action_rx: mpsc::Receiver<WorldAction>,
    outbound_rx: mpsc::Receiver<StreamMessage>,
    estop: Arc<EstopSwitch>,
    is_connected: Arc<AtomicBool>,
}

impl StreamSession {
    /// Run the persistent TCP connection loop with exponential backoff on disconnect.
    pub async fn run_tcp_client(&mut self, addr: SocketAddr) {
        let mut backoff = Duration::from_millis(50);
        let max_backoff = Duration::from_secs(5);

        loop {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(stream) => {
                    info!(target = %addr, "connected to external world stream");
                    backoff = Duration::from_millis(50);
                    let (reader, writer) = stream.into_split();
                    let _ = self.run_io(reader, writer).await;
                    warn!(target = %addr, "world stream disconnected; attempting reconnect");
                }
                Err(e) => {
                    debug!(target = %addr, error = %e, "connection attempt failed, backing off");
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    }

    /// Drive framed I/O over arbitrary async reader and writer halves.
    pub async fn run_io<R, W>(&mut self, reader: R, mut writer: W) -> Result<(), std::io::Error>
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        self.is_connected.store(true, Ordering::SeqCst);

        let perception_tx = self.perception_tx.clone();
        let estop = self.estop.clone();

        // Spawn read loop
        let read_handle = tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                match serde_json::from_str::<StreamMessage>(trimmed) {
                    Ok(StreamMessage::Perception(p)) => {
                        if perception_tx.send(p).await.is_err() {
                            break;
                        }
                    }
                    Ok(StreamMessage::Estop { reason }) => {
                        warn!(reason = %reason, "remote world engaged E-Stop");
                        estop.trigger(reason);
                    }
                    Ok(StreamMessage::Resume) => {
                        info!("remote world cleared E-Stop");
                        estop.resume();
                    }
                    Ok(StreamMessage::Heartbeat) => {
                        debug!("received heartbeat ping");
                    }
                    Ok(other) => {
                        debug!(msg = ?other, "ignoring unhandled stream message");
                    }
                    Err(e) => {
                        error!(error = %e, line = %trimmed, "failed to parse stream message");
                    }
                }
            }
        });

        // Write loop in this task
        loop {
            tokio::select! {
                Some(action) = self.action_rx.recv() => {
                    let msg = StreamMessage::Action(action);
                    if let Ok(mut encoded) = serde_json::to_string(&msg) {
                        encoded.push('\n');
                        if writer.write_all(encoded.as_bytes()).await.is_err() {
                            break;
                        }
                        if writer.flush().await.is_err() {
                            break;
                        }
                    }
                }
                Some(msg) = self.outbound_rx.recv() => {
                    if let Ok(mut encoded) = serde_json::to_string(&msg) {
                        encoded.push('\n');
                        if writer.write_all(encoded.as_bytes()).await.is_err() {
                            break;
                        }
                        if writer.flush().await.is_err() {
                            break;
                        }
                    }
                }
                else => break,
            }
        }

        self.is_connected.store(false, Ordering::SeqCst);
        let _ = read_handle.await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::{Affordance, BrainPathway, Urgency, WorldState};

    #[tokio::test]
    async fn test_stream_adapter_bidirectional_exchange() {
        let manifest = WorldManifest::new("sim_world", "1.0")
            .with_affordance(Affordance::instant("step_forward", "Advance 1 unit"));

        let (client_io, mut server_io) = tokio::io::duplex(4096);
        let adapter = StreamWorldAdapter::from_duplex(client_io, manifest);

        let (_tx, mut rx) = adapter.open();

        // 1. Server sends perception to agent
        let p = Perception {
            when: chrono::Utc::now(),
            sequence: 1,
            urgency: Urgency::Medium,
            signals: vec![],
            events: vec![],
            state: WorldState {
                schema_id: "sim".into(),
                data: serde_json::json!({ "tick": 100 }),
            },
        };
        let mut p_json = serde_json::to_string(&StreamMessage::Perception(p)).unwrap();
        p_json.push('\n');
        server_io.write_all(p_json.as_bytes()).await.unwrap();
        server_io.flush().await.unwrap();

        // 2. Adapter receives perception
        let received = rx.recv().await.expect("perception received");
        assert_eq!(received.sequence, 1);
        assert_eq!(received.urgency, Urgency::Medium);

        // 3. Adapter executes action -> sent to server over wire
        let action = WorldAction::bare(
            "step_forward",
            BrainPathway::Reflexive {
                model: "reflex".into(),
            },
        );
        adapter.execute(action).await.expect("action accepted");

        // 4. Server receives action
        let mut server_reader = BufReader::new(server_io);
        let mut line = String::new();
        server_reader.read_line(&mut line).await.unwrap();
        let parsed: StreamMessage = serde_json::from_str(line.trim()).unwrap();
        match parsed {
            StreamMessage::Action(a) => {
                assert_eq!(a.kind, "step_forward");
            }
            other => panic!("Expected StreamMessage::Action, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_stream_adapter_estop_interlock() {
        let manifest = WorldManifest::new("sim_world", "1.0")
            .with_affordance(Affordance::instant("step_forward", "Advance 1 unit"));

        let (client_io, mut server_io) = tokio::io::duplex(4096);
        let adapter = StreamWorldAdapter::from_duplex(client_io, manifest);

        assert!(!adapter.is_estopped());

        // Server sends remote E-Stop
        let estop_msg = StreamMessage::Estop {
            reason: "Simulated actuator stall".into(),
        };
        let mut estop_json = serde_json::to_string(&estop_msg).unwrap();
        estop_json.push('\n');
        server_io.write_all(estop_json.as_bytes()).await.unwrap();
        server_io.flush().await.unwrap();

        // Wait briefly for read loop to process
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(adapter.is_estopped());

        // Subsequent execute must fail with E-Stop
        let action = WorldAction::bare(
            "step_forward",
            BrainPathway::Reflexive {
                model: "reflex".into(),
            },
        );
        let err = adapter.execute(action).await.unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));

        // Server sends Resume
        let resume_msg = StreamMessage::Resume;
        let mut resume_json = serde_json::to_string(&resume_msg).unwrap();
        resume_json.push('\n');
        server_io.write_all(resume_json.as_bytes()).await.unwrap();
        server_io.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!adapter.is_estopped());
    }

    #[tokio::test]
    async fn test_stream_adapter_tcp_server_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let manifest = WorldManifest::new("tcp_world", "1.0")
            .with_affordance(Affordance::instant("ping", "Ping world"));

        let adapter = StreamWorldAdapter::connect_tcp(addr, manifest);

        // Server accepts connection
        let (server_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(adapter.is_connected());

        // Server writes a perception
        let (reader, mut writer) = server_stream.into_split();
        let p = Perception {
            when: chrono::Utc::now(),
            sequence: 42,
            urgency: Urgency::Low,
            signals: vec![],
            events: vec![],
            state: WorldState {
                schema_id: "tcp".into(),
                data: serde_json::Value::Null,
            },
        };
        let mut msg = serde_json::to_string(&StreamMessage::Perception(p)).unwrap();
        msg.push('\n');
        writer.write_all(msg.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        let (_tx, mut rx) = adapter.open();
        let received = rx.recv().await.unwrap();
        assert_eq!(received.sequence, 42);

        drop(reader);
        drop(writer);
    }
}
