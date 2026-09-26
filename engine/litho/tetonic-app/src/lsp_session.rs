//! Product-pack [`LspSession`] / [`LspSessionOpen`] over `lokai-lsp` + sandbox launcher.

use std::path::Path;
use std::sync::Arc;

use tetonic_domain::{LspSession, LspSessionOpen, ToolOutcome};
use tetonic_lsp::{available_languages, lsp_enabled_by_env, LspPool};
use tetonic_tools::process_executor::{coding_executor, EnforcementLevel};

use crate::lsp_launcher::SandboxLspLauncher;

const MAX_OUTPUT_BYTES: usize = 32 * 1024;

pub struct SandboxLspSessionOpen {
    consumer: Option<Arc<dyn tetonic_domain::CapabilityConsumer>>,
}

impl SandboxLspSessionOpen {
    pub fn new(consumer: Option<Arc<dyn tetonic_domain::CapabilityConsumer>>) -> Self {
        Self { consumer }
    }
}

impl LspSessionOpen for SandboxLspSessionOpen {
    fn open(&self, root: &Path) -> Result<Box<dyn LspSession>, String> {
        let mut executor = coding_executor(root, EnforcementLevel::Sandboxed);
        if let Some(consumer) = &self.consumer {
            executor = executor.with_capability_consumer(consumer.clone());
        }
        let launcher: Option<Arc<dyn tetonic_lsp::LspProcessLauncher>> =
            if executor.uses_os_sandbox() {
                Some(SandboxLspLauncher::new(executor))
            } else {
                None
            };
        let pool = LspPool::new_with_launcher(root, launcher).map_err(|e| e.to_string())?;
        Ok(Box::new(PoolLspSession { pool }))
    }

    fn available(&self, root: &Path) -> bool {
        if !lsp_enabled_by_env() {
            return false;
        }
        if available_languages().is_empty() {
            return false;
        }
        LspPool::new_with_launcher(root, None).is_ok()
    }
}

struct PoolLspSession {
    pool: LspPool,
}

fn truncate_lsp(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        s.to_string()
    } else {
        // Keep compatibility with the declared Rust 1.80 MSRV.
        let mut end = MAX_OUTPUT_BYTES - "…".len();
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

impl LspSession for PoolLspSession {
    fn request_stop(&self) {
        self.pool.request_stop();
    }

    fn goto_definition(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<ToolOutcome, String> {
        match self.pool.goto_definition(path, line, character) {
            Ok(hits) if hits.is_empty() => Ok(ToolOutcome::ok(
                "no LSP definition",
                "No definition at that position. Try find_definition or search_code.",
            )),
            Ok(hits) => {
                let body = hits
                    .iter()
                    .map(|h| format!("{}:{}:{}  {}", h.path, h.line, h.character, h.message))
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(ToolOutcome::ok(
                    format!("{} definition(s)", hits.len()),
                    truncate_lsp(&body),
                ))
            }
            Err(e) => Ok(ToolOutcome {
                ok: false,
                summary: "LSP degraded".into(),
                content: format!(
                    "ERROR: LSP error ({e}). Fall back to find_definition/search_code/grep."
                ),
                error_kind: Some("lsp_degraded".into()),
                change: None,
            }),
        }
    }

    fn find_references(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<ToolOutcome, String> {
        match self.pool.find_references(path, line, character) {
            Ok(hits) if hits.is_empty() => Ok(ToolOutcome::ok(
                "no LSP references",
                "No references at that position.",
            )),
            Ok(hits) => {
                let body = hits
                    .iter()
                    .map(|h| format!("{}:{}:{}", h.path, h.line, h.character))
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(ToolOutcome::ok(
                    format!("{} reference(s)", hits.len()),
                    truncate_lsp(&body),
                ))
            }
            Err(e) => Ok(ToolOutcome {
                ok: false,
                summary: "LSP degraded".into(),
                content: format!("ERROR: LSP error ({e}). Fall back to find_mentions/grep."),
                error_kind: Some("lsp_degraded".into()),
                change: None,
            }),
        }
    }

    fn diagnostics(&self, path: &str) -> Result<ToolOutcome, String> {
        match self.pool.diagnostics(path) {
            Ok(hits) if hits.is_empty() => Ok(ToolOutcome::ok(
                "no diagnostics",
                "No diagnostics reported (file may be clean or server still indexing).",
            )),
            Ok(hits) => {
                let body = hits
                    .iter()
                    .map(|d| {
                        format!(
                            "{}:{}:{} [{}/{}] {}",
                            d.path,
                            d.line,
                            d.character,
                            d.severity,
                            d.source.as_deref().unwrap_or("?"),
                            d.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(ToolOutcome::ok(
                    format!("{} diagnostic(s)", hits.len()),
                    truncate_lsp(&body),
                ))
            }
            Err(e) => Ok(ToolOutcome {
                ok: false,
                summary: "LSP degraded".into(),
                content: format!(
                    "ERROR: LSP error ({e}). Continue with index tools or run verify."
                ),
                error_kind: Some("lsp_degraded".into()),
                change: None,
            }),
        }
    }
}

#[cfg(test)]
mod truncation_tests {
    use super::*;
    #[test]
    fn unicode_is_safe_and_the_suffix_fits_the_byte_budget() {
        for character in ['a', 'é', '界', '😀'] {
            for offset in 0..4 {
                let input = format!(
                    "{}{}",
                    "a".repeat(MAX_OUTPUT_BYTES - 4 + offset),
                    character.to_string().repeat(8)
                );
                let output = truncate_lsp(&input);
                assert!(output.len() <= MAX_OUTPUT_BYTES);
                assert!(output.ends_with('…'));
                assert!(input.starts_with(output.strip_suffix('…').unwrap()));
            }
        }
        let exact = "a".repeat(MAX_OUTPUT_BYTES);
        assert_eq!(truncate_lsp(&exact), exact);
    }
}
