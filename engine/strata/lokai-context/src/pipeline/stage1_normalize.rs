use crate::types::ContextRequest;

/// The output of objective normalization — structured queries for the retrieval stage.
#[derive(Debug, Clone)]
pub struct NormalizedObjective {
    /// The raw objective text, passed through as the primary query.
    pub query: String,
    /// Individual tokens that look like file paths (contain `/`, `\`, or `.rs`/`.ts` etc.).
    pub named_paths: Vec<String>,
    /// Tokens that look like symbol names (CamelCase or snake_case identifiers).
    pub symbols: Vec<String>,
    /// Error message substrings detected (e.g. lines starting with "error[" or "FAILED").
    pub error_snippets: Vec<String>,
}

/// Deterministic objective normalization (Stage 1).
///
/// Extracts structured signals from the raw task objective without any LLM involvement.
/// LLM-assisted rewriting may be added later behind a feature flag.
pub fn normalize(request: &ContextRequest) -> Result<NormalizedObjective, String> {
    let text = &request.objective;

    // --- Named paths ---
    // Words containing path separators or common source extensions.
    let path_extensions = &[
        ".rs", ".ts", ".js", ".py", ".go", ".java", ".kt", ".cpp", ".c", ".h", ".toml", ".json",
        ".yaml", ".yml", ".md",
    ];
    let named_paths: Vec<String> = text
        .split_whitespace()
        .filter(|tok| {
            tok.contains('/')
                || tok.contains('\\')
                || path_extensions.iter().any(|ext| tok.ends_with(ext))
        })
        .map(|s| {
            s.trim_matches(|c: char| {
                !c.is_alphanumeric() && c != '.' && c != '/' && c != '_' && c != '-'
            })
            .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();

    // --- Symbols ---
    // Words that look like identifiers: start with a letter, contain only alphanumeric / `_`,
    // and are either CamelCase (contains an uppercase non-first char) or snake_case (contains `_`).
    let symbols: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|tok| {
            if tok.len() < 3 {
                return false;
            }
            let first = tok.chars().next().unwrap_or(' ');
            if !first.is_alphabetic() {
                return false;
            }
            let has_upper_after_first = tok.chars().skip(1).any(|c| c.is_uppercase());
            let has_underscore = tok.contains('_');
            has_upper_after_first || has_underscore
        })
        .map(|s| s.to_string())
        .collect();

    // --- Error snippets ---
    // Lines that match common compiler/test failure patterns.
    let error_snippets: Vec<String> = text
        .lines()
        .filter(|line| {
            let l = line.trim_start();
            l.starts_with("error")
                || l.starts_with("FAILED")
                || l.starts_with("panicked")
                || l.starts_with("thread '")
                || l.contains("error[E")
        })
        .map(|s| s.trim().to_string())
        .collect();

    Ok(NormalizedObjective {
        query: text.clone(),
        named_paths,
        symbols,
        error_snippets,
    })
}
