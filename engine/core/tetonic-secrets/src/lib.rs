pub mod detectors;
pub mod eval;
pub mod override_scope;
pub mod private_file;
pub mod scanner;
pub mod types;

pub use override_scope::OverrideScope;
pub use scanner::ScannerEngine;

use serde_json::Value;
use std::sync::OnceLock;
use tetonic_domain::secrets::RedactionRecordReference;

/// Emitted when the scanner returns `Err`. Never the original plaintext.
pub const SCAN_FAILED_PLACEHOLDER: &str = "[REDACTED_SCAN_FAILED]";

pub(crate) fn map_scan_result(
    text: &str,
    result: Result<Option<(Vec<RedactionRecordReference>, String)>, String>,
) -> Result<(String, bool), String> {
    match result {
        Ok(Some((_, redacted))) => Ok((redacted, true)),
        Ok(None) => Ok((text.to_string(), false)),
        Err(e) => Err(e),
    }
}

/// Sync redact for event/telemetry/tool edges (R4-3).
///
/// `Ok((text, hit))`: `hit` is true when the scanner found material.
/// `Ok` with `hit = false` keeps the original. `Err` never yields the original.
pub fn redact_text_sync(scanner: &ScannerEngine, text: &str) -> Result<(String, bool), String> {
    map_scan_result(text, scanner.scan_and_redact_sync(text, None))
}

/// Emit-safe redact: scanner `Err` becomes [`SCAN_FAILED_PLACEHOLDER`], never plaintext.
pub fn redact_text_sync_lossy(scanner: &ScannerEngine, text: &str) -> String {
    match redact_text_sync(scanner, text) {
        Ok((out, _)) => out,
        Err(_) => SCAN_FAILED_PLACEHOLDER.to_string(),
    }
}

/// Walk JSON string leaves through [`redact_text_sync`].
pub fn redact_json_value(scanner: &ScannerEngine, value: &Value) -> Result<(Value, bool), String> {
    match value {
        Value::String(s) => {
            let (out, hit) = redact_text_sync(scanner, s)?;
            Ok((Value::String(out), hit))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            let mut hit = false;
            for item in items {
                let (v, h) = redact_json_value(scanner, item)?;
                hit |= h;
                out.push(v);
            }
            Ok((Value::Array(out), hit))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            let mut hit = false;
            for (k, v) in map {
                let (nv, h) = redact_json_value(scanner, v)?;
                hit |= h;
                out.insert(k.clone(), nv);
            }
            Ok((Value::Object(out), hit))
        }
        other => Ok((other.clone(), false)),
    }
}

static INSTALLED_SCANNER: OnceLock<ScannerEngine> = OnceLock::new();
static DEFAULT_SCANNER: OnceLock<ScannerEngine> = OnceLock::new();

/// Install the store-hydrated engine used by process IO and telemetry.
/// First successful install wins; later calls are ignored.
pub fn install_shared_scanner(engine: ScannerEngine) {
    let _ = INSTALLED_SCANNER.set(engine);
}

/// Shared process-wide engine for formatters that cannot hold an `Arc` (telemetry).
/// Prefers [`install_shared_scanner`]; otherwise a process-local default engine.
pub fn shared_scanner() -> &'static ScannerEngine {
    INSTALLED_SCANNER
        .get()
        .unwrap_or_else(|| DEFAULT_SCANNER.get_or_init(ScannerEngine::default_engine))
}

#[cfg(test)]
mod tests;

pub mod key_storage;
