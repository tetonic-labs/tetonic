//! Remote artifact quarantine (M3-3 / M5-4).

use lokai_domain::artifact::{ArtifactError, ArtifactMetadata, VerificationState};
use lokai_domain::result_integrity::ArtifactOrigin;
use lokai_domain::workspace::ContentDigest;

/// Default max remote artifact size (100 MiB) when caller does not supply a tighter limit.
pub const DEFAULT_MAX_REMOTE_ARTIFACT_BYTES: u64 = 100 * 1024 * 1024;
pub const DEFAULT_MAX_DECLARED_PATHS: usize = 10_000;
pub const DEFAULT_MAX_ARCHIVE_ENTRIES: u64 = 10_000;
pub const DEFAULT_MAX_ARCHIVE_UNCOMPRESSED: u64 = 500 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct QuarantineLimits {
    pub max_bytes: u64,
    pub max_declared_paths: usize,
    pub max_archive_entries: u64,
    pub max_archive_uncompressed_bytes: u64,
    pub allow_executable: bool,
}

impl Default for QuarantineLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_REMOTE_ARTIFACT_BYTES,
            max_declared_paths: DEFAULT_MAX_DECLARED_PATHS,
            max_archive_entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
            max_archive_uncompressed_bytes: DEFAULT_MAX_ARCHIVE_UNCOMPRESSED,
            allow_executable: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QuarantineInspection {
    pub declared_paths: Vec<String>,
    /// Locally measured archive entry count, never an untrusted manifest claim.
    /// The current byte inspector rejects archives instead of extracting them.
    pub archive_entries: Option<u64>,
    /// Locally measured expansion size; a decoder must bound work while reading.
    pub archive_uncompressed_bytes: Option<u64>,
    /// True when content looks like an executable/script payload.
    pub executable_content: bool,
}

/// Quarantine checks for a remote artifact. On success, marks origin Remote and
/// advances Unverified → StructurallyValid. Does not authorize workspace apply,
/// execution, memory promotion, or capability issuance.
pub fn validate_remote_artifact(
    meta: &mut ArtifactMetadata,
    actual_digest: &ContentDigest,
) -> Result<(), ArtifactError> {
    validate_remote_artifact_with(
        meta,
        actual_digest,
        &QuarantineLimits::default(),
        &QuarantineInspection::default(),
    )
}

pub fn validate_remote_artifact_with(
    meta: &mut ArtifactMetadata,
    actual_digest: &ContentDigest,
    limits: &QuarantineLimits,
    inspection: &QuarantineInspection,
) -> Result<(), ArtifactError> {
    meta.origin = ArtifactOrigin::Remote;

    if meta.content_digest != *actual_digest {
        return Err(ArtifactError::DigestMismatch {
            expected: meta.content_digest.clone(),
            actual: actual_digest.clone(),
        });
    }

    if meta.schema_version != 1 {
        return Err(ArtifactError::InvalidStateTransition(format!(
            "Unknown artifact schema version: {}",
            meta.schema_version
        )));
    }

    if meta.size_bytes > limits.max_bytes {
        return Err(ArtifactError::QuarantineRejected(format!(
            "artifact size {} exceeds limit {}",
            meta.size_bytes, limits.max_bytes
        )));
    }

    if inspection.declared_paths.len() > limits.max_declared_paths {
        return Err(ArtifactError::QuarantineRejected(
            "too many declared paths".into(),
        ));
    }

    for path in &inspection.declared_paths {
        if !is_safe_workspace_path(path) {
            return Err(ArtifactError::QuarantineRejected(format!(
                "unsafe path: {path}"
            )));
        }
    }

    if let Some(entries) = inspection.archive_entries {
        if entries > limits.max_archive_entries {
            return Err(ArtifactError::QuarantineRejected(
                "archive entry count exceeds limit (possible archive bomb)".into(),
            ));
        }
    }
    if let Some(uncompressed) = inspection.archive_uncompressed_bytes {
        if uncompressed > limits.max_archive_uncompressed_bytes {
            return Err(ArtifactError::QuarantineRejected(
                "archive uncompressed size exceeds limit (possible archive bomb)".into(),
            ));
        }
        if meta.size_bytes > 0 && uncompressed / meta.size_bytes > 100 {
            return Err(ArtifactError::QuarantineRejected(
                "archive compression ratio exceeds policy".into(),
            ));
        }
    }

    if inspection.executable_content && !limits.allow_executable {
        return Err(ArtifactError::QuarantineRejected(
            "executable payload rejected by quarantine policy".into(),
        ));
    }

    if meta.verification_state == VerificationState::Unverified {
        meta.verification_state = VerificationState::StructurallyValid;
    }

    Ok(())
}

/// Remote quarantined artifacts cannot be treated as trusted inputs.
pub fn assert_artifact_usable(meta: &ArtifactMetadata) -> Result<(), ArtifactError> {
    if meta.origin == ArtifactOrigin::Remote
        && matches!(
            meta.verification_state,
            VerificationState::Unverified | VerificationState::FailedVerification
        )
    {
        return Err(ArtifactError::StillQuarantined);
    }
    if meta.origin == ArtifactOrigin::Remote
        && meta.verification_state == VerificationState::StructurallyValid
    {
        // Structural validity alone is insufficient for apply/execute/memory/capability.
        return Err(ArtifactError::StillQuarantined);
    }
    Ok(())
}

pub fn is_safe_workspace_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    if path
        .chars()
        .any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let normalized = path.replace('\\', "/");
    if normalized.contains("://") {
        return false;
    }
    for part in normalized.split('/') {
        // Portable path contract: reject Windows aliases even on Unix so a
        // remotely validated artifact cannot change meaning on its recipient.
        if part.ends_with(['.', ' ']) {
            return false;
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) {
            return false;
        }
        if part == ".." || part == "." {
            return false;
        }
        if part.is_empty() {
            return false;
        }
    }
    true
}

pub fn looks_executable(bytes: &[u8], declared_path: Option<&str>) -> bool {
    if bytes.starts_with(b"#!")
        || bytes.starts_with(b"MZ")
        || bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(b"\xca\xfe\xba\xbe")
        || bytes.starts_with(b"\xcf\xfa\xed\xfe")
        || bytes.starts_with(b"\xce\xfa\xed\xfe")
        || bytes.starts_with(b"\xfe\xed\xfa\xce")
        || bytes.starts_with(b"\xfe\xed\xfa\xcf")
        || bytes.starts_with(b"\xbe\xba\xfe\xca")
        || bytes.starts_with(b"\xca\xfe\xba\xbf")
        || bytes.starts_with(b"\xbf\xba\xfe\xca")
    {
        return true;
    }
    if let Some(path) = declared_path {
        let lower = path.to_ascii_lowercase();
        return lower.ends_with(".exe")
            || lower.ends_with(".dll")
            || lower.ends_with(".so")
            || lower.ends_with(".dylib")
            || lower.ends_with(".bat")
            || lower.ends_with(".cmd")
            || lower.ends_with(".ps1")
            || lower.ends_with(".sh")
            || [
                ".vbs", ".vbe", ".wsf", ".wsh", ".hta", ".com", ".scr", ".cpl", ".pif", ".msi",
                ".msp", ".js", ".jse", ".psm1", ".psd1", ".lnk", ".url",
            ]
            .iter()
            .any(|ext| lower.ends_with(ext));
    }
    false
}

/// Inspect locally available bytes. Archive extraction is not supported by this
/// boundary: reject recognizable containers rather than trusting worker-supplied
/// expansion counts. Any future extractor must enforce limits while decoding.
pub fn inspect_remote_bytes(
    bytes: &[u8],
    declared_paths: &[String],
) -> Result<QuarantineInspection, ArtifactError> {
    let archive_magic = [
        &b"PK\x03\x04"[..],
        &b"PK\x05\x06"[..],
        &b"PK\x07\x08"[..],
        &b"\x1f\x8b"[..],
        &b"BZh"[..],
        &b"\xfd7zXZ\x00"[..],
        &b"7z\xbc\xaf\x27\x1c"[..],
        &b"Rar!"[..],
        &b"\x28\xb5\x2f\xfd"[..],
        &b"MSCF"[..],
    ]
    .iter()
    .any(|magic| bytes.starts_with(magic))
        || bytes.get(257..262) == Some(b"ustar");
    let archive_path = declared_paths.iter().any(|path| {
        let lower = path.to_ascii_lowercase();
        [
            ".zip", ".tar", ".gz", ".tgz", ".bz2", ".tbz2", ".xz", ".txz", ".7z", ".rar", ".zst",
            ".cab", ".jar", ".war", ".whl",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
    });
    if archive_magic || archive_path {
        return Err(ArtifactError::QuarantineRejected(
            "archive payload requires bounded local extraction; unsupported by quarantine".into(),
        ));
    }
    Ok(QuarantineInspection {
        declared_paths: declared_paths.to_vec(),
        executable_content: looks_executable(bytes, None)
            || declared_paths
                .iter()
                .any(|path| looks_executable(bytes, Some(path))),
        ..QuarantineInspection::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use lokai_domain::artifact::{ArtifactKind, ArtifactState, RetentionPolicy};
    use lokai_domain::classify::DataClass;
    use lokai_domain::ids::{ArtifactId, AttemptId, RunId, TaskId};

    #[test]
    fn local_inspection_rejects_archives_without_manifest_metrics() {
        for bytes in [&b"PK\x03\x04bomb"[..], &b"\x1f\x8bbomb"[..]] {
            assert!(inspect_remote_bytes(bytes, &["innocent.txt".into()]).is_err());
        }
        assert!(inspect_remote_bytes(b"data", &["payload.TAR".into()]).is_err());
        assert!(inspect_remote_bytes(b"ordinary text", &["notes.txt".into()]).is_ok());
    }

    #[test]
    fn portable_executable_detection() {
        for ext in [
            "VBS", "vbe", "wsf", "wsh", "hta", "com", "scr", "cpl", "pif", "msi", "msp",
        ] {
            assert!(looks_executable(b"data", Some(&format!("payload.{ext}"))));
        }
        for magic in [
            b"\xce\xfa\xed\xfe",
            b"\xfe\xed\xfa\xce",
            b"\xfe\xed\xfa\xcf",
        ] {
            assert!(looks_executable(magic, None));
        }
        assert!(!looks_executable(b"documentation", Some("README.md")));
    }

    #[test]
    fn rejects_windows_path_aliases_on_every_platform() {
        for path in [
            "C:payload",
            "D:/payload",
            "file.rs:stream",
            "CON",
            "aux.txt",
            "dir/LPT1.log",
            "COM¹",
            "dir/file. ",
            "dir/file.",
            "dir/a?b",
            "dir/../file",
        ] {
            assert!(!is_safe_workspace_path(path), "accepted {path}");
        }
        for path in [
            "src/main.rs",
            "src\\main.rs",
            "docs/com10.txt",
            "auxiliary.txt",
        ] {
            assert!(is_safe_workspace_path(path), "rejected {path}");
        }
    }

    fn test_metadata() -> ArtifactMetadata {
        ArtifactMetadata {
            artifact_id: ArtifactId::new("art_1"),
            kind: ArtifactKind::FileSnapshot,
            schema_version: 1,
            producer_run_id: RunId::new("run_1"),
            producer_task_id: TaskId::new("task_1"),
            producer_attempt_id: AttemptId::new("attempt_1"),
            worker_id: None,
            content_digest: ContentDigest::new("d1"),
            size_bytes: 100,
            workspace_version: None,
            data_class: DataClass::Secret,
            storage_location: lokai_domain::artifact::ArtifactLocation::Remote(
                "http://test".into(),
            ),
            lifecycle_state: ArtifactState::Sealed,
            verification_state: VerificationState::Unverified,
            origin: ArtifactOrigin::Local,
            created_at: Utc::now(),
            retention_policy: RetentionPolicy::Ephemeral,
        }
    }

    #[test]
    fn test_quarantine_validation_success() {
        let mut meta = test_metadata();
        let res = validate_remote_artifact(&mut meta, &ContentDigest::new("d1"));
        assert!(res.is_ok());
        assert_eq!(
            meta.verification_state,
            VerificationState::StructurallyValid
        );
        assert_eq!(meta.origin, ArtifactOrigin::Remote);
        assert!(assert_artifact_usable(&meta).is_err());
    }

    #[test]
    fn test_quarantine_validation_digest_mismatch() {
        let mut meta = test_metadata();
        let res = validate_remote_artifact(&mut meta, &ContentDigest::new("d2"));
        assert!(matches!(res, Err(ArtifactError::DigestMismatch { .. })));
        assert_eq!(meta.verification_state, VerificationState::Unverified);
    }

    #[test]
    fn test_unknown_artifact_schema() {
        let mut meta = test_metadata();
        meta.schema_version = 999;
        let res = validate_remote_artifact(&mut meta, &ContentDigest::new("d1"));
        assert!(matches!(res, Err(ArtifactError::InvalidStateTransition(_))));
    }

    #[test]
    fn path_traversal_rejected() {
        let mut meta = test_metadata();
        let inspection = QuarantineInspection {
            declared_paths: vec!["../etc/passwd".into()],
            ..Default::default()
        };
        let err = validate_remote_artifact_with(
            &mut meta,
            &ContentDigest::new("d1"),
            &QuarantineLimits::default(),
            &inspection,
        )
        .unwrap_err();
        assert!(matches!(err, ArtifactError::QuarantineRejected(_)));
    }

    #[test]
    fn archive_bomb_rejected() {
        let mut meta = test_metadata();
        meta.size_bytes = 100;
        let inspection = QuarantineInspection {
            archive_entries: Some(50_000),
            archive_uncompressed_bytes: Some(50 * 1024 * 1024),
            ..Default::default()
        };
        let err = validate_remote_artifact_with(
            &mut meta,
            &ContentDigest::new("d1"),
            &QuarantineLimits::default(),
            &inspection,
        )
        .unwrap_err();
        assert!(matches!(err, ArtifactError::QuarantineRejected(_)));
    }

    #[test]
    fn executable_payload_rejected() {
        let mut meta = test_metadata();
        let inspection = QuarantineInspection {
            executable_content: true,
            declared_paths: vec!["tool.exe".into()],
            ..Default::default()
        };
        let err = validate_remote_artifact_with(
            &mut meta,
            &ContentDigest::new("d1"),
            &QuarantineLimits::default(),
            &inspection,
        )
        .unwrap_err();
        assert!(matches!(err, ArtifactError::QuarantineRejected(_)));
    }
}
