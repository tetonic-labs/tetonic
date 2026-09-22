//! Build a store-hydrated [`ScannerEngine`] (R12 durable overrides).

use std::sync::Arc;

use tetonic_memory::{SecretOverrideScope, SharedStore, Store};
use tetonic_secrets::detectors::default_detectors;
use tetonic_secrets::override_scope::OverrideScope;
use tetonic_secrets::ScannerEngine;

fn to_engine_scope(scope: SecretOverrideScope) -> OverrideScope {
    match scope {
        SecretOverrideScope::Global => OverrideScope::Global,
        SecretOverrideScope::Session(s) => OverrideScope::Session(s),
        SecretOverrideScope::Project(p) => OverrideScope::Project(p),
    }
}

pub fn to_store_scope(scope: &OverrideScope) -> SecretOverrideScope {
    match scope {
        OverrideScope::Global => SecretOverrideScope::Global,
        OverrideScope::Session(s) => SecretOverrideScope::Session(s.clone()),
        OverrideScope::Project(p) => SecretOverrideScope::Project(p.clone()),
    }
}

/// Hydrate HMAC + durable overrides from `lokai.db`.
pub fn scanner_engine_from_store(store: &Store) -> ScannerEngine {
    let hmac = match store.ensure_scanner_hmac_key() {
        Ok(k) => k,
        Err(_) => format!("ephemeral_{}", std::process::id()),
    };
    let engine = ScannerEngine::with_hmac_key(default_detectors(), hmac);
    if let Ok(rows) = store.list_active_durable_secret_overrides() {
        engine.hydrate_overrides(
            rows.into_iter()
                .map(|r| (r.fingerprint, to_engine_scope(r.scope))),
        );
    }
    engine
}

pub fn scanner_from_shared_store(store: &Option<SharedStore>) -> Arc<ScannerEngine> {
    match store {
        Some(s) => s
            .read_sync(|db| Arc::new(scanner_engine_from_store(db)))
            .unwrap_or_else(|_| Arc::new(ScannerEngine::default_engine())),
        None => Arc::new(ScannerEngine::default_engine()),
    }
}

/// Hydrate the process-wide telemetry/process-IO scanner from `lokai.db`.
/// No-op when the store is missing so a later store-backed install can still win.
pub fn install_shared_scanner_from_store(store: &Option<SharedStore>) {
    let Some(s) = store else {
        return;
    };
    if let Ok(engine) = s.read_sync(scanner_engine_from_store) {
        tetonic_secrets::install_shared_scanner(engine);
    }
}

/// The artifact scan policy, built once and shared by every assembly that seals
/// artifacts (M6, ADR-V3-016).
///
/// This exists so `lokai-eval` scans with the same engine production does.
/// Before M6 the hook was constructed inline in the session bootstrap, the eval
/// binaries built their stores without one, and CI therefore proved the system
/// works on a path that skipped the scanner. Duplicating the closure into eval
/// would satisfy the compiler and not the ADR.
pub fn artifact_scan_policy(store: &Option<SharedStore>) -> tetonic_artifact::ScanPolicy {
    let scanner = scanner_from_shared_store(store);
    tetonic_artifact::ScanPolicy::Scan(Arc::new(move |text: &str| -> bool {
        if let Ok(Some((findings, _))) = scanner.scan_and_redact_sync(text, None) {
            !findings.is_empty()
        } else {
            false
        }
    }))
}
