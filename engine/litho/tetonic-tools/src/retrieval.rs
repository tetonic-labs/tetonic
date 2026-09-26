//! Code index and episodic memory retrieval tool implementations.

use serde_json::Value;
use tetonic_domain::CodeIndex;

use crate::exec::truncate_output as truncate;
use crate::types::{
    FindDefinitionArgs, FindMentionsArgs, OutlineArgs, RecallArgs, SearchCodeArgs, ToolError,
    ToolOutcome,
};
use crate::workspace::Workspace;

pub fn recall_context(
    store: &tetonic_memory::Store,
    actor: &str,
    context: &str,
    args: Value,
) -> Result<ToolOutcome, ToolError> {
    let args: RecallArgs = serde_json::from_value(args)
        .map_err(|_| ToolError::BadArgs("expected query and optional max_results".into()))?;
    let hits = store
        .recall_context_messages(
            actor,
            context,
            &args.query,
            args.max_results.unwrap_or(8).min(20) as u32,
        )
        .map_err(|_| ToolError::Other("context recall unavailable or access denied".into()))?;
    let text = hits
        .iter()
        .map(|h| {
            format!(
                "<untrusted recall>\n[{} {}] {}\n</untrusted recall>",
                h.session_id, h.label, h.snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutcome::ok(
        format!("{} authorized message hit(s)", hits.len()),
        truncate(&text),
    ))
}

pub fn find_definition(
    index: &dyn CodeIndex,
    ws_key: &str,
    args: Value,
) -> Result<ToolOutcome, ToolError> {
    let a: FindDefinitionArgs =
        serde_json::from_value(args).map_err(|e| ToolError::BadArgs(e.to_string()))?;
    let defs = index
        .find_definition_in(ws_key, &a.name, a.path.as_deref())
        .map_err(|e| ToolError::Other(e.to_string()))?;
    if defs.is_empty() {
        return Ok(ToolOutcome::ok(
            format!("no definition for '{}'", a.name),
            format!(
                "No indexed definition named '{}'. Try search_code or grep.",
                a.name
            ),
        ));
    }
    let body = defs
        .iter()
        .map(|d| {
            format!("{} {}:{}  {}", d.kind, d.rel, d.start_line, d.signature)
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutcome::ok(
        format!("{} definition(s) for '{}'", defs.len(), a.name),
        truncate(&body),
    ))
}

pub fn search_code(
    index: &dyn CodeIndex,
    ws_key: &str,
    args: Value,
) -> Result<ToolOutcome, ToolError> {
    let a: SearchCodeArgs =
        serde_json::from_value(args).map_err(|e| ToolError::BadArgs(e.to_string()))?;
    let limit = a.max_results.unwrap_or(10).min(50) as u32;
    let hits = index
        .search(ws_key, &a.query, limit)
        .map_err(|e| ToolError::Other(e.to_string()))?;
    if hits.is_empty() {
        return Ok(ToolOutcome::ok(
            format!("no matches for '{}'", a.query),
            "No indexed matches. Try different terms, or grep for a regex.".to_string(),
        ));
    }
    let body = hits
        .iter()
        .map(|h| {
            let sym = if h.symbol_name.is_empty() {
                String::new()
            } else {
                format!(" [{}]", h.symbol_name)
            };
            format!(
                "{}:{}-{}{}  {}",
                h.rel, h.start_line, h.end_line, sym, h.preview
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutcome::ok(
        format!("{} match(es) for '{}'", hits.len(), a.query),
        truncate(&body),
    ))
}

pub fn outline(index: &dyn CodeIndex, ws_key: &str, args: Value) -> Result<ToolOutcome, ToolError> {
    let a: OutlineArgs =
        serde_json::from_value(args).map_err(|e| ToolError::BadArgs(e.to_string()))?;
    let rows = index
        .outline(ws_key, &a.path)
        .map_err(|e| ToolError::Other(e.to_string()))?;
    if rows.is_empty() {
        return Ok(ToolOutcome::ok(
            format!("no outline for {}", a.path),
            format!("No indexed symbols in {} (read_file to view it).", a.path),
        ));
    }
    let body = rows
        .iter()
        .map(|r| {
            format!(
                "{}{} {} :{}",
                "  ".repeat(r.depth),
                r.kind,
                r.name,
                r.start_line
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutcome::ok(
        format!("{} symbol(s) in {}", rows.len(), a.path),
        truncate(&body),
    ))
}

pub fn find_mentions(
    index: &dyn CodeIndex,
    ws_key: &str,
    args: Value,
) -> Result<ToolOutcome, ToolError> {
    let a: FindMentionsArgs =
        serde_json::from_value(args).map_err(|e| ToolError::BadArgs(e.to_string()))?;
    let limit = a.max_results.unwrap_or(30).min(100) as u32;
    let hits = index
        .find_mentions(ws_key, &a.name, limit)
        .map_err(|e| ToolError::Other(e.to_string()))?;
    if hits.is_empty() {
        return Ok(ToolOutcome::ok(
            format!("no mentions of '{}'", a.name),
            format!(
                "No indexed mentions of '{}'. This is keyword search — try lsp_find_references or grep for precise uses.",
                a.name
            ),
        ));
    }
    let body = hits
        .iter()
        .map(|h| format!("{}:{}-{}  {}", h.rel, h.start_line, h.end_line, h.preview))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutcome::ok(
        format!("{} mention(s) of '{}'", hits.len(), a.name),
        truncate(&body),
    ))
}

pub fn recall(
    store: &tetonic_memory::Store,
    ws: &Workspace,
    session_id: Option<&str>,
    args: Value,
) -> Result<ToolOutcome, ToolError> {
    let a: RecallArgs =
        serde_json::from_value(args).map_err(|e| ToolError::BadArgs(e.to_string()))?;
    let limit = a.max_results.unwrap_or(8).min(20) as u32;
    let hits = store
        .recall_history(ws.root(), &a.query, limit, session_id)
        .map_err(|_| ToolError::Other("recall unavailable".into()))?;
    if hits.is_empty() {
        return Ok(ToolOutcome::ok(
            format!("no recall hits for '{}'", a.query),
            "No prior sessions matched. Try broader keywords or check project digest in briefing."
                .to_string(),
        ));
    }
    let body = hits
        .iter()
        .map(|h| {
            let snippet = if h.kind == "tool" {
                format!("<untrusted recall>\n{}\n</untrusted recall>", h.snippet)
            } else {
                h.snippet.clone()
            };
            let class_tag = tetonic_policy::classify_text_content(&snippet)
                .map(|c| format!("[class:{}] ", tetonic_policy::data_class_name(c.class)))
                .unwrap_or_default();
            format!(
                "{class_tag}[{} {} {}] {}: {}",
                h.started_at, h.kind, h.label, h.session_id, snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutcome::ok(
        format!("{} recall hit(s) for '{}'", hits.len(), a.query),
        truncate(&body),
    ))
}
