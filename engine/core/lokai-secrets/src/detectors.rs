use crate::types::*;
use hmac::{Hmac, Mac};
use regex::Regex;
use std::sync::{Arc, OnceLock};

pub trait Detector: Send + Sync {
    fn scan(&self, text: &str, path: Option<&str>, hmac_key: &str) -> Vec<SecretFinding>;
}

pub struct RegexDetector {
    pub rule_id: String,
    pub version: u32,
    pub regex: Regex,
    pub kind: SecretKind,
    pub confidence: FindingConfidence,
}

impl Detector for RegexDetector {
    fn scan(&self, text: &str, path: Option<&str>, hmac_key: &str) -> Vec<SecretFinding> {
        let mut findings = Vec::new();
        for mat in self.regex.find_iter(text) {
            let fp = fingerprint(hmac_key, "content", mat.as_str());

            let finding = SecretFinding {
                finding_id: uuid::Uuid::new_v4().to_string(),
                rule_id: self.rule_id.clone(),
                rule_version: self.version,
                source: SourceReference {
                    path: path.map(String::from),
                },
                location: FindingLocation {
                    byte_start: mat.start(),
                    byte_end: mat.end(),
                    line_start: None,
                    line_end: None,
                },
                secret_kind: self.kind.clone(),
                confidence: self.confidence.clone(),
                resulting_class: lokai_domain::classify::DataClass::Secret,
                fingerprint: SecretFingerprint(fp),
                preview: RedactedPreview(format!("[REDACTED:{}]", self.rule_id)),
                detected_at: chrono::Utc::now(),
            };
            findings.push(finding);
        }
        findings
    }
}

pub struct PathDetector {
    pub rule_id: String,
    pub version: u32,
    pub patterns: Vec<Regex>,
    pub kind: SecretKind,
    pub confidence: FindingConfidence,
}

impl Detector for PathDetector {
    fn scan(&self, text: &str, path: Option<&str>, hmac_key: &str) -> Vec<SecretFinding> {
        let mut findings = Vec::new();
        if let Some(p) = path {
            for pattern in &self.patterns {
                if pattern.is_match(p) {
                    let fp = fingerprint(hmac_key, "path", p);

                    let finding = SecretFinding {
                        finding_id: uuid::Uuid::new_v4().to_string(),
                        rule_id: self.rule_id.clone(),
                        rule_version: self.version,
                        source: SourceReference {
                            path: Some(p.to_string()),
                        },
                        location: FindingLocation {
                            byte_start: 0,
                            byte_end: text.len(),
                            line_start: None,
                            line_end: None,
                        },
                        secret_kind: self.kind.clone(),
                        confidence: self.confidence.clone(),
                        resulting_class: lokai_domain::classify::DataClass::Secret,
                        fingerprint: SecretFingerprint(fp),
                        preview: RedactedPreview(format!("[REDACTED_FILE:{}]", self.rule_id)),
                        detected_at: chrono::Utc::now(),
                    };
                    findings.push(finding);
                    break;
                }
            }
        }
        findings
    }
}

pub fn default_detectors() -> Vec<Arc<dyn Detector>> {
    vec![
        Arc::new(RegexDetector {
            rule_id: "pem-private-key".into(),
            version: 2,
            regex: Regex::new(r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----[\s\S]+?-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----").unwrap(),
            kind: SecretKind::PrivateKey,
            confidence: FindingConfidence::Confirmed,
        }),
        Arc::new(RegexDetector {
            rule_id: "aws-access-key".into(),
            version: 1,
            regex: Regex::new(r"(?i)(AKIA|A3T|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ASIA)[A-Z0-9]{16}").unwrap(),
            kind: SecretKind::ProviderToken,
            confidence: FindingConfidence::High,
        }),
        Arc::new(RegexDetector {
            rule_id: "github-token".into(),
            version: 1,
            regex: Regex::new(r"(?i)gh[pousr]_[a-zA-Z0-9]{36}").unwrap(),
            kind: SecretKind::ProviderToken,
            confidence: FindingConfidence::High,
        }),
        Arc::new(PathDetector {
            rule_id: "env-file".into(),
            version: 1,
            patterns: vec![Regex::new(r"(?i)\.env(\.[a-z0-9-]+)?$").unwrap()],
            kind: SecretKind::ConnectionString,
            confidence: FindingConfidence::Medium,
        }),
        Arc::new(BinaryArchiveDetector {
            rule_id: "binary-archive".into(),
            version: 1,
        }),
        Arc::new(EntropyDetector {
            rule_id: "high-entropy".into(),
            version: 1,
            threshold: 4.5,
            min_length: 20,
        })
    ]
}

pub struct BinaryArchiveDetector {
    pub rule_id: String,
    pub version: u32,
}

impl Detector for BinaryArchiveDetector {
    fn scan(&self, text: &str, path: Option<&str>, hmac_key: &str) -> Vec<SecretFinding> {
        let mut findings = Vec::new();
        if let Some(p) = path {
            let lower_path = p.to_lowercase();
            if lower_path.ends_with(".zip")
                || lower_path.ends_with(".tar.gz")
                || lower_path.ends_with(".bin")
                || lower_path.ends_with(".exe")
                || lower_path.ends_with(".dll")
                || lower_path.ends_with(".so")
            {
                let fp = fingerprint(hmac_key, "binary", p);

                let finding = SecretFinding {
                    finding_id: uuid::Uuid::new_v4().to_string(),
                    rule_id: self.rule_id.clone(),
                    rule_version: self.version,
                    source: SourceReference {
                        path: Some(p.to_string()),
                    },
                    location: FindingLocation {
                        byte_start: 0,
                        byte_end: text.len(),
                        line_start: None,
                        line_end: None,
                    },
                    secret_kind: SecretKind::HighEntropy, // Using HighEntropy as a proxy for arbitrary binary
                    confidence: FindingConfidence::Confirmed,
                    resulting_class: lokai_domain::classify::DataClass::SensitiveSource,
                    fingerprint: SecretFingerprint(fp),
                    preview: RedactedPreview(format!("[REDACTED_BINARY:{}]", self.rule_id)),
                    detected_at: chrono::Utc::now(),
                };
                findings.push(finding);
            }
        }
        findings
    }
}

pub struct EntropyDetector {
    pub rule_id: String,
    pub version: u32,
    pub threshold: f64,
    pub min_length: usize,
}

impl Detector for EntropyDetector {
    fn scan(&self, text: &str, path: Option<&str>, hmac_key: &str) -> Vec<SecretFinding> {
        let mut findings = Vec::new();
        static WORD_REGEX: OnceLock<Regex> = OnceLock::new();
        let word_regex = WORD_REGEX.get_or_init(|| Regex::new(r"[a-zA-Z0-9+/=_-]{20,}").unwrap());

        for mat in word_regex.find_iter(text) {
            let word = mat.as_str();
            if word.len() < self.min_length {
                continue;
            }

            if word.contains('/') {
                let has_high_entropy_component = word.split('/').any(|segment| {
                    segment.len() >= self.min_length && shannon_entropy(segment) > self.threshold
                });
                if !has_high_entropy_component {
                    continue;
                }
            }

            let entropy = shannon_entropy(word);
            if entropy > self.threshold {
                let fp = fingerprint(hmac_key, "entropy", word);

                let finding = SecretFinding {
                    finding_id: uuid::Uuid::new_v4().to_string(),
                    rule_id: self.rule_id.clone(),
                    rule_version: self.version,
                    source: SourceReference {
                        path: path.map(String::from),
                    },
                    location: FindingLocation {
                        byte_start: mat.start(),
                        byte_end: mat.end(),
                        line_start: None,
                        line_end: None,
                    },
                    secret_kind: SecretKind::HighEntropy,
                    confidence: FindingConfidence::Medium,
                    resulting_class: lokai_domain::classify::DataClass::SensitiveSource,
                    fingerprint: SecretFingerprint(fp),
                    preview: RedactedPreview(format!("[REDACTED:{}]", self.rule_id)),
                    detected_at: chrono::Utc::now(),
                };
                findings.push(finding);
            }
        }
        findings
    }
}

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }

    let len = s.len() as f64;
    let mut entropy = 0.0;
    for &count in counts.values() {
        let p = count as f64 / len;
        entropy -= p * p.log2();
    }
    entropy
}

/// Versioned and domain-separated so historical prefix hashes cannot be confused
/// with HMAC fingerprints. Existing findings remain historical records.
fn fingerprint(key: &str, domain: &str, value: &str) -> String {
    let mut mac =
        Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(b"lokai-secret-fingerprint-v2\0");
    mac.update(domain.as_bytes());
    mac.update(b"\0");
    mac.update(value.as_bytes());
    format!(
        "hmac-sha256:v2:{}",
        hex::encode(mac.finalize().into_bytes())
    )
}

#[cfg(test)]
mod fingerprint_tests {
    #[test]
    fn hmac_known_vector_and_domain_separation() {
        assert_eq!(
            super::fingerprint("test-key", "content", "test-value"),
            "hmac-sha256:v2:dabf7a4b98c7cf7d6b9169fccb300154d32138e4111ccc50f10310610fa30918"
        );
        assert_ne!(
            super::fingerprint("test-key", "path", "test-value"),
            super::fingerprint("test-key", "content", "test-value")
        );
    }
}
