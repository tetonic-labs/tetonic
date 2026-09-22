//! Task router (D11) — heuristic specialist selection; optional LLM override (v4).

use crate::specialist::{RoleId, SpecialistPack};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSource {
    Keyword,
    FileHint,
    Llm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteMode {
    /// Root agent, full tool set (default on low-capacity / ambiguous tasks).
    Single,
    Specialist(RoleId),
}

#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub mode: RouteMode,
    pub reason: String,
    /// Use the session's hard model tier for this turn (refactor / large scope).
    pub use_hard_tier: bool,
    pub source: RouteSource,
}

impl RouteDecision {
    pub fn single(reason: impl Into<String>) -> Self {
        Self {
            mode: RouteMode::Single,
            reason: reason.into(),
            use_hard_tier: false,
            source: RouteSource::Keyword,
        }
    }

    fn keyword(mode: RouteMode, reason: impl Into<String>, use_hard_tier: bool) -> Self {
        Self {
            mode,
            reason: reason.into(),
            use_hard_tier,
            source: RouteSource::Keyword,
        }
    }
}

#[derive(Clone)]
pub struct RouteContext<'a> {
    pub workspace_root: &'a std::path::Path,
    pub index_db: Option<&'a std::path::Path>,
    pub code_index: Option<&'a dyn lokai_domain::CodeIndexOpen>,
    pub pack: &'a dyn SpecialistPack,
}

fn pack_role(ctx: &RouteContext<'_>, name: &str) -> RouteMode {
    RouteMode::Specialist(
        ctx.pack
            .parse(name)
            .unwrap_or_else(|| ctx.pack.default_role()),
    )
}

/// Resolve routing: optional LLM override, then keyword v3, with session hard-tier preference.
pub fn resolve_route(
    user_input: &str,
    orchestration_auto: bool,
    ctx: RouteContext<'_>,
    llm_override: Option<RouteDecision>,
    session_prefers_hard: bool,
) -> RouteDecision {
    let mut decision = llm_override
        .unwrap_or_else(|| route_task_with_context(user_input, orchestration_auto, ctx));
    if session_prefers_hard {
        decision.use_hard_tier = true;
    }
    decision
}

/// Classify a user turn into a routing decision (v3: path hints + hard tier).
pub fn route_task(
    user_input: &str,
    orchestration_auto: bool,
    pack: &dyn SpecialistPack,
) -> RouteDecision {
    route_task_with_context(
        user_input,
        orchestration_auto,
        RouteContext {
            workspace_root: std::path::Path::new("."),
            index_db: None,
            code_index: None,
            pack,
        },
    )
}

pub fn route_task_with_context(
    user_input: &str,
    orchestration_auto: bool,
    ctx: RouteContext<'_>,
) -> RouteDecision {
    if !orchestration_auto {
        return RouteDecision::single("orchestration disabled");
    }

    let t = user_input.to_ascii_lowercase();
    let use_hard_tier = is_hard_tier_task(&t);

    if is_explain_only(&t) {
        return RouteDecision::keyword(
            pack_role(&ctx, "planner"),
            "explain / read-only — planner specialist",
            use_hard_tier,
        );
    }
    if is_planning(&t) {
        return RouteDecision::keyword(
            pack_role(&ctx, "planner"),
            "planning / design keywords",
            use_hard_tier,
        );
    }
    if is_review(&t) {
        return RouteDecision::keyword(
            pack_role(&ctx, "reviewer"),
            "review / audit keywords",
            use_hard_tier,
        );
    }
    if is_debug(&t) {
        return RouteDecision::keyword(
            pack_role(&ctx, "debugger"),
            "debug / failure keywords",
            use_hard_tier,
        );
    }
    if is_implementation(&t) {
        return RouteDecision::keyword(
            pack_role(&ctx, "coder"),
            "implementation keywords",
            use_hard_tier,
        );
    }

    if let Some(hint) = file_path_routing_hint(user_input, ctx) {
        return hint;
    }

    RouteDecision::keyword(
        RouteMode::Single,
        "no specialist signal — using root agent",
        use_hard_tier,
    )
}

pub fn is_hard_tier_task(t: &str) -> bool {
    [
        "refactor",
        "migrate",
        "multiple files",
        "across the codebase",
        "whole module",
        "entire ",
    ]
    .iter()
    .any(|k| t.contains(k))
}

/// Boost to coder when the user names an existing workspace file.
fn file_path_routing_hint(user_input: &str, ctx: RouteContext<'_>) -> Option<RouteDecision> {
    for token in extract_path_tokens(user_input) {
        let path = ctx.workspace_root.join(&token);
        let t = user_input.to_ascii_lowercase();
        if path.is_file()
            && (is_implementation(&t) || is_debug(&t) || t.contains("in ") || t.contains("file "))
        {
            return Some(RouteDecision {
                mode: pack_role(&ctx, "coder"),
                reason: format!("file path hint: {token}"),
                use_hard_tier: is_hard_tier_task(&t),
                source: RouteSource::FileHint,
            });
        }
        if let (Some(opener), Some(db)) = (ctx.code_index, ctx.index_db) {
            if index_mentions_path(opener, db, ctx.workspace_root, &token) {
                return Some(RouteDecision {
                    mode: pack_role(&ctx, "coder"),
                    reason: format!("indexed file hint: {token}"),
                    use_hard_tier: is_hard_tier_task(&t),
                    source: RouteSource::FileHint,
                });
            }
        }
    }
    None
}

fn extract_path_tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in input.split_whitespace() {
        let w = word.trim_matches(|c: char| ",;:'\"()[]{}".contains(c));
        if w.contains('.') && w.chars().any(|c| c.is_alphabetic()) {
            out.push(w.to_string());
        }
    }
    out
}

/// True when the index knows about a path token in this workspace.
pub(crate) fn index_mentions_path(
    opener: &dyn lokai_domain::CodeIndexOpen,
    db: &std::path::Path,
    workspace_root: &std::path::Path,
    token: &str,
) -> bool {
    let Ok(index) = opener.open(db) else {
        return false;
    };
    let index: Box<dyn lokai_domain::CodeIndex> = index;
    let ws = index.workspace_key(workspace_root);
    let name = token.rsplit('/').next().unwrap_or(token);
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    index
        .find_definition_in(&ws, stem, None)
        .ok()
        .is_some_and(|v| !v.is_empty())
        || index
            .find_definition_in(&ws, name, None)
            .ok()
            .is_some_and(|v| !v.is_empty())
        || index
            .search(&ws, name, 3)
            .ok()
            .is_some_and(|v| !v.is_empty())
        || index
            .search(&ws, stem, 3)
            .ok()
            .is_some_and(|v| !v.is_empty())
}

fn is_explain_only(t: &str) -> bool {
    let asks = [
        "explain ",
        "what does ",
        "what is ",
        "what are ",
        "how does ",
        "how do ",
        "describe ",
        "walk me through",
        "tell me about",
        "tell me how",
        "summary of",
        "summarize ",
    ];
    let read_only = [
        "read only",
        "read-only",
        "do not edit",
        "don't edit",
        "dont edit",
        "without editing",
        "no edits",
    ];
    (asks.iter().any(|k| t.contains(k)) || read_only.iter().any(|k| t.contains(k)))
        && !is_implementation(t)
        && !is_debug(t)
        && !is_review(t)
}

fn is_planning(t: &str) -> bool {
    [
        "plan ",
        "design ",
        "architect",
        "how should we",
        "break down",
        "outline the steps",
        "strategy for",
        "roadmap",
        "milestones",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn is_review(t: &str) -> bool {
    [
        "review ",
        "audit ",
        "check my ",
        "look over",
        "critique",
        "code review",
        "security review",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn is_debug(t: &str) -> bool {
    [
        "debug",
        "traceback",
        "stack trace",
        "failing test",
        "test fail",
        "error:",
        "fix the bug",
        "why does",
        "doesn't work",
        "does not work",
        "panic",
        "segfault",
        "broken",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn is_implementation(t: &str) -> bool {
    [
        "implement",
        "add ",
        "create ",
        "write ",
        "refactor",
        "update ",
        "change ",
        "fix ",
        "build ",
        "migrate",
        "rename ",
        "move ",
    ]
    .iter()
    .any(|k| t.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{RouteMode, RouteSource};
    use crate::specialist::{RoleId, TestCodingPack};

    fn rt(input: &str, auto: bool) -> RouteDecision {
        route_task(input, auto, &TestCodingPack)
    }

    fn spec(name: &str) -> RouteMode {
        RouteMode::Specialist(RoleId::new(name))
    }

    #[test]
    fn routes_implementation_to_coder() {
        let d = rt("Implement slugify in utils.py", true);
        assert_eq!(d.mode, spec("coder"));
    }

    #[test]
    fn routes_plan_to_planner() {
        let d = rt("Plan the refactor for the auth module", true);
        assert_eq!(d.mode, spec("planner"));
    }

    #[test]
    fn single_when_disabled() {
        let d = rt("Implement foo", false);
        assert_eq!(d.mode, RouteMode::Single);
    }

    #[test]
    fn routes_debug_and_review() {
        let d = rt("debug the failing test", true);
        assert_eq!(d.mode, spec("debugger"));
        let d = rt("review my changes", true);
        assert_eq!(d.mode, spec("reviewer"));
    }

    #[test]
    fn explain_only_routes_planner() {
        let d = rt("Explain what main.rs does", true);
        assert_eq!(d.mode, spec("planner"));
    }

    #[test]
    fn hard_tier_on_large_scope_keywords() {
        let d = rt("Refactor auth across the codebase", true);
        assert!(d.use_hard_tier);
        assert_eq!(d.mode, spec("coder"));
        assert_eq!(d.source, RouteSource::Keyword);
    }

    #[test]
    fn session_prefers_hard_via_resolve() {
        let d = resolve_route(
            "Explain main.rs",
            true,
            RouteContext {
                workspace_root: std::path::Path::new("."),
                index_db: None,
                code_index: None,
                pack: &TestCodingPack,
            },
            None,
            true,
        );
        assert!(d.use_hard_tier);
    }

    #[test]
    fn file_path_hint_routes_to_coder() {
        let dir = std::env::temp_dir().join(format!("lokai-router-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("foo.rs"), "fn main() {}").unwrap();
        let d = route_task_with_context(
            "Issues in foo.rs",
            true,
            RouteContext {
                workspace_root: &dir,
                index_db: None,
                code_index: None,
                pack: &TestCodingPack,
            },
        );
        assert_eq!(d.mode, spec("coder"));
        assert!(d.reason.contains("foo.rs"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn indexed_file_hint_uses_storage_key() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        std::fs::write(ws.join("widget.rs"), "pub fn widget() {}\n").unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = db_dir.path().join("index.db");
        let index = lokai_index::Index::open(&db).unwrap();
        index.index_workspace(ws).unwrap();
        std::fs::remove_file(ws.join("widget.rs")).unwrap();
        let opener = lokai_index::FilesystemCodeIndex;
        let key = lokai_index::workspace_storage_key(ws);
        assert!(
            index_mentions_path(&opener, db.as_path(), ws, "widget.rs"),
            "index should resolve symbol via storage key"
        );
        let decision = route_task_with_context(
            "widget.rs context",
            true,
            RouteContext {
                workspace_root: ws,
                index_db: Some(db.as_path()),
                code_index: Some(&opener),
                pack: &TestCodingPack,
            },
        );
        assert_eq!(decision.mode, spec("coder"));
        assert_eq!(decision.source, RouteSource::FileHint);
        assert!(decision.reason.contains("indexed file hint"));
        assert!(index.status(&key).unwrap().files >= 1);
    }
}
