use super::tests::{base_request, expansion_request, make_evidence, MockProvider};
use crate::{pipeline::ContextCompiler, types::PathPolicy};
use std::sync::Arc;
use tetonic_domain::classify::DataClass;

#[tokio::test]
async fn cache_separates_scope_owner_retrieval_and_artifacts() {
    let request = base_request();
    let pack = ContextCompiler::new(Arc::new(MockProvider::default()))
        .compile(request.clone())
        .await
        .unwrap();
    let cache = crate::cache::ContextCache::new();
    cache.insert(&request, &pack);
    assert!(cache.get(&request).is_some());
    let mut changes = Vec::new();
    let mut req = request.clone();
    req.allowed_paths.rules.push("src".into());
    changes.push(req);
    let mut req = request.clone();
    req.excluded_paths.rules.push("src/private".into());
    changes.push(req);
    let mut req = request.clone();
    req.retrieval_profile.max_candidates += 1;
    changes.push(req);
    let mut req = request.clone();
    req.session_id = tetonic_domain::ids::SessionId::new("other-session");
    changes.push(req);
    let mut req = request.clone();
    req.run_id = tetonic_domain::ids::RunId::new("other-run");
    changes.push(req);
    let mut req = request.clone();
    req.task_id = tetonic_domain::ids::TaskId::new("other-task");
    changes.push(req);
    let mut req = request.clone();
    req.prior_artifacts
        .push(tetonic_domain::ids::ArtifactId::new("other-artifact"));
    changes.push(req);
    for req in changes {
        assert!(cache.get(&req).is_none());
    }
}

#[tokio::test]
async fn expansion_preserves_exclusions_and_redacts_before_returning() {
    let secret = "credential AKIAIOSFODNN7EXAMPLE in source";
    let mut provider = MockProvider::default();
    let mut allowed = make_evidence(secret, Some("src/public.rs"), DataClass::RepositorySource);
    allowed.symbol_id = Some("symbol".into());
    provider.symbol_results = vec![
        allowed,
        make_evidence(
            "excluded",
            Some("src/private/key.rs"),
            DataClass::RepositorySource,
        ),
        make_evidence(
            "prefix confusion",
            Some("src-other/key.rs"),
            DataClass::RepositorySource,
        ),
        make_evidence("pathless", None, DataClass::RepositorySource),
        make_evidence("secret class", Some("src/class.rs"), DataClass::Secret),
    ];
    let compiler = ContextCompiler::new(Arc::new(provider)).with_secret_scanner(Arc::new(
        tetonic_secrets::scanner::ScannerEngine::default_engine(),
    ));
    let mut req = base_request();
    req.objective = "find target_symbol".into();
    req.allowed_paths.rules.push("src".into());
    req.excluded_paths.rules.push("src/private".into());
    let fp = req.workspace_version.state_fingerprint();
    let pack = compiler.compile(req).await.unwrap();
    let handle = pack
        .expansion_handles
        .iter()
        .find(|h| h.query.kind == "symbol")
        .unwrap();
    assert_eq!(handle.allowed_scope.excluded_paths.rules, ["src/private"]);
    assert!(compiler
        .expand_pack(&expansion_request(&handle.handle_id.0, ""))
        .await
        .is_err());
    let results = compiler
        .expand_pack(&expansion_request(&handle.handle_id.0, &fp))
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(!results[0].text.contains("AKIAIOSFODNN7EXAMPLE"));
    use sha2::{Digest, Sha256};
    assert_eq!(
        results[0].content_digest.0,
        format!("{:x}", Sha256::digest(results[0].text.as_bytes()))
    );
}

#[test]
fn path_policy_uses_components_and_rejects_ambiguous_paths() {
    use crate::pipeline::stage3_filter::filter;
    let allowed = PathPolicy {
        rules: vec!["src".into()],
    };
    let excluded = PathPolicy {
        rules: vec!["src/private".into()],
    };
    let paths = [
        "src/ok.rs",
        "src\\nested\\ok.rs",
        "src-other/no.rs",
        "src/private/key.rs",
        "src/../outside.rs",
        "/src/no.rs",
        "C:\\src\\no.rs",
        "src/file:stream",
        "src/private./key.rs",
    ];
    let (accepted, omitted) = filter(
        paths
            .iter()
            .map(|p| make_evidence("text", Some(p), DataClass::RepositorySource))
            .collect(),
        &DataClass::RepositorySource,
        &allowed,
        &excluded,
    )
    .unwrap();
    assert_eq!(accepted.len(), 2);
    assert_eq!(omitted.len(), paths.len() - 2);
    assert!(filter(
        vec![make_evidence(
            "text",
            Some("src/ok.rs"),
            DataClass::RepositorySource
        )],
        &DataClass::RepositorySource,
        &allowed,
        &PathPolicy {
            rules: vec!["../invalid".into()]
        }
    )
    .is_err());
}

#[tokio::test]
async fn expired_handles_are_reclaimed() {
    let mut provider = MockProvider::default();
    provider.text_results.push(make_evidence(
        "pub fn example() {}",
        Some("src/example.rs"),
        DataClass::RepositorySource,
    ));
    let compiler = ContextCompiler::new(Arc::new(provider));
    let req = base_request();
    let fp = req.workspace_version.state_fingerprint();
    let pack = compiler.compile(req).await.unwrap();
    let id = &pack.expansion_handles[0].handle_id.0;
    compiler
        .handles
        .lock()
        .unwrap()
        .get_mut(id)
        .unwrap()
        .handle
        .expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    assert!(compiler
        .expand_pack(&expansion_request(id, &fp))
        .await
        .is_err());
    assert!(!compiler.handles.lock().unwrap().contains_key(id));
}

struct OmitOrFailScanner(bool);

#[async_trait::async_trait]
impl crate::interfaces::SecretScanner for OmitOrFailScanner {
    async fn scan_and_redact(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<tetonic_domain::secrets::ScanOutcome, String> {
        if self.0 {
            Err("scanner unavailable".into())
        } else {
            Ok(Some((vec![], String::new())))
        }
    }
}

#[tokio::test]
async fn file_expansion_omits_fully_redacted_content_and_fails_on_scanner_error() {
    for fail in [false, true] {
        let mut provider = MockProvider::default();
        provider.text_results.push(make_evidence(
            "safe excerpt",
            Some("src/file.rs"),
            DataClass::RepositorySource,
        ));
        provider
            .file_content
            .insert("src/file.rs".into(), "sensitive full file".into());
        let mut compiler = ContextCompiler::new(Arc::new(provider));
        let request = base_request();
        let fp = request.workspace_version.state_fingerprint();
        let pack = compiler.compile(request).await.unwrap();
        compiler.secret_scanner = Some(Arc::new(OmitOrFailScanner(fail)));
        let result = compiler
            .expand_pack(&expansion_request(
                &pack.expansion_handles[0].handle_id.0,
                &fp,
            ))
            .await;
        if fail {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_empty());
        }
    }
}
