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
        let response = c.guard.post_mcp(&c.config.endpoint, &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"tetonic","version":"0.1"}}}), None, VERSION, credential.as_ref()).await.map_err(connection_error)?;
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
            .map_err(connection_error)
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
                if self.connection.config.read_tools.iter().any(|n| n == name)
                    || (self.connection.stored.is_some()
                        && manifest.pointer("/annotations/readOnlyHint")
                            == Some(&Value::Bool(true)))
                {
                    if manifest.pointer("/annotations/readOnlyHint") != Some(&Value::Bool(true)) {
                        return Err("An operator-approved read tool does not advertise read-only behavior. Check the server configuration.".into());
                    }
                    if manifest
                        .pointer("/execution/taskSupport")
                        .and_then(Value::as_str)
                        == Some("required")
                    {
                        return Err("MCP background tasks are not supported".into());
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
        Err(e) => ToolOutcome::fail(e, "mcp_error"),
        Ok(value) => {
            if value.get("isError") == Some(&Value::Bool(true)) {
                return ToolOutcome::fail(
                    "The MCP tool reported an error. Check the tool arguments and service access.",
                    "mcp_tool_error",
                );
            }
            let Some(content) = value.get("content").and_then(Value::as_array) else {
                return ToolOutcome::fail("MCP result has no content", "unsupported_result");
            };
            if content.iter().any(|v| {
                v.get("type").and_then(Value::as_str) != Some("text")
                    || v.get("text").and_then(Value::as_str).is_none()
            }) {
                return ToolOutcome::fail(
                    "This MCP profile supports text and structured results only",
                    "unsupported_result",
                );
            }
            let mut text = content
                .iter()
                .filter_map(|v| v.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(structured) = value.get("structuredContent") {
                text.push_str(&format!("\n{}", structured));
            }
            if text.len() > 32_768 {
                return ToolOutcome::fail(
                    "MCP result exceeds 32 KiB; narrow the request",
                    "result_too_large",
                );
            }
            ToolOutcome::ok(format!("{} returned a result", tool.name), text)
        }
    }
}

async fn canceled(cancel: &CancellationSignal) {
    while !cancel.is_canceled() {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

fn connection_error(error: tetonic_egress::EgressError) -> String {
    match error {
        tetonic_egress::EgressError::StreamDecode(ref message) if message == "MCP HTTP 401" => "Service rejected authentication. Update its token; browser OAuth is not supported in this profile.".into(),
        tetonic_egress::EgressError::StreamDecode(ref message) if message == "MCP HTTP 403" => "Service denied access. Check the token permissions.".into(),
        _ => "Could not reach this MCP service, or it returned an unsupported response. Check its endpoint and access.".into(),
    }
}
