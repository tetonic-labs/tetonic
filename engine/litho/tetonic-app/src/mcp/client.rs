use super::*;
pub(super) const VERSION: &str = "2025-11-25";
struct Session<'a> {
    connection: &'a Connection,
    id: Option<String>,
    version: String,
}
impl<'a> Session<'a> {
    async fn close(&self) {
        if let Some(id) = &self.id {
            let Ok(credential) = self.connection.credential().await else {
                return;
            };
            let _ = self
                .connection
                .guard
                .end_mcp(
                    &self.connection.config.endpoint,
                    id,
                    &self.version,
                    credential.as_ref(),
                )
                .await;
        }
    }
    async fn open(c: &'a Connection) -> Result<Self, String> {
        let credential = c.credential().await?;
        let response = c.guard.post_mcp(&c.config.endpoint, &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"tetonic","version":"0.1"}}}), None, VERSION, credential.as_ref()).await.map_err(|error| connection_error(error, c, credential.is_some()))?;
        let result = result(
            response
                .message
                .ok_or("MCP initialization returned no response")?,
        )?;
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or("Missing MCP protocol version")?;
        if ![VERSION, "2025-06-18", "2025-03-26"].contains(&version)
            || result
                .get("capabilities")
                .is_none_or(|v| v.get("tools").is_none())
        {
            return Err("Unsupported MCP protocol or missing tools capability".into());
        }
        let session = Self {
            connection: c,
            id: response.session,
            version: version.into(),
        };
        session
            .post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        Ok(session)
    }
    async fn post(&self, body: Value) -> Result<Option<Value>, String> {
        let credential = self.connection.credential().await?;
        self.connection
            .guard
            .post_mcp(
                &self.connection.config.endpoint,
                &body,
                self.id.as_deref(),
                &self.version,
                credential.as_ref(),
            )
            .await
            .map(|r| r.message)
            .map_err(|error| connection_error(error, self.connection, credential.is_some()))
    }
    async fn tools(&self) -> Result<Vec<McpTool>, String> {
        let mut cursor: Option<String> = None;
        let mut cursors = HashSet::new();
        let mut names = HashSet::new();
        let mut tools = Vec::new();
        for page in 0..4 {
            let params = cursor
                .as_ref()
                .map(|c| json!({"cursor":c}))
                .unwrap_or_else(|| json!({}));
            let value = result(
                self.post(
                    json!({"jsonrpc":"2.0","id":2+page,"method":"tools/list","params":params}),
                )
                .await?
                .ok_or("MCP list returned no response")?,
            )?;
            let list = value
                .get("tools")
                .and_then(Value::as_array)
                .ok_or("MCP list is invalid")?;
            for manifest in list {
                let name = manifest
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("MCP tool has no name")?;
                if !names.insert(name.to_string()) || names.len() > 128 {
                    return Err("MCP list has duplicate tools or exceeds 128 tools".into());
                }
                let read_grant = self.connection.config.read_tools.iter().any(|n| n == name);
                if read_grant
                    || self
                        .connection
                        .config
                        .action_tools
                        .iter()
                        .any(|n| n == name)
                    || self.connection.stored.is_some()
                {
                    if read_grant
                        && manifest.pointer("/annotations/readOnlyHint") != Some(&Value::Bool(true))
                    {
                        return Err("An operator-approved read tool does not advertise read-only behavior. Check the server configuration.".into());
                    }
                    if manifest
                        .pointer("/execution/taskSupport")
                        .and_then(Value::as_str)
                        == Some("required")
                    {
                        // One task-only tool must not hide usable foreground tools.
                        continue;
                    }
                    tools.push(pinned_tool(self.connection, manifest.clone())?);
                }
            }
            match value.get("nextCursor") {
                None | Some(Value::Null) => {
                    tools.sort_by(|a, b| a.id.cmp(&b.id));
                    return Ok(tools);
                }
                Some(Value::String(next))
                    if !next.is_empty() && next.len() <= 1024 && cursors.insert(next.clone()) =>
                {
                    cursor = Some(next.clone())
                }
                _ => return Err("Invalid MCP pagination cursor".into()),
            }
        }
        Err("MCP tool discovery exceeds four pages".into())
    }
}
fn result(message: Value) -> Result<Value, String> {
    // Error bodies may contain server secrets. Keep them out of logs and user/model text.
    message
        .get("result")
        .cloned()
        .ok_or_else(|| "The MCP server rejected this request".into())
}
pub(super) async fn discover(c: &Connection) -> Result<Vec<McpTool>, String> {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let session = Session::open(c).await?;
        let result = session.tools().await;
        session.close().await;
        result
    })
    .await
    .map_err(|_| "MCP discovery timed out")?
}
pub(super) async fn call(
    c: &Connection,
    tool: McpTool,
    args: &Value,
    cancel: &CancellationSignal,
) -> ToolOutcome {
    if cancel.is_canceled() {
        return ToolOutcome::fail("MCP setup canceled", "canceled");
    }
    let opened = tokio::select! {
        session = Session::open(c) => session,
        _ = canceled(cancel) => return ToolOutcome::fail("MCP setup canceled", "canceled"),
        _ = c.revoked() => return ToolOutcome::fail("MCP connection disconnected or changed", "denied"),
    };
    let session = match opened {
        Ok(s) => s,
        Err(e) => return ToolOutcome::fail(e, "unavailable"),
    };
    let dispatched = std::sync::atomic::AtomicBool::new(false);
    let operation = async {
        let current = session.tools().await?;
        if !current
            .iter()
            .any(|t| t.id == tool.id && t.manifest == tool.manifest)
        {
            return Err("MCP tool changed or was removed. Refresh discovery and review the tool and update the agent selection.".into());
        }
        if cancel.is_canceled() || !c.allowed(&tool) {
            return Err("MCP call canceled or access removed before dispatch".into());
        }
        dispatched.store(true, std::sync::atomic::Ordering::SeqCst);
        result(session.post(json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":tool.name,"arguments":args}})).await?.ok_or("MCP call returned no response")?)
    };
    tokio::pin!(operation);
    let response = tokio::select! {
        value = tokio::time::timeout(std::time::Duration::from_secs(20), &mut operation) => value.unwrap_or_else(|_| Err("MCP operation timed out; no automatic retry was sent".into())),
        _ = c.tool_revoked(&tool) => Err("MCP access revoked; waiting stopped. Remote completion is not confirmed.".into()),
        _ = canceled(cancel) => {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), session.post(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10,"reason":"Tetonic attempt stopped"}}))).await;
            Err("MCP call canceled; cancellation was requested from the server, not confirmed".into())
        }
    };
    session.close().await;
    match response {
        Err(e) if dispatched.load(std::sync::atomic::Ordering::SeqCst) && !tool.read_only => ToolOutcome::fail(
            format!("{e} Remote action outcome is unknown; it may have completed. Inspect the service state before deciding whether to retry. No automatic retry was sent."),
            "mcp_outcome_unknown",
        ),
        Err(e) => ToolOutcome::fail(e, "mcp_error"),
        Ok(value) => super::results::tool_result(&tool.name, &value),
    }
}

async fn canceled(cancel: &CancellationSignal) {
    while !cancel.is_canceled() {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

fn connection_error(error: tetonic_egress::EgressError, c: &Connection, has_token: bool) -> String {
    use tetonic_egress::EgressError;
    // Use only our transport's classification, never a remote body, URL, or credential.
    // A 401 alone cannot establish that this address belongs to an MCP service.
    match error {
        EgressError::StreamDecode(ref message) => match message.as_str() {
            "MCP HTTP 401" if !has_token => "This address requires sign-in. Check that it is the service’s MCP address. If the service gave you a token, add it in connection settings.",
            "MCP HTTP 401" => "This address did not accept the saved token. Check the MCP address and replace the token in connection settings if needed.",
            "MCP HTTP 403" => "Access was denied. Check the service address and whether your account or token can use it.",
            "MCP HTTP 404" | "MCP HTTP 405" | "unsupported MCP response type" | "invalid MCP response" | "MCP response does not match request" => "This address did not respond as an MCP service. Copy the MCP address from the service’s setup instructions; its website or app address may be different.",
            "MCP HTTP 301" | "MCP HTTP 302" | "MCP HTTP 303" | "MCP HTTP 307" | "MCP HTTP 308" => "This address redirects elsewhere. Use the direct MCP address provided by the service.",
            "MCP HTTP 429" => "The service is receiving too many requests. Wait a moment, then check the connection again.",
            "MCP HTTP 500" | "MCP HTTP 502" | "MCP HTTP 503" | "MCP HTTP 504" => "The service is having trouble responding. Try checking the connection again later.",
            _ => "The service returned an unsupported MCP response. Check its MCP address and supported connection options.",
        },
        EgressError::Http(ref error) if error.is_timeout() => "The service took too long to respond. Check that it is running, then try again.",
        EgressError::Http(ref error) if error.is_connect() && c.config.endpoint.starts_with("http:") => "Could not connect to the service on this computer. Open or start the service, then check that the port in its MCP address matches this connection.",
        EgressError::Http(ref error) if error.is_connect() => "Could not connect to the service. Check the address, your internet connection, and the service’s availability.",
        EgressError::Resolve(_) => "Could not find the service’s address. Check the address and your internet connection.",
        _ => "Could not connect to this MCP service. Check its address and connection settings.",
    }.into()
}
