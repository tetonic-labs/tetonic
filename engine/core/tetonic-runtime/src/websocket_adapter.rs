//! TWP WebSocket transport. Actions complete only after an authoritative receipt.
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tetonic_domain::{
    ActionResult, EstopSwitch, Perception, PerceptionReceiver, PerceptionSender, Signal,
    SignalValue, Urgency, WorldAction, WorldAdapter, WorldError, WorldManifest,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

struct Request {
    id: String,
    action: WorldAction,
    deadline: Instant,
    reply: oneshot::Sender<Result<ActionResult, WorldError>>,
}

pub struct WebSocketWorldAdapter {
    manifest: WorldManifest,
    observer: std::sync::Mutex<Option<crate::brain::InferenceObserver>>,
    estop: Arc<EstopSwitch>,
    local_estop: Arc<EstopSwitch>,
    connected: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    tx: mpsc::Sender<Request>,
    acknowledgements: mpsc::Sender<Value>,
    perceptions: PerceptionSender,
    receiver: Mutex<Option<PerceptionReceiver>>,
}

impl WebSocketWorldAdapter {
    pub fn connect(
        url: String,
        agent_id: String,
        manifest: WorldManifest,
        cadence: Duration,
    ) -> Arc<Self> {
        let (tx, mut rx) = mpsc::channel::<Request>(8);
        let (ptx, prx) = mpsc::channel(32);
        let (ack_tx, mut ack_rx) = mpsc::channel::<Value>(16);
        let adapter = Arc::new(Self {
            manifest,
            observer: std::sync::Mutex::new(None),
            estop: Arc::new(EstopSwitch::new()),
            local_estop: Arc::new(EstopSwitch::new()),
            connected: Arc::new(AtomicBool::new(false)),
            epoch: Arc::new(AtomicU64::new(0)),
            tx,
            acknowledgements: ack_tx,
            perceptions: ptx.clone(),
            receiver: Mutex::new(Some(prx)),
        });
        let connected = adapter.connected.clone();
        let estop = adapter.estop.clone();
        let epoch = adapter.epoch.clone();
        let local_estop = adapter.local_estop.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_millis(500);
            loop {
                if rx.is_closed() {
                    break;
                }
                let connection =
                    tokio::time::timeout(Duration::from_secs(5), connect_async(&url)).await;
                if let Ok(Ok((mut ws, _))) = connection {
                    epoch.fetch_add(1, Ordering::SeqCst);
                    connected.store(true, Ordering::SeqCst);
                    tracing::info!(%agent_id, "world connected");
                    backoff = Duration::from_millis(500);
                    let mut pending: Option<Request> = None;
                    let mut last_result = Value::Null;
                    let started = Instant::now();
                    let mut expiry = tokio::time::interval(Duration::from_millis(100));
                    loop {
                        tokio::select! {
                            ack = ack_rx.recv() => {
                                if let Some(data)=ack { if ws.send(Message::Text(json!({"type":"events_ack","data":data}).to_string().into())).await.is_err(){break;} }
                            }
                            _ = expiry.tick() => {
                                if pending.as_ref().is_some_and(|r| r.deadline <= Instant::now() || r.reply.is_closed()) {
                                    if let Some(r) = pending.take() { let _ = r.reply.send(Err(WorldError::Timeout { elapsed_ms: 5000 })); }
                                }
                            }
                            request = rx.recv(), if pending.is_none() => {
                                let Some(request) = request else { let _ = ws.close(None).await; return; };
                                if request.reply.is_closed() || request.deadline <= Instant::now() { continue; }
                                if request.action.payload["_world_epoch"].as_u64() != Some(epoch.load(Ordering::SeqCst)) { let _ = request.reply.send(Err(WorldError::ActionRejected { kind: request.action.kind, reason: "stale world connection or E-Stop epoch".into() })); continue; }
                                if let Err(e) = estop.check(&request.action.kind) { let _ = request.reply.send(Err(e)); continue; }
                                if let Err(e) = local_estop.check(&request.action.kind) { let _ = request.reply.send(Err(e)); continue; }
                                let mut data = serde_json::to_value(&request.action).expect("serializable action");
                                data["agent_id"] = json!(agent_id);
                                data["action_id"] = json!(request.id);
                                let packet = json!({"type":"action", "data":data});
                                pending = Some(request);
                                if ws.send(Message::Text(packet.to_string().into())).await.is_err() { break; }
                            }
                            frame = ws.next() => {
                                let Some(Ok(frame)) = frame else { break; };
                                if frame.is_close() { break; }
                                if let Message::Ping(bytes) = frame { if ws.send(Message::Pong(bytes)).await.is_err() { break; } continue; }
                                let Message::Text(text) = frame else { continue; };
                                let Ok(packet) = serde_json::from_str::<Value>(&text) else { tracing::warn!("invalid world JSON"); continue; };
                                match packet["type"].as_str() {
                                    Some("perception") => {
                                        match serde_json::from_value::<Perception>(packet["data"].clone()) {
                                            Ok(mut p) => {
                                                if p.state.data["is_estopped"].as_bool() == Some(true) { if !estop.is_estopped() { epoch.fetch_add(1, Ordering::SeqCst); } estop.trigger("World E-Stop"); } else { estop.resume(); }
                                                if estop.is_estopped() || local_estop.is_estopped() { continue; }
                                                p.state.data["last_action_result"] = last_result.clone();
                                                p.state.data["_world_epoch"] = json!(epoch.load(Ordering::SeqCst));
                                                // A bounded wakeup lets goal-driven agents act in a quiet world.
                                                p.signals.push(Signal { name: "decision_window".into(), value: SignalValue::Int((started.elapsed().as_millis() / cadence.as_millis().max(1)) as i64), changed: false, trend: None, urgency: Urgency::Low });
                                                let _ = ptx.try_send(p);
                                            }
                                            Err(e) => tracing::warn!(error=%e, "invalid perception"),
                                        }
                                    }
                                    Some("action_result") => {
                                        if pending.as_ref().is_some_and(|r| packet["data"]["action_id"].as_str() == Some(r.id.as_str())) {
                                            let r = pending.take().unwrap();
                                            let result = serde_json::from_value::<ActionResult>(packet["data"].clone()).map_err(|e| WorldError::Adapter { detail:e.to_string() });
                                            last_result = json!({"action_id":r.id,"kind":r.action.kind,"result":packet["data"]});
                                            tracing::info!(receipt=%last_result, "world action receipt");
                                            let _ = r.reply.send(result);
                                        }
                                    }
                                    Some("estop") => { epoch.fetch_add(1, Ordering::SeqCst); estop.trigger("World E-Stop"); },
                                    Some("resume") => estop.resume(),
                                    _ => {}
                                }
                            }
                        }
                    }
                    connected.store(false, Ordering::SeqCst);
                    if let Some(r) = pending {
                        let _ = r.reply.send(Err(WorldError::NotConnected));
                    }
                    tracing::warn!(%agent_id, "world disconnected; no actions replayed");
                }
                connected.store(false, Ordering::SeqCst);
                while let Ok(r) = rx.try_recv() {
                    let _ = r.reply.send(Err(WorldError::NotConnected));
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
            }
        });
        adapter
    }
    pub fn set_observer(&self, observer: crate::brain::InferenceObserver) { *self.observer.lock().unwrap() = Some(observer); }
    /// Acknowledges decision inclusion only. A full queue leaves events pending at the world.
    pub fn acknowledge_events(&self, session:&str, ids:&[String], decision_id:&str) {
        if !ids.is_empty() { let _=self.acknowledgements.try_send(json!({"world_session":session,"event_ids":ids,"decision_id":decision_id})); }
    }
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WorldAdapter for WebSocketWorldAdapter {
    fn open(&self) -> (PerceptionSender, PerceptionReceiver) {
        (
            self.perceptions.clone(),
            self.receiver
                .try_lock()
                .expect("one reader")
                .take()
                .expect("open once"),
        )
    }
    async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError> {
        let observer = self.observer.lock().unwrap().clone();
        let trace_id = action.payload["_decision_trace"].as_str().unwrap_or("unattributed").to_owned();
        if let Some(o)=&observer { o(&trace_id,"action_submit",json!({"action":action})); }
        let outcome = async {
        if !self.is_connected() {
            return Err(WorldError::NotConnected);
        }
        self.estop.check(&action.kind)?;
        self.local_estop.check(&action.kind)?;
        self.manifest.validate_action(&action)?;
        let (reply, result) = oneshot::channel();
        self.tx
            .send(Request {
                id: uuid::Uuid::new_v4().to_string(),
                action,
                deadline: Instant::now() + Duration::from_secs(5),
                reply,
            })
            .await
            .map_err(|_| WorldError::NotConnected)?;
        tokio::time::timeout(Duration::from_secs(5), result)
            .await
            .map_err(|_| WorldError::Timeout { elapsed_ms: 5000 })?
            .map_err(|_| WorldError::NotConnected)?
        }.await;
        if let Some(o)=&observer {
            match &outcome {
                Ok(result)=>o(&trace_id,"action_result",json!(result)),
                Err(error)=>o(&trace_id,"action_error",json!({"error":error.to_string()})),
            }
        }
        outcome
    }

    fn describe(&self) -> &str {
        "twp:websocket"
    }
    fn manifest(&self) -> WorldManifest {
        self.manifest.clone()
    }
    fn trigger_estop(&self, reason: String) -> Result<(), WorldError> {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.local_estop.trigger(reason);
        Ok(())
    }
    fn resume(&self) -> Result<(), WorldError> {
        self.local_estop.resume();
        Ok(())
    }
    fn is_estopped(&self) -> bool {
        self.estop.is_estopped() || self.local_estop.is_estopped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::BrainPathway;

    #[tokio::test]
    async fn event_acknowledgements_use_the_shared_wire_fixture_without_a_world_action(){
        let fixture:Value=serde_json::from_str(include_str!("../tests/fixtures/event-delivery-v1.json")).unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let addr=listener.local_addr().unwrap();
        let wire=fixture.clone();
        let server=tokio::spawn(async move {
            let (socket,_)=listener.accept().await.unwrap();let mut ws=tokio_tungstenite::accept_async(socket).await.unwrap();
            ws.send(Message::Text(json!({"type":"perception","data":wire["perception"]}).to_string().into())).await.unwrap();
            let frame=ws.next().await.unwrap().unwrap();let packet:Value=serde_json::from_str(frame.to_text().unwrap()).unwrap();assert_eq!(packet,wire["ack"]);
        });
        let adapter=WebSocketWorldAdapter::connect(format!("ws://{addr}"),"a".into(),WorldManifest::new("test","1"),Duration::from_secs(6));
        let (_,mut rx)=adapter.open();let p=tokio::time::timeout(Duration::from_secs(3),rx.recv()).await.unwrap().unwrap();
        assert_eq!(p.events.len(),1);assert_eq!(p.state.data["delivery"]["protocol"],1);
        adapter.acknowledge_events("session-1",&["session-1:1".into()],"perception-7");
        tokio::time::timeout(Duration::from_secs(3),server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn waits_for_matching_world_receipt_and_fences_old_decisions() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (receipt_tx, receipt_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let perception = json!({"type":"perception","data":{"when":chrono::Utc::now(),"sequence":1,"urgency":"low","signals":[],"events":[],"state":{"schema_id":"test","data":{"is_estopped":false}}}});
            ws.send(Message::Text(perception.to_string().into()))
                .await
                .unwrap();
            let packet: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(packet["data"]["kind"], "step_move");
            assert_eq!(packet["data"]["payload"]["dx"], 1);
            assert_eq!(packet["data"]["agent_id"], "agent-1");
            // A wrong receipt cannot complete the action.
            ws.send(Message::Text(json!({"type":"action_result","data":{"action_id":"wrong","success":true,"feedback":"wrong receipt","state_changed":true}}).to_string().into())).await.unwrap();
            receipt_rx.await.unwrap();
            ws.send(Message::Text(json!({"type":"action_result","data":{"action_id":packet["data"]["action_id"],"success":false,"feedback":"wall","state_changed":false}}).to_string().into())).await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let adapter = WebSocketWorldAdapter::connect(
            format!("ws://{addr}"),
            "agent-1".into(),
            WorldManifest::new("test", "1"),
            Duration::from_secs(6),
        );
        let (_, mut perceptions) = adapter.open();
        let p = tokio::time::timeout(Duration::from_secs(3), perceptions.recv())
            .await
            .unwrap()
            .unwrap();
        let action = WorldAction::with_payload(
            "step_move",
            json!({"dx":1,"dy":0,"_world_epoch":p.state.data["_world_epoch"]}),
            BrainPathway::Single {
                model: "test".into(),
            },
        );
        let exec_adapter = adapter.clone();
        let action_clone = action.clone();
        let mut execution = tokio::spawn(async move { exec_adapter.execute(action_clone).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut execution)
                .await
                .is_err()
        );
        receipt_tx.send(()).unwrap();
        let result = execution.await.unwrap().unwrap();
        assert!(!result.success);
        assert_eq!(result.feedback.as_deref(), Some("wall"));
        adapter.trigger_estop("test".into()).unwrap();
        adapter.resume().unwrap();
        assert!(matches!(
            adapter.execute(action).await,
            Err(WorldError::ActionRejected { .. })
        ));
        server.abort();
    }
}
