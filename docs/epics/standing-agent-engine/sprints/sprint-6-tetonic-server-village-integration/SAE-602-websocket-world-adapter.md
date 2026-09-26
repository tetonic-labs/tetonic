# SAE-602 — WebSocket World Adapter

| Field        | Value                                              |
|--------------|----------------------------------------------------|
| **Ticket**   | SAE-602                                            |
| **Sprint**   | Sprint 6 — Tetonic Server + Village Integration    |
| **Epic**     | Standing Agent Engine                              |
| **Type**     | Feature                                            |
| **Priority** | P0 — Critical Path                                 |
| **Estimate** | 5 pts                                              |
| **Depends**  | SAE-203 (StreamWorldAdapter), SAE-601              |

---

## Objective

Add a WebSocket transport variant to `tetonic-runtime` so agents can connect to worlds that expose WebSocket endpoints (like The Village's `TetonicGateway`).

The existing `StreamWorldAdapter` speaks NDJSON over raw TCP or in-process duplex streams. The Village gateway speaks **WebSocket** with the same JSON envelope shape (`{ "type": "perception", "data": {...} }`). We need a `connect_ws()` constructor (or a new `WebSocketWorldAdapter`) that bridges this gap.

## Background — Protocol Alignment

The `StreamMessage` enum in Rust:
```rust
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum StreamMessage {
    Perception(Perception),
    Action(WorldAction),
    ActionResult(ActionResult),
    Estop { reason: String },
    Resume,
    Heartbeat,
}
```

The Village TS gateway sends/receives:
```typescript
{ type: "perception", data: { ... } }
{ type: "action",     data: { ... } }
```

**The JSON shape already matches.** The only difference is transport (WebSocket frames vs NDJSON lines).

## Acceptance Criteria

- [ ] Add `tokio-tungstenite` dependency to `tetonic-runtime/Cargo.toml` (workspace dep or crate-level).
- [ ] New constructor on `StreamWorldAdapter`: `connect_ws(url: &str, manifest: WorldManifest) -> Arc<Self>`
  - Establishes a WebSocket connection to the given URL.
  - Each inbound WebSocket text frame is deserialized as a `StreamMessage`.
  - Each outbound `StreamMessage` is serialized as a WebSocket text frame.
  - Reconnects with exponential backoff on disconnect (same pattern as `run_tcp_client`).
- [ ] Alternatively, if the adapter abstraction doesn't fit cleanly, create a standalone `WebSocketWorldAdapter` in a new file `ws_adapter.rs` that implements `WorldAdapter` directly.
- [ ] Unit test: round-trip a `StreamMessage::Perception` and `StreamMessage::Action` through a local WebSocket mock (use `tokio-tungstenite` server side in test).
- [ ] Integration test: verify connect/reconnect behavior with a mock WS server that drops connections.

## Implementation Notes

### Option A: Extend `StreamWorldAdapter` with `connect_ws()`

Add a `run_ws_client` method to `StreamSession` that mirrors `run_tcp_client` but uses WebSocket frames instead of NDJSON lines:

```rust
impl StreamSession {
    pub async fn run_ws_client(&mut self, url: &str) {
        let mut backoff = Duration::from_millis(50);
        let max_backoff = Duration::from_secs(5);

        loop {
            match tokio_tungstenite::connect_async(url).await {
                Ok((ws_stream, _)) => {
                    info!(url, "connected to world via websocket");
                    backoff = Duration::from_millis(50);
                    let _ = self.run_ws_io(ws_stream).await;
                    warn!(url, "websocket disconnected; reconnecting");
                }
                Err(e) => {
                    debug!(url, error = %e, "ws connect failed, backing off");
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    }
}
```

### Option B: Standalone `WebSocketWorldAdapter`

If the `StreamSession` I/O loop doesn't map cleanly to WebSocket semantics (frames vs. byte streams), create a separate adapter that owns the WebSocket connection directly.

### Preference

Option A is preferred — the `StreamMessage` enum, `EstopSwitch`, perception channel, and action dispatch are all reusable. Only the I/O framing layer changes.

## Workspace Dependency Addition

Add to `engine/Cargo.toml` workspace dependencies:
```toml
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
```

Or use `rustls` features to match the existing `reqwest` rustls-tls usage:
```toml
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-native-roots"] }
```

## Out of Scope

- Village gateway protocol changes (it already speaks the right format)
- Authentication / TLS for the WebSocket (localhost only for now)
- Multiple simultaneous world connections (future: `CompositeWorldAdapter`)
