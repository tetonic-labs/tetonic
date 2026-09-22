//! R12: durable secret override hydrates across store reopen.

use crate::scanner_engine_from_store;
use tetonic_domain::secrets::SecretScanner;
use tetonic_memory::{SecretOverrideScope, Store};
use tetonic_secrets::detectors::default_detectors;

#[tokio::test]
async fn durable_override_survives_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r12.db");
    let token = "AKIAIOSFODNN7EXAMPLE";
    let text = "Token is AKIAIOSFODNN7EXAMPLE";

    let fingerprint = {
        let store = Store::open(&path).unwrap();
        let eng = scanner_engine_from_store(&store);
        let fp = default_detectors()[1].scan(token, None, &eng.hmac_key)[0]
            .fingerprint
            .0
            .clone();
        store
            .grant_secret_override(&fp, SecretOverrideScope::Global, true, Some("t"))
            .unwrap();
        fp
    };

    let store2 = Store::open(&path).unwrap();
    let eng2 = scanner_engine_from_store(&store2);
    assert!(
        eng2.scan_and_redact(text, None).await.unwrap().is_none(),
        "hydrated durable allow must suppress redaction"
    );

    // Fresh engine with same HMAC but without hydrate still redacts.
    let bare = tetonic_secrets::ScannerEngine::with_hmac_key(
        default_detectors(),
        store2.ensure_scanner_hmac_key().unwrap(),
    );
    assert!(bare.scan_and_redact(text, None).await.unwrap().is_some());
    let _ = fingerprint;
}
