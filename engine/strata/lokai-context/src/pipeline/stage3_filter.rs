use crate::types::{ContextEvidence, ContextOmission, PathPolicy};
use lokai_domain::classify::DataClass as DomainDataClass;

/// Stage 3: Exclusion and classification filtering.
///
/// Enforces:
/// - Allowed path policy (if non-empty, path must match at least one path-component prefix)
/// - Excluded path policy (path must not match any path-component prefix)
/// - Data-class ceiling (evidence with a higher class than the request ceiling is rejected)
///
/// Every rejected candidate produces a `ContextOmission` record.
/// Secret content and ignored content are tracked separately.
pub fn filter(
    candidates: Vec<ContextEvidence>,
    data_class_ceiling: &DomainDataClass,
    allowed_paths: &PathPolicy,
    excluded_paths: &PathPolicy,
) -> Result<(Vec<ContextEvidence>, Vec<ContextOmission>), String> {
    let mut accepted = Vec::new();
    let mut omissions = Vec::new();

    for ev in candidates {
        // --- Data-class ceiling check ---
        // Classes are ordered: Public < Internal < RepositorySource < Confidential < Secret
        if class_exceeds_ceiling(&ev.data_class, data_class_ceiling) {
            omissions.push(ContextOmission {
                reason: format!(
                    "data-class {:?} exceeds ceiling {:?}",
                    ev.data_class, data_class_ceiling
                ),
                path: ev.repository_path.clone(),
            });
            continue;
        }

        if let Some(path) = ev.repository_path.as_deref() {
            if !path_is_allowed(path, allowed_paths, excluded_paths)? {
                omissions.push(ContextOmission {
                    reason: "path is outside the permitted scope".into(),
                    path: ev.repository_path.clone(),
                });
                continue;
            }
        } else if !allowed_paths.rules.is_empty() || !excluded_paths.rules.is_empty() {
            // Pathless evidence cannot establish membership in a restricted scope.
            omissions.push(ContextOmission {
                reason: "pathless evidence cannot satisfy path restrictions".into(),
                path: None,
            });
            continue;
        }

        accepted.push(ev);
    }

    Ok((accepted, omissions))
}

/// Returns true if `class` is *more sensitive* than `ceiling`, meaning it should be filtered out.
/// Ordering: Public < Internal < RepositorySource < Confidential < Secret
pub fn class_exceeds_ceiling(class: &DomainDataClass, ceiling: &DomainDataClass) -> bool {
    class_rank(class) > class_rank(ceiling)
}

/// Public alias for use by expansion handle enforcement in pipeline/mod.rs.
pub fn class_exceeds_ceiling_pub(class: &DomainDataClass, ceiling: &DomainDataClass) -> bool {
    class_exceeds_ceiling(class, ceiling)
}

fn class_rank(class: &DomainDataClass) -> u8 {
    match class {
        DomainDataClass::Public => 0,
        DomainDataClass::RepositorySource => 1,
        DomainDataClass::SensitiveSource => 2,
        DomainDataClass::Secret => 3,
    }
}

/// Lexical repository-relative policy, not a filesystem/symlink authority.
pub(crate) fn path_is_allowed(
    path: &str,
    allowed: &PathPolicy,
    excluded: &PathPolicy,
) -> Result<bool, String> {
    let normalize_rules = |policy: &PathPolicy| -> Result<Vec<String>, String> {
        policy
            .rules
            .iter()
            .map(|rule| {
                normalize_path(rule)
                    .ok_or_else(|| "invalid repository-relative path policy".to_string())
            })
            .collect()
    };
    let allowed = normalize_rules(allowed)?;
    let excluded = normalize_rules(excluded)?;
    let Some(path) = normalize_path(path) else {
        return Ok(false);
    };
    let matches = |rule: &String| {
        rule.is_empty()
            || path == *rule
            || path
                .strip_prefix(rule)
                .is_some_and(|tail| tail.starts_with('/'))
    };
    Ok(!excluded.iter().any(matches) && (allowed.is_empty() || allowed.iter().any(matches)))
}

fn normalize_path(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    if path.is_empty() || path.starts_with('/') || path.contains(':') || path.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part == ".." || (part != "." && (part.ends_with('.') || part.ends_with(' '))) {
            return None;
        }
        if !part.is_empty() && part != "." {
            parts.push(part);
        }
    }
    let path = parts.join("/");
    Some(if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    })
}
