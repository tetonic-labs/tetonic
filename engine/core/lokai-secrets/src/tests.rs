use crate::detectors::default_detectors;
use crate::scanner::ScannerEngine;
use lokai_domain::secrets::{ScanContext, SecretScanner};
use sha2::Digest;

#[tokio::test]
async fn test_scan_and_redact_aws_key() {
    let scanner = ScannerEngine::default_engine();
    let text = "Here is my key: AKIAIOSFODNN7EXAMPLE\nDon't share it!";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    assert!(result.is_some());
    let (_records, redacted) = result.unwrap();
    assert_eq!(
        redacted,
        "Here is my key: [REDACTED:aws-access-key]\nDon't share it!"
    );
}

#[tokio::test]
async fn test_scan_and_redact_pem_key() {
    let scanner = ScannerEngine::default_engine();
    let text =
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    assert!(result.is_some());
    let (_records, redacted) = result.unwrap();
    assert_eq!(redacted, "[REDACTED:pem-private-key]");
}

#[tokio::test]
async fn test_clean_text() {
    let scanner = ScannerEngine::default_engine();
    let text = "This is a clean text without any secrets.";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_uuid_is_not_aws_key() {
    let scanner = ScannerEngine::default_engine();
    let text = "My id is 123e4567-e89b-12d3-a456-426614174000";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    // UUID should not trigger AWS or GitHub token detectors
    assert!(result.is_none());
}

#[tokio::test]
async fn test_env_file_omission() {
    let scanner = ScannerEngine::default_engine();
    let text = "SOME_KEY=123\nOTHER=456";
    let result = scanner.scan_and_redact(text, Some(".env")).await.unwrap();

    assert!(result.is_some());
    let (records, redacted) = result.unwrap();
    assert_eq!(redacted, ""); // Omitted completely
    assert_eq!(
        records[0].redacted_digest.0,
        format!("sha256:{}", hex::encode(sha2::Sha256::digest(b"")))
    );
}

#[tokio::test]
async fn test_split_secrets() {
    let scanner = ScannerEngine::default_engine();
    let text = "AKIAIOSFOD\nNN7EXAMPLE";
    // Usually regex doesn't match split secrets unless we do something special.
    // Assuming regex misses it, entropy detector shouldn't trigger unless we handle newlines.
    let result = scanner.scan_and_redact(text, None).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_high_entropy_hash_mistaken_for_token() {
    let scanner = ScannerEngine::default_engine();
    // 64-char hex hash shouldn't match AWS key, but might trigger entropy.
    // Entropy threshold is 4.5. Hex chars have max entropy of 4.0.
    let text = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    // Entropy of 64 hex chars is <= 4.0, which is < 4.5.
    assert!(result.is_none());
}

#[tokio::test]
async fn test_high_entropy_secret() {
    let scanner = ScannerEngine::default_engine();
    // A highly random base64 string
    let text = "password=SuperSecretRandomBase64StringWithManyChars123!";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    // The base64 part has high entropy and should be redacted.
    assert!(result.is_some());
    let (records, redacted) = result.unwrap();
    assert_eq!(redacted, "[REDACTED:high-entropy]!");
    assert_eq!(
        records[0].redacted_digest.0,
        format!("sha256:{}", hex::encode(sha2::Sha256::digest(&redacted)))
    );
}

#[tokio::test]
async fn test_file_paths_with_slashes_not_flagged_as_high_entropy() {
    let scanner = ScannerEngine::default_engine();
    let text = "Refer to docs/architecture/v4/13-COMPLETION-PROGRAM for details";
    let result = scanner.scan_and_redact(text, None).await.unwrap();
    assert!(result.is_none());

    let text2 = "File at capabilities/lokai-artifact/src/quarantine.rs updated";
    let result2 = scanner.scan_and_redact(text2, None).await.unwrap();
    assert!(result2.is_none());
}

#[tokio::test]
async fn test_binary_archive_bomb() {
    let scanner = ScannerEngine::default_engine();
    let text = "PK\x03\x04..."; // zip header
    let result = scanner
        .scan_and_redact(text, Some("payload.zip"))
        .await
        .unwrap();

    // Should be omitted completely due to binary detector
    assert!(result.is_some());
    let (records, redacted) = result.unwrap();
    assert_eq!(redacted, "");
    assert_eq!(
        records[0].redacted_digest.0,
        format!("sha256:{}", hex::encode(sha2::Sha256::digest(b"")))
    );
}

#[tokio::test]
async fn test_json_encoded_aws_key() {
    let scanner = ScannerEngine::default_engine();
    let text = r#"{"aws_access_key_id": "AKIAIOSFODNN7EXAMPLE"}"#;
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    assert!(result.is_some());
    let (_, redacted) = result.unwrap();
    assert_eq!(
        redacted,
        r#"{"aws_access_key_id": "[REDACTED:aws-access-key]"}"#
    );
}

#[tokio::test]
async fn test_yaml_encoded_aws_key() {
    let scanner = ScannerEngine::default_engine();
    let text = "aws_access_key_id: 'AKIAIOSFODNN7EXAMPLE'";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    assert!(result.is_some());
    let (_, redacted) = result.unwrap();
    assert_eq!(redacted, "aws_access_key_id: '[REDACTED:aws-access-key]'");
}

#[tokio::test]
async fn test_unusual_quoting_github_token() {
    let scanner = ScannerEngine::default_engine();
    let text = "Token is `ghp_1234567890abcdefghijklmnopqrstuvwxyz` here";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    assert!(result.is_some());
    let (_, redacted) = result.unwrap();
    assert_eq!(redacted, "Token is `[REDACTED:github-token]` here");
}

#[tokio::test]
async fn test_multiline_json_pem_key() {
    let scanner = ScannerEngine::default_engine();
    let text = "{\n  \"private_key\": \"-----BEGIN RSA PRIVATE KEY-----\\nMIIEpAIBAAKCAQEA...\\n-----END RSA PRIVATE KEY-----\"\n}";
    let result = scanner.scan_and_redact(text, None).await.unwrap();

    // Note: the regex for pem-private-key relies on specific formatting, but the \n might cause issues if literal.
    // Assuming the user rule catches literal \n if they adjust the regex, but the current regex uses [\s\S]+? which will match.
    assert!(result.is_some());
}

#[tokio::test]
async fn test_oversized_file() {
    let scanner = ScannerEngine::default_engine();
    // Simulate an oversized file by using a string that contains a token
    // In actual use, the artifact store layer cuts it off at 1MB, but ScannerEngine handles it.
    let mut large_text = String::with_capacity(1024 * 1024 + 100);
    for _ in 0..10240 {
        large_text.push_str(
            "Some innocuous log line with no secrets whatsoever.......................\n",
        );
    }
    large_text.push_str("AKIAIOSFODNN7EXAMPLE\n");

    let result = scanner.scan_and_redact(&large_text, None).await.unwrap();
    assert!(result.is_some());
    let (_, redacted) = result.unwrap();
    assert!(redacted.contains("[REDACTED:aws-access-key]"));
}

#[tokio::test]
async fn test_unicode_secret() {
    let scanner = ScannerEngine::default_engine();
    // Simulate a secret embedded in unicode text
    let text = "🔑 這是我的秘密: AKIAIOSFODNN7EXAMPLE 🔑";
    let result = scanner.scan_and_redact(text, None).await.unwrap();
    assert!(result.is_some());
    let (_, redacted) = result.unwrap();
    assert_eq!(redacted, "🔑 這是我的秘密: [REDACTED:aws-access-key] 🔑");
}

#[tokio::test]
async fn test_cached_clean_result_after_rule_update() {
    let scanner = ScannerEngine::default_engine();
    let text = "this_is_a_test_token_999999";

    // Initially not detected
    let result1 = scanner.scan_and_redact(text, None).await.unwrap();
    assert!(result1.is_none());

    // Now add a rule
    use crate::detectors::RegexDetector;
    use crate::types::*;
    use regex::Regex;
    use std::sync::Arc;
    let detector = RegexDetector {
        rule_id: "custom_rule".to_string(),
        version: 1,
        regex: Regex::new(r"this_is_a_test_token_\d+").unwrap(),
        kind: SecretKind::ProviderToken,
        confidence: FindingConfidence::Confirmed,
    };
    scanner.add_user_rule(Arc::new(detector));

    // Re-scan. Because rule update clears cache and bumps version, this should detect it now.
    let result2 = scanner.scan_and_redact(text, None).await.unwrap();
    assert!(result2.is_some());
    let (_, redacted) = result2.unwrap();
    assert_eq!(redacted, "[REDACTED:custom_rule]");
}

#[tokio::test]
async fn test_override_fingerprint_scope() {
    let scanner = ScannerEngine::default_engine();
    let text1 = "Token is AKIAIOSFODNN7EXAMPLE";
    let text2 = "Token is AKIAIOSFODNN8EXAMPLE"; // Different token

    // First, verify both are redacted
    let res1_before = scanner.scan_and_redact(text1, None).await.unwrap().unwrap();
    let res2_before = scanner.scan_and_redact(text2, None).await.unwrap().unwrap();
    assert_eq!(res1_before.1, "Token is [REDACTED:aws-access-key]");
    assert_eq!(res2_before.1, "Token is [REDACTED:aws-access-key]");

    // Compute fingerprint for text1's token "AKIAIOSFODNN7EXAMPLE"
    let token = "AKIAIOSFODNN7EXAMPLE";
    let fingerprint = default_detectors()[1].scan(token, None, &scanner.hmac_key)[0]
        .fingerprint
        .0
        .clone();

    // Allow fingerprint
    scanner.allow_fingerprint(&fingerprint);

    // Scan text1 again -> should be ignored (None or no redaction for this token)
    let res1_after = scanner.scan_and_redact(text1, None).await.unwrap();
    assert!(
        res1_after.is_none(),
        "text1 should not be redacted because its fingerprint was allowed"
    );

    // Scan text2 again -> should still be redacted
    let res2_after = scanner.scan_and_redact(text2, None).await.unwrap().unwrap();
    assert_eq!(res2_after.1, "Token is [REDACTED:aws-access-key]");
}

#[tokio::test]
async fn scoped_override_does_not_apply_out_of_scope() {
    use crate::OverrideScope;
    let scanner = ScannerEngine::with_hmac_key(default_detectors(), "fixed-hmac".into());
    let text = "Token is AKIAIOSFODNN7EXAMPLE";
    let token = "AKIAIOSFODNN7EXAMPLE";
    let fingerprint = default_detectors()[1].scan(token, None, &scanner.hmac_key)[0]
        .fingerprint
        .0
        .clone();

    scanner.grant_override(&fingerprint, OverrideScope::Session("s-a".into()));

    // Wrong session: still redact
    assert!(scanner
        .scan_and_redact_in_context(
            text,
            None,
            ScanContext {
                session_id: Some("s-b"),
                project_id: None
            }
        )
        .await
        .unwrap()
        .is_some());

    // Matching session: allow
    assert!(scanner
        .scan_and_redact_in_context(
            text,
            None,
            ScanContext {
                session_id: Some("s-a"),
                project_id: None
            }
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn revoke_override_restores_redaction() {
    use crate::OverrideScope;
    let scanner = ScannerEngine::with_hmac_key(default_detectors(), "fixed-hmac".into());
    let text = "Token is AKIAIOSFODNN7EXAMPLE";
    let token = "AKIAIOSFODNN7EXAMPLE";
    let fingerprint = default_detectors()[1].scan(token, None, &scanner.hmac_key)[0]
        .fingerprint
        .0
        .clone();

    scanner.grant_override(&fingerprint, OverrideScope::Global);
    assert!(scanner.scan_and_redact(text, None).await.unwrap().is_none());
    scanner.revoke_override(&fingerprint, OverrideScope::Global);
    assert!(scanner.scan_and_redact(text, None).await.unwrap().is_some());
}

#[test]
fn redact_text_sync_err_does_not_return_original_secret() {
    let secret = "AKIAIOSFODNN7EXAMPLE";
    let mapped = crate::map_scan_result(secret, Err("scanner exploded".into()));
    assert!(
        mapped.is_err(),
        "Err must stay Err, not unwrap to plaintext"
    );
    let emitted = match mapped {
        Ok((text, _)) => text,
        Err(_) => crate::SCAN_FAILED_PLACEHOLDER.to_string(),
    };
    assert!(
        !emitted.contains(secret),
        "scan Err must not emit the original secret: {emitted}"
    );
    assert_eq!(emitted, crate::SCAN_FAILED_PLACEHOLDER);
}

#[test]
fn redact_text_sync_ok_none_keeps_clean_text() {
    let scanner = ScannerEngine::default_engine();
    let text = "no secrets here";
    let (out, hit) = crate::redact_text_sync(&scanner, text).expect("scan");
    assert!(!hit);
    assert_eq!(out, text);
}

/// PRO-01 regression: equal-length content change at the same path must
/// invalidate the cache and detect the newly-introduced secret.
#[tokio::test]
async fn test_cache_invalidates_on_equal_length_content_change() {
    let scanner = ScannerEngine::default_engine();
    let path = Some("config/settings.rs");

    // Clean text — no secrets. 42 bytes.
    let clean = "xx]abcdefghijklmnopqrstu[padding-text]xxxx";
    let dirty = "xx]AKIAIOSFODNN7EXAMPLE[padding-text]xxxxx";
    assert_eq!(clean.len(), dirty.len(), "precondition: equal byte length");
    assert_eq!(clean.len(), 42);

    let result1 = scanner.scan_and_redact(clean, path).await.unwrap();
    assert!(result1.is_none(), "clean text should produce no findings");

    let result2 = scanner.scan_and_redact(dirty, path).await.unwrap();
    assert!(
        result2.is_some(),
        "PRO-01: equal-length secret-bearing edit must NOT return cached clean result"
    );
    let (_, redacted) = result2.unwrap();
    assert!(
        redacted.contains("[REDACTED:aws-access-key]"),
        "secret must be redacted, got: {redacted}"
    );
}

#[test]
fn redaction_receipts_hash_the_actual_bytes() {
    let scanner = ScannerEngine::default_engine();
    for path in [None, Some(".env")] {
        let text = "key=AKIAIOSFODNN7EXAMPLE";
        let (records, redacted) = scanner.scan_and_redact_sync(text, path).unwrap().unwrap();
        assert_eq!(
            records[0].original_digest.0,
            format!("sha256:{}", hex::encode(sha2::Sha256::digest(text)))
        );
        assert_eq!(
            records[0].redacted_digest.0,
            format!("sha256:{}", hex::encode(sha2::Sha256::digest(&redacted)))
        );
    }
}

// Pause the first detection to force permission changes while a scan is in flight.
struct PausingDetector {
    entered: std::sync::mpsc::SyncSender<()>,
    resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    first: std::sync::atomic::AtomicBool,
}

impl crate::detectors::Detector for PausingDetector {
    fn scan(&self, text: &str, path: Option<&str>, key: &str) -> Vec<crate::types::SecretFinding> {
        if self.first.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            self.resume
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        default_detectors()[1].scan(text, path, key)
    }
}

fn paused_scanner() -> (
    std::sync::Arc<ScannerEngine>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::SyncSender<()>,
    String,
) {
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let scanner = std::sync::Arc::new(ScannerEngine::with_hmac_key(
        vec![std::sync::Arc::new(PausingDetector {
            entered: entered_tx,
            resume: std::sync::Mutex::new(resume_rx),
            first: std::sync::atomic::AtomicBool::new(true),
        })],
        "fixed-hmac".into(),
    ));
    let fingerprint = default_detectors()[1].scan("AKIAIOSFODNN7EXAMPLE", None, &scanner.hmac_key)
        [0]
    .fingerprint
    .0
    .clone();
    (scanner, entered_rx, resume_tx, fingerprint)
}

#[test]
fn concurrent_sessions_do_not_share_scan_authority() {
    let (scanner, entered, resume, fingerprint) = paused_scanner();
    scanner.grant_override(
        &fingerprint,
        crate::OverrideScope::Session("allowed".into()),
    );
    let denied = scanner.clone();
    let worker = std::thread::spawn(move || {
        denied
            .scan_and_redact_scoped_sync(
                "AKIAIOSFODNN7EXAMPLE",
                None,
                ScanContext {
                    session_id: Some("denied"),
                    project_id: None,
                },
            )
            .unwrap()
    });
    entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(scanner
        .scan_and_redact_scoped_sync(
            "AKIAIOSFODNN7EXAMPLE",
            None,
            ScanContext {
                session_id: Some("allowed"),
                project_id: None
            }
        )
        .unwrap()
        .is_none());
    resume.send(()).unwrap();
    assert!(worker.join().unwrap().is_some());
    assert!(scanner
        .scan_and_redact_sync("AKIAIOSFODNN7EXAMPLE", None)
        .unwrap()
        .is_some());
}

#[test]
fn revoked_permission_cannot_be_repopulated_by_inflight_cache_write() {
    let (scanner, entered, resume, fingerprint) = paused_scanner();
    scanner.grant_override(&fingerprint, crate::OverrideScope::Global);
    let old_scan = scanner.clone();
    let worker = std::thread::spawn(move || {
        old_scan
            .scan_and_redact_sync("AKIAIOSFODNN7EXAMPLE", None)
            .unwrap()
    });
    entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    scanner.revoke_override(&fingerprint, crate::OverrideScope::Global);
    resume.send(()).unwrap();
    // This scan linearized before revocation; its old permission cache is unreachable
    // by any scan admitted after revocation, even though it writes the cache later.
    assert!(worker.join().unwrap().is_none());
    assert!(scanner
        .scan_and_redact_sync("AKIAIOSFODNN7EXAMPLE", None)
        .unwrap()
        .is_some());
}

#[test]
fn grant_during_scan_does_not_change_its_permission_snapshot() {
    let (scanner, entered, resume, fingerprint) = paused_scanner();
    let old_scan = scanner.clone();
    let worker = std::thread::spawn(move || {
        old_scan
            .scan_and_redact_sync("AKIAIOSFODNN7EXAMPLE", None)
            .unwrap()
    });
    entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    scanner.grant_override(&fingerprint, crate::OverrideScope::Global);
    resume.send(()).unwrap();
    assert!(worker.join().unwrap().is_some());
    assert!(scanner
        .scan_and_redact_sync("AKIAIOSFODNN7EXAMPLE", None)
        .unwrap()
        .is_none());
}

#[test]
fn project_scope_takes_precedence_and_empty_ids_are_unscoped() {
    let scanner = ScannerEngine::with_hmac_key(default_detectors(), "fixed-hmac".into());
    let text = "AKIAIOSFODNN7EXAMPLE";
    let fingerprint = default_detectors()[1].scan(text, None, &scanner.hmac_key)[0]
        .fingerprint
        .0
        .clone();
    scanner.grant_override(
        &fingerprint,
        crate::OverrideScope::Session("session".into()),
    );
    let context = ScanContext {
        session_id: Some("session"),
        project_id: Some("project"),
    };
    assert!(scanner
        .scan_and_redact_scoped_sync(text, None, context)
        .unwrap()
        .is_some());
    scanner.grant_override(
        &fingerprint,
        crate::OverrideScope::Project("project".into()),
    );
    assert!(scanner
        .scan_and_redact_scoped_sync(text, None, context)
        .unwrap()
        .is_none());
    assert!(scanner
        .scan_and_redact_scoped_sync(
            text,
            None,
            ScanContext {
                session_id: Some(""),
                project_id: Some("")
            }
        )
        .unwrap()
        .is_some());
}

#[test]
fn pem_variants_redact_whole_blocks_and_preserve_public_material() {
    let scanner = ScannerEngine::default_engine();
    for label in [
        "PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "DSA PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "PGP PRIVATE KEY",
    ] {
        let text = format!(
            "before\n-----BEGIN {label}-----\r\nprivate-payload\r\n-----END {label}-----\nafter"
        );
        let (_, redacted) = scanner
            .scan_and_redact_sync(&text, None)
            .unwrap()
            .expect("private key detected");
        assert_eq!(redacted, "before\n[REDACTED:pem-private-key]\nafter");
    }
    for label in ["PUBLIC KEY", "CERTIFICATE"] {
        let text = format!("-----BEGIN {label}-----\npublic-payload\n-----END {label}-----");
        assert!(scanner.scan_and_redact_sync(&text, None).unwrap().is_none());
    }
}
