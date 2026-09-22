//! Session data-class classification and floor helpers (M2-2 / D1).

use std::fs;
use std::path::Path;

use chrono::Utc;
use tetonic_domain::{
    combine_data_classes, Classification, ClassificationSource, DataClass, DisclosureTier,
    PolicyVersion, CLASSIFICATION_POLICY_VERSION,
};

const CLASSIFY_MAX_DEPTH: u32 = 5;
const SKIP_DIR_NAMES: &[&str] = &[".git", "target", "node_modules", ".lokai", "dist", "build"];
const SENSITIVE_FILE_NAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    "credentials.json",
    "id_rsa",
    "id_ed25519",
    "secrets.json",
];

/// Heuristic session classifier (M2-2). User override via `session/start.data_class`.
pub fn classify_session(workspace_root: &Path, goal: Option<&str>) -> Classification {
    let root_key = workspace_storage_key(workspace_root);
    let root_lower = root_key.to_ascii_lowercase();
    if root_lower.contains(".env")
        || root_lower.contains("secret")
        || root_lower.contains("credential")
        || goal.is_some_and(|g| {
            let g = g.to_ascii_lowercase();
            g.contains("secret") || g.contains(".env") || g.contains("api key")
        })
    {
        return Classification::new(
            DataClass::Secret,
            vec![
                ClassificationSource::PathPolicy,
                ClassificationSource::DefaultRule,
            ],
        );
    }
    if workspace_has_sensitive_file(workspace_root, CLASSIFY_MAX_DEPTH) {
        return Classification::new(
            DataClass::Secret,
            vec![
                ClassificationSource::PathPolicy,
                ClassificationSource::SecretDetector,
            ],
        );
    }
    Classification::new(
        DataClass::RepositorySource,
        vec![ClassificationSource::DefaultRule],
    )
}

fn workspace_storage_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if cfg!(windows) {
        raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
    } else {
        raw.into_owned()
    }
}

fn workspace_has_sensitive_file(root: &Path, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    for name in SENSITIVE_FILE_NAMES {
        if root.join(name).is_file() {
            return true;
        }
    }
    let Ok(read) = fs::read_dir(root) else {
        return false;
    };
    for ent in read.flatten() {
        let path = ent.path();
        if path.is_file() {
            let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
            if SENSITIVE_FILE_NAMES.iter().any(|s| name == *s) {
                return true;
            }
            if name.ends_with(".pem") || name.ends_with("_rsa") {
                return true;
            }
        } else if path.is_dir() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if SKIP_DIR_NAMES.iter().any(|s| name == *s) {
                continue;
            }
            if workspace_has_sensitive_file(&path, depth - 1) {
                return true;
            }
        }
    }
    false
}

/// Basic M2 secret heuristics in arbitrary text (full scanner arrives in M4-2).
pub fn classify_text_content(text: &str) -> Option<Classification> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("begin private key")
        || lower.contains("begin rsa private key")
        || lower.contains("begin openssh private key")
    {
        return Some(Classification::new(
            DataClass::Secret,
            vec![
                ClassificationSource::SecretDetector,
                ClassificationSource::ContentHeuristic,
            ],
        ));
    }
    if text.contains("AKIA") && text.len() >= 20 {
        return Some(Classification::new(
            DataClass::Secret,
            vec![
                ClassificationSource::SecretDetector,
                ClassificationSource::ContentHeuristic,
            ],
        ));
    }
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with("export ") || t.starts_with("set ") {
            let rest = t.split_once(' ').map(|(_, v)| v).unwrap_or(t);
            if let Some((key, _val)) = rest.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                if key.ends_with("_key")
                    || key.ends_with("_token")
                    || key.ends_with("_secret")
                    || key.ends_with("_password")
                    || matches!(key.as_str(), "password" | "secret" | "api_key" | "apikey")
                {
                    return Some(Classification::new(
                        DataClass::Secret,
                        vec![
                            ClassificationSource::SecretDetector,
                            ClassificationSource::ContentHeuristic,
                        ],
                    ));
                }
            }
        }
        let Some((key, _val)) = t.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "password"
                | "passwd"
                | "secret"
                | "api_key"
                | "apikey"
                | "token"
                | "access_token"
                | "private_key"
        ) || key.ends_with("_secret")
            || key.ends_with("_token")
            || key.ends_with("_key")
        {
            return Some(Classification::new(
                DataClass::Secret,
                vec![
                    ClassificationSource::SecretDetector,
                    ClassificationSource::ContentHeuristic,
                ],
            ));
        }
    }
    let bytes = lower.as_bytes();
    let mut cursor = 0;
    while let Some(pos) = lower[cursor..].find("sk-") {
        let abs_pos = cursor + pos;
        let is_start_boundary = abs_pos == 0
            || (!bytes[abs_pos - 1].is_ascii_alphanumeric() && bytes[abs_pos - 1] != b'_');
        if is_start_boundary {
            let after = &bytes[abs_pos + 3..];
            let token_len = after
                .iter()
                .take_while(|&&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                .count();
            if token_len >= 10 {
                return Some(Classification::new(
                    DataClass::Secret,
                    vec![
                        ClassificationSource::SecretDetector,
                        ClassificationSource::ContentHeuristic,
                    ],
                ));
            }
        }
        cursor = abs_pos + 3;
    }
    None
}

/// Aggregate classification from multiple message bodies (full outbound payload).
pub fn classify_message_payloads(messages: &[(&str, &str)]) -> Option<Classification> {
    let mut max: Option<DataClass> = None;
    let mut sources = Vec::new();
    for (_role, content) in messages {
        if let Some(c) = classify_text_content(content) {
            max = combine_data_classes(max.into_iter().chain(std::iter::once(c.class)));
            sources.extend(c.sources);
        }
    }
    max.map(|class| {
        let mut s = sources;
        s.push(ClassificationSource::AggregatedPayload);
        Classification {
            class,
            sources: s,
            policy_version: CLASSIFICATION_POLICY_VERSION,
            classified_at: Utc::now(),
        }
    })
}

pub fn parse_data_class(s: &str) -> Option<DataClass> {
    match s {
        // M2-2 canonical names
        "public" => Some(DataClass::Public),
        "repository_source" => Some(DataClass::RepositorySource),
        "sensitive_source" => Some(DataClass::SensitiveSource),
        "secret" => Some(DataClass::Secret),
        // Legacy wire / DB names (backward compatible)
        "private" => Some(DataClass::Secret),
        "personal" => Some(DataClass::RepositorySource),
        "circle_ok" => Some(DataClass::SensitiveSource),
        _ => None,
    }
}

pub fn data_class_name(c: DataClass) -> &'static str {
    match c {
        DataClass::Public => "public",
        DataClass::RepositorySource => "repository_source",
        DataClass::SensitiveSource => "sensitive_source",
        DataClass::Secret => "secret",
    }
}

/// Normalize persisted / wire names to canonical storage form.
pub fn normalize_data_class_name(s: &str) -> Option<String> {
    parse_data_class(s).map(|c| data_class_name(c).to_string())
}

/// Restriction rank (lower = more restrictive). SEC2-E2-026 / M2-2 sensitivity order.
pub fn data_class_rank(c: DataClass) -> u8 {
    match c {
        DataClass::Secret => 0,
        DataClass::SensitiveSource => 1,
        DataClass::RepositorySource => 2,
        DataClass::Public => 3,
    }
}

/// Most restrictive of two classes wins (session floor vs job metadata).
pub fn restrict_data_class(session: DataClass, job: DataClass) -> DataClass {
    if data_class_rank(session) <= data_class_rank(job) {
        session
    } else {
        job
    }
}

/// Combine multiple classifications — highest sensitivity wins.
pub fn combine_classifications(
    classes: impl IntoIterator<Item = Classification>,
) -> Option<Classification> {
    let mut iter = classes.into_iter();
    let first = iter.next()?;
    let mut acc = first;
    for next in iter {
        let class = restrict_data_class(acc.class, next.class);
        let mut sources = acc.sources;
        sources.extend(next.sources);
        if !sources.contains(&ClassificationSource::AggregatedPayload) {
            sources.push(ClassificationSource::AggregatedPayload);
        }
        acc = Classification {
            class,
            sources,
            policy_version: CLASSIFICATION_POLICY_VERSION,
            classified_at: Utc::now(),
        };
    }
    Some(acc)
}

/// Client override cannot reduce restriction below the workspace floor.
pub fn apply_data_class_floor(floor: DataClass, requested: Option<DataClass>) -> DataClass {
    match requested {
        None => floor,
        Some(r) if data_class_rank(r) > data_class_rank(floor) => floor,
        Some(r) => r,
    }
}

pub fn classification_from_data_class(
    class: DataClass,
    source: ClassificationSource,
) -> Classification {
    Classification::new(class, vec![source])
}

/// Default fabric disclosure tier for a session data class (SEC2-E2-029 / M2-2).
pub fn default_disclosure_tier(class: DataClass) -> DisclosureTier {
    match class {
        DataClass::Secret => DisclosureTier::MetadataOnly,
        DataClass::RepositorySource => DisclosureTier::Auditable,
        DataClass::SensitiveSource | DataClass::Public => DisclosureTier::Summary,
    }
}

pub fn policy_version() -> PolicyVersion {
    CLASSIFICATION_POLICY_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_and_name_data_classes() {
        assert_eq!(parse_data_class("secret"), Some(DataClass::Secret));
        assert_eq!(parse_data_class("private"), Some(DataClass::Secret));
        assert_eq!(
            parse_data_class("personal"),
            Some(DataClass::RepositorySource)
        );
        assert_eq!(
            parse_data_class("circle_ok"),
            Some(DataClass::SensitiveSource)
        );
        assert_eq!(parse_data_class("work"), None);
        assert_eq!(
            data_class_name(DataClass::SensitiveSource),
            "sensitive_source"
        );
    }

    #[test]
    fn restrict_data_class_takes_most_restrictive() {
        assert_eq!(
            restrict_data_class(DataClass::Secret, DataClass::SensitiveSource),
            DataClass::Secret
        );
        assert_eq!(
            restrict_data_class(DataClass::RepositorySource, DataClass::SensitiveSource),
            DataClass::SensitiveSource
        );
    }

    #[test]
    fn default_disclosure_tiers() {
        assert_eq!(
            default_disclosure_tier(DataClass::Secret),
            DisclosureTier::MetadataOnly
        );
        assert_eq!(
            default_disclosure_tier(DataClass::RepositorySource),
            DisclosureTier::Auditable
        );
    }

    #[test]
    fn classifies_nested_env_as_secret() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/.env"), "KEY=x\n").unwrap();
        assert_eq!(classify_session(tmp.path(), None).class, DataClass::Secret);
    }

    #[test]
    fn classifies_clean_repo_as_repository_source() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
        assert_eq!(
            classify_session(tmp.path(), None).class,
            DataClass::RepositorySource
        );
    }

    #[test]
    fn goal_keyword_triggers_secret() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            classify_session(tmp.path(), Some("rotate api key in prod")).class,
            DataClass::Secret
        );
    }

    #[test]
    fn content_heuristic_detects_private_key_block() {
        let c = classify_text_content("-----BEGIN PRIVATE KEY-----\nMIIE");
        assert_eq!(c.unwrap().class, DataClass::Secret);
    }

    #[test]
    fn content_heuristic_detects_env_style_secret() {
        let c = classify_text_content("API_KEY=sk-live-abc123\n");
        assert_eq!(c.unwrap().class, DataClass::Secret);
    }

    #[test]
    fn process_output_export_secret_is_secret() {
        let c = classify_text_content("export AWS_SECRET_ACCESS_KEY=supersecret\n");
        assert_eq!(c.unwrap().class, DataClass::Secret);
    }

    #[test]
    fn derived_summary_retains_secret_material() {
        let summary =
            "Earlier the user pasted -----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----";
        let c = classify_text_content(summary);
        assert_eq!(c.unwrap().class, DataClass::Secret);
    }

    #[test]
    fn recall_snippet_with_secret_classifies_as_secret() {
        let recall = "[class:repository_source]\npassword=hunter2\nfrom prior session";
        let c = classify_text_content(recall);
        assert_eq!(c.unwrap().class, DataClass::Secret);
    }

    #[test]
    fn sk_api_key_standalone_is_secret() {
        let c1 = classify_text_content("Bearer sk-ant-api03-secret12345");
        assert_eq!(c1.unwrap().class, DataClass::Secret);

        let c2 = classify_text_content("sk-proj-abc1234567890xyz");
        assert_eq!(c2.unwrap().class, DataClass::Secret);
    }

    #[test]
    fn words_ending_in_sk_are_not_secret() {
        let text = "The coding assistant is a task-oriented software-work application.";
        assert!(classify_text_content(text).is_none());

        let text2 = "risk-reward evaluation for disk-bound tasks with desk-check";
        assert!(classify_text_content(text2).is_none());
    }
}
