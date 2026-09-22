//! Compare a session `--model` to a saved capacity profile.

/// Stem used to compare tags like `qwen3.5:latest` and `qwen3.5-estate`.
pub fn canonical_model_stem(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    let base = lower.split(':').next().unwrap_or(&lower);
    base.trim_end_matches("-estate").to_string()
}

pub fn same_model_family(a: &str, b: &str) -> bool {
    let a = canonical_model_stem(a);
    let b = canonical_model_stem(b);
    !a.is_empty() && a == b
}

/// True when this chat's model is the profile default (estate tag or base tag).
pub fn session_matches_profile(
    session_model: &str,
    profile_model: Option<&str>,
    profile_base: Option<&str>,
) -> bool {
    profile_model.is_some_and(|m| same_model_family(session_model, m))
        || profile_base.is_some_and(|m| same_model_family(session_model, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_ignore_latest_and_estate() {
        assert_eq!(canonical_model_stem("qwen3.5:latest"), "qwen3.5");
        assert_eq!(canonical_model_stem("qwen3.5-estate"), "qwen3.5");
        assert_ne!(
            canonical_model_stem("qwen3.5:latest"),
            canonical_model_stem("qwen3.6:latest")
        );
    }

    #[test]
    fn session_override_is_not_the_profile() {
        assert!(!session_matches_profile(
            "qwen3.5:latest",
            Some("qwen3.6-estate"),
            Some("qwen3.6:latest"),
        ));
        assert!(session_matches_profile(
            "qwen3.6:latest",
            Some("qwen3.6-estate"),
            Some("qwen3.6:latest"),
        ));
    }
}
