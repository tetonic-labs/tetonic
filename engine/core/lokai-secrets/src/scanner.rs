use crate::detectors::*;
use crate::override_scope::{OverrideScope, ScopedFingerprint};
use crate::types::*;
use async_trait::async_trait;
use lokai_domain::secrets::{RedactionRecordReference, ScanContext, SecretScanner};
use lokai_domain::workspace::ContentDigest;
use sha2::Digest;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

type ScanCacheMap = HashMap<String, Option<(Vec<RedactionRecordReference>, String)>>;

pub struct ScannerEngine {
    pub detectors: Mutex<Vec<Arc<dyn Detector>>>,
    /// Legacy global allow set (also mirrored into `overrides` as Global).
    pub allowed_fingerprints: Mutex<HashSet<String>>,
    /// Scoped fingerprint overrides (R12).
    overrides: Mutex<HashSet<ScopedFingerprint>>,
    pub version: Mutex<String>,
    pub hmac_key: String,
    cache_map: Mutex<ScanCacheMap>,
    cache_queue: Mutex<VecDeque<String>>,
}

impl ScannerEngine {
    pub fn new(detectors: Vec<Arc<dyn Detector>>) -> Self {
        Self::with_hmac_key(detectors, uuid::Uuid::new_v4().to_string())
    }

    pub fn with_hmac_key(detectors: Vec<Arc<dyn Detector>>, hmac_key: String) -> Self {
        Self {
            detectors: Mutex::new(detectors),
            allowed_fingerprints: Mutex::new(HashSet::new()),
            overrides: Mutex::new(HashSet::new()),
            version: Mutex::new("1.1.0".into()),
            hmac_key,
            cache_map: Mutex::new(HashMap::new()),
            cache_queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn default_engine() -> Self {
        Self::new(default_detectors())
    }

    pub fn add_user_rule(&self, rule: Arc<dyn Detector>) {
        let mut detectors = self.detectors.lock().unwrap();
        detectors.push(rule);
        let mut version = self.version.lock().unwrap();
        *version = format!("{}-updated", *version);
        drop(version);
        drop(detectors);
        self.clear_cache();
    }

    /// Global in-memory allow (back-compat). Prefer [`grant_override`] for scoped grants.
    pub fn allow_fingerprint(&self, fingerprint: &str) {
        self.grant_override(fingerprint, OverrideScope::Global);
    }

    pub fn grant_override(&self, fingerprint: &str, scope: OverrideScope) {
        if matches!(scope, OverrideScope::Global) {
            self.allowed_fingerprints
                .lock()
                .unwrap()
                .insert(fingerprint.to_string());
        }
        self.overrides.lock().unwrap().insert(ScopedFingerprint {
            fingerprint: fingerprint.to_string(),
            scope,
        });
        self.clear_cache();
    }

    pub fn revoke_override(&self, fingerprint: &str, scope: OverrideScope) {
        self.overrides.lock().unwrap().remove(&ScopedFingerprint {
            fingerprint: fingerprint.to_string(),
            scope: scope.clone(),
        });
        if matches!(scope, OverrideScope::Global) {
            self.allowed_fingerprints
                .lock()
                .unwrap()
                .remove(fingerprint);
        }
        self.clear_cache();
    }

    pub fn hydrate_overrides(&self, entries: impl IntoIterator<Item = (String, OverrideScope)>) {
        let mut set = self.overrides.lock().unwrap();
        let mut global = self.allowed_fingerprints.lock().unwrap();
        for (fingerprint, scope) in entries {
            if matches!(scope, OverrideScope::Global) {
                global.insert(fingerprint.clone());
            }
            set.insert(ScopedFingerprint { fingerprint, scope });
        }
        drop(set);
        drop(global);
        self.clear_cache();
    }

    fn clear_cache(&self) {
        self.cache_map.lock().unwrap().clear();
        self.cache_queue.lock().unwrap().clear();
    }

    pub fn scan_and_redact_sync(
        &self,
        text: &str,
        path: Option<&str>,
    ) -> Result<Option<(Vec<RedactionRecordReference>, String)>, String> {
        self.scan_and_redact_scoped_sync(text, path, ScanContext::default())
    }

    pub fn scan_and_redact_scoped_sync(
        &self,
        text: &str,
        path: Option<&str>,
        context: ScanContext<'_>,
    ) -> Result<Option<(Vec<RedactionRecordReference>, String)>, String> {
        // Preserve project-first scope semantics without mutating shared authority.
        let scope = context
            .project_id
            .filter(|s| !s.is_empty())
            .map(|s| OverrideScope::Project(s.to_owned()))
            .or_else(|| {
                context
                    .session_id
                    .filter(|s| !s.is_empty())
                    .map(|s| OverrideScope::Session(s.to_owned()))
            });
        // Snapshot permissions once. In-flight scans can finish under this snapshot,
        // but cannot populate a cache entry usable after a grant/revocation changes it.
        let allowed: HashSet<String> = self
            .overrides
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.scope.applies_during(scope.as_ref()))
            .map(|o| o.fingerprint.clone())
            .collect();
        let mut fingerprints: Vec<_> = allowed.iter().map(String::as_str).collect();
        fingerprints.sort_unstable();
        let (detectors, version) = {
            let detectors = self.detectors.lock().unwrap();
            (detectors.clone(), self.version.lock().unwrap().clone())
        };
        // Structured encoding prevents path/version/scope delimiter collisions.
        let cache_key = hex::encode(sha2::Sha256::digest(
            serde_json::to_vec(&(path, version, fingerprints, text))
                .map_err(|_| "scan cache key encoding failed".to_string())?,
        ));
        {
            let map = self.cache_map.lock().unwrap();
            if let Some(cached) = map.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let mut findings = Vec::new();
        {
            for detector in detectors.iter() {
                findings.extend(detector.scan(text, path, &self.hmac_key));
            }
        }

        findings.retain(|f| !allowed.contains(&f.fingerprint.0));

        if findings.is_empty() {
            let mut map = self.cache_map.lock().unwrap();
            let mut queue = self.cache_queue.lock().unwrap();
            if map.len() >= 1000 {
                if let Some(oldest) = queue.pop_front() {
                    map.remove(&oldest);
                }
            }
            map.insert(cache_key.clone(), None);
            queue.push_back(cache_key);
            return Ok(None);
        }

        findings.sort_by_key(|f| f.location.byte_start);

        let file_omitted = findings.iter().any(|f| {
            f.location.byte_start == 0
                && f.location.byte_end == text.len()
                && f.secret_kind.matches_file()
        });

        if file_omitted {
            let orig = content_digest(text);
            let empty = content_digest("");
            let result = Some((
                vec![RedactionRecordReference {
                    original_digest: orig,
                    redacted_digest: empty,
                }],
                String::new(),
            ));

            let mut map = self.cache_map.lock().unwrap();
            let mut queue = self.cache_queue.lock().unwrap();
            if map.len() >= 1000 {
                if let Some(oldest) = queue.pop_front() {
                    map.remove(&oldest);
                }
            }
            map.insert(cache_key.clone(), result.clone());
            queue.push_back(cache_key);

            return Ok(result);
        }

        let mut redacted_text = String::new();
        let mut last_end = 0;
        let mut transformations = Vec::new();

        for finding in &findings {
            if finding.location.byte_start >= last_end {
                redacted_text.push_str(&text[last_end..finding.location.byte_start]);
                redacted_text.push_str(&finding.preview.0);
                last_end = finding.location.byte_end;

                transformations.push(RedactionTransformation::SpanMasked {
                    start: finding.location.byte_start,
                    end: finding.location.byte_end,
                });
            }
        }
        redacted_text.push_str(&text[last_end..]);

        let redacted_digest = content_digest(&redacted_text);
        let original_digest = content_digest(text);

        for detector in detectors.iter() {
            if !detector
                .scan(&redacted_text, path, &self.hmac_key)
                .is_empty()
            {
                let result = Some((
                    vec![RedactionRecordReference {
                        original_digest,
                        redacted_digest: content_digest(""),
                    }],
                    String::new(),
                ));

                let mut map = self.cache_map.lock().unwrap();
                let mut queue = self.cache_queue.lock().unwrap();
                if map.len() >= 1000 {
                    if let Some(oldest) = queue.pop_front() {
                        map.remove(&oldest);
                    }
                }
                map.insert(cache_key.clone(), result.clone());
                queue.push_back(cache_key);

                return Ok(result);
            }
        }

        let result = Some((
            vec![RedactionRecordReference {
                original_digest,
                redacted_digest,
            }],
            redacted_text,
        ));

        let mut map = self.cache_map.lock().unwrap();
        let mut queue = self.cache_queue.lock().unwrap();
        if map.len() >= 1000 {
            if let Some(oldest) = queue.pop_front() {
                map.remove(&oldest);
            }
        }
        map.insert(cache_key.clone(), result.clone());
        queue.push_back(cache_key);

        Ok(result)
    }
}

#[async_trait]
impl SecretScanner for ScannerEngine {
    async fn scan_and_redact(
        &self,
        text: &str,
        path: Option<&str>,
    ) -> Result<Option<(Vec<RedactionRecordReference>, String)>, String> {
        self.scan_and_redact_sync(text, path)
    }

    async fn scan_and_redact_in_context(
        &self,
        text: &str,
        path: Option<&str>,
        context: ScanContext<'_>,
    ) -> Result<Option<(Vec<RedactionRecordReference>, String)>, String> {
        self.scan_and_redact_scoped_sync(text, path, context)
    }
}

impl SecretKind {
    pub fn matches_file(&self) -> bool {
        matches!(self, SecretKind::ConnectionString)
    }
}

fn content_digest(text: &str) -> ContentDigest {
    ContentDigest(format!(
        "sha256:{}",
        hex::encode(sha2::Sha256::digest(text.as_bytes()))
    ))
}
