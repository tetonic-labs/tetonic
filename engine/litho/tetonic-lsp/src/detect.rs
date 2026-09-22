//! Discover language-server binaries on PATH (local subprocess only).

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Python,
    TypeScript,
}

impl Lang {
    pub fn from_path(p: &Path) -> Option<Self> {
        match p
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("rs") => Some(Lang::Rust),
            Some("py") | Some("pyi") => Some(Lang::Python),
            Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => {
                Some(Lang::TypeScript)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub lang: Lang,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub label: String,
}

#[derive(Debug, Error)]
pub enum DetectError {
    #[error("no language server for this file type")]
    Unsupported,
    #[error("{0} not found on PATH")]
    NotFound(String),
}

/// Whether LSP tools are enabled (`LOKAI_LSP=1` or `true`; default on when a server exists).
pub fn lsp_enabled_by_env() -> bool {
    match std::env::var("LOKAI_LSP").ok().as_deref() {
        Some("0") | Some("false") | Some("off") => false,
        Some("1") | Some("true") | Some("on") => true,
        _ => true,
    }
}

pub fn detect_server(lang: Lang) -> Result<ServerSpec, DetectError> {
    if let Some(spec) = test_mock_override(lang) {
        return Ok(spec);
    }
    match lang {
        Lang::Rust => detect_rust_analyzer(),
        Lang::Python => detect_pyright(),
        Lang::TypeScript => detect_typescript(),
    }
}

/// When `LOKAI_LSP_TEST_MOCK=1`, route spawns to `LOKAI_LSP_TEST_MOCK_BIN` (integration tests).
fn test_mock_override(lang: Lang) -> Option<ServerSpec> {
    if std::env::var("LOKAI_LSP_TEST_MOCK").ok().as_deref() != Some("1") {
        return None;
    }
    let program = std::env::var_os("LOKAI_LSP_TEST_MOCK_BIN").map(PathBuf::from)?;
    let args = std::env::var("LOKAI_LSP_TEST_MOCK_ARGS")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(ServerSpec {
        lang,
        program,
        args,
        label: "test-mock".into(),
    })
}

fn detect_rust_analyzer() -> Result<ServerSpec, DetectError> {
    let program =
        which("rust-analyzer").ok_or_else(|| DetectError::NotFound("rust-analyzer".into()))?;
    Ok(ServerSpec {
        lang: Lang::Rust,
        program,
        args: Vec::new(),
        label: "rust-analyzer".into(),
    })
}

fn detect_pyright() -> Result<ServerSpec, DetectError> {
    if let Some(program) = which("pyright-langserver") {
        return Ok(ServerSpec {
            lang: Lang::Python,
            program,
            args: vec!["--stdio".into()],
            label: "pyright-langserver".into(),
        });
    }
    Err(DetectError::NotFound("pyright-langserver".into()))
}

fn detect_typescript() -> Result<ServerSpec, DetectError> {
    if let Some(program) = which("typescript-language-server") {
        return Ok(ServerSpec {
            lang: Lang::TypeScript,
            program,
            args: vec!["--stdio".into()],
            label: "typescript-language-server".into(),
        });
    }
    if let Some(program) = which("vtsls") {
        return Ok(ServerSpec {
            lang: Lang::TypeScript,
            program,
            args: vec!["--stdio".into()],
            label: "vtsls".into(),
        });
    }
    Err(DetectError::NotFound(
        "typescript-language-server or vtsls".into(),
    ))
}

fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe_name(name));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn exe_name(name: &str) -> String {
    if name.ends_with(".exe") {
        name.to_string()
    } else {
        format!("{name}.exe")
    }
}

#[cfg(not(windows))]
fn exe_name(name: &str) -> String {
    name.to_string()
}

/// Languages with at least one server on PATH.
pub fn available_languages() -> Vec<Lang> {
    let mut out = Vec::new();
    if detect_rust_analyzer().is_ok() {
        out.push(Lang::Rust);
    }
    if detect_pyright().is_ok() {
        out.push(Lang::Python);
    }
    if detect_typescript().is_ok() {
        out.push(Lang::TypeScript);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_from_extension() {
        assert_eq!(Lang::from_path(Path::new("foo.rs")), Some(Lang::Rust));
        assert_eq!(Lang::from_path(Path::new("bar.py")), Some(Lang::Python));
        assert_eq!(Lang::from_path(Path::new("app.ts")), Some(Lang::TypeScript));
        assert_eq!(
            Lang::from_path(Path::new("app.tsx")),
            Some(Lang::TypeScript)
        );
        assert_eq!(
            Lang::from_path(Path::new("index.js")),
            Some(Lang::TypeScript)
        );
        assert_eq!(Lang::from_path(Path::new("readme.md")), None);
    }
}
