//! Optional LLM router (D11 v4) — one-line role label with keyword fallback.

use tetonic_inference::{ChatRequest, FabricCallMeta, InferenceProvider, Message};

use crate::router::{route_task_with_context, RouteContext, RouteDecision, RouteMode, RouteSource};
use crate::specialist::SpecialistPack;

const ROUTER_PROMPT: &str = "You are a task router. Reply with exactly one line:\n\
ROLE: brief reason\n\
ROLE is one of: SINGLE, PLANNER, CODER, DEBUGGER, REVIEWER.\n\
SINGLE = explain/ambiguous; PLANNER = design/plan; CODER = implement/edit; \
DEBUGGER = fix failures; REVIEWER = audit/review only.";

/// Parse a one-line LLM router response into a [`RouteDecision`].
pub fn parse_llm_route_response(
    text: &str,
    user_input: &str,
    pack: &dyn SpecialistPack,
) -> Option<RouteDecision> {
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim();
    let (role_part, reason) = line.split_once(':').unwrap_or((line, ""));
    let role = role_part.trim().to_ascii_uppercase();
    let reason = if reason.trim().is_empty() {
        format!("llm router ({role})")
    } else {
        reason.trim().to_string()
    };
    let t = user_input.to_ascii_lowercase();
    let use_hard_tier = crate::router::is_hard_tier_task(&t);
    let mode = match role.as_str() {
        "SINGLE" | "ROOT" => RouteMode::Single,
        "PLANNER" | "PLAN" => {
            RouteMode::Specialist(pack.parse("planner").unwrap_or_else(|| pack.default_role()))
        }
        "CODER" | "CODE" | "IMPLEMENT" => {
            RouteMode::Specialist(pack.parse("coder").unwrap_or_else(|| pack.default_role()))
        }
        "DEBUGGER" | "DEBUG" => RouteMode::Specialist(
            pack.parse("debugger")
                .unwrap_or_else(|| pack.default_role()),
        ),
        "REVIEWER" | "REVIEW" | "CRITIC" => RouteMode::Specialist(
            pack.parse("reviewer")
                .unwrap_or_else(|| pack.default_role()),
        ),
        _ => return None,
    };
    Some(RouteDecision {
        mode,
        reason,
        use_hard_tier,
        source: RouteSource::Llm,
    })
}

/// Ask the inference provider to classify the turn; fall back to keyword router on failure.
pub async fn llm_route_task(
    provider: &dyn InferenceProvider,
    router_model: &str,
    user_input: &str,
    orchestration_auto: bool,
    ctx: RouteContext<'_>,
    fabric: FabricCallMeta,
    inference_allowed: impl Fn() -> bool,
) -> RouteDecision {
    let pack = ctx.pack;
    let keyword = route_task_with_context(user_input, orchestration_auto, ctx);
    if !orchestration_auto {
        return keyword;
    }

    let req = ChatRequest {
        model: router_model.to_string(),
        model_digest: None,
        messages: vec![Message::system(ROUTER_PROMPT), Message::user(user_input)],
        tools: vec![],
        temperature: 0.0,
        num_ctx: Some(2048),
        keep_alive: None,
        fabric: Some(fabric),
        response_format: None,
        outbound_scan: Default::default(),
        ..Default::default()
    };

    if !inference_allowed() {
        return keyword;
    }
    let mut sink = |_: &str| {};
    match provider.chat(req, &mut sink).await {
        Ok(resp) => {
            parse_llm_route_response(&resp.message.content, user_input, pack).unwrap_or(keyword)
        }
        Err(_) => keyword,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl InferenceProvider for CountingProvider {
        async fn chat(
            &self,
            _: ChatRequest,
            _: &mut tetonic_inference::TokenSink<'_>,
        ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(tetonic_inference::ChatResponse {
                message: Message::assistant("CODER: should not run"),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn llm_route_task_does_not_call_the_provider_when_execution_is_denied() {
        let calls = Arc::new(AtomicUsize::new(0));
        let decision = llm_route_task(
            &CountingProvider {
                calls: calls.clone(),
            },
            "router-model",
            "implement the handler",
            true,
            RouteContext {
                workspace_root: std::path::Path::new("."),
                index_db: None,
                code_index: None,
                pack: &crate::specialist::TestCodingPack,
            },
            FabricCallMeta::default(),
            || false,
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(decision.source, RouteSource::Keyword);
    }

    use crate::router::RouteMode;
    use crate::specialist::{RoleId, TestCodingPack};

    fn spec(name: &str) -> RouteMode {
        RouteMode::Specialist(RoleId::new(name))
    }

    #[test]
    fn parses_llm_router_lines() {
        let d = parse_llm_route_response(
            "CODER: implement auth module",
            "implement auth",
            &TestCodingPack,
        )
        .unwrap();
        assert_eq!(d.mode, spec("coder"));
        assert_eq!(d.source, RouteSource::Llm);

        let d = parse_llm_route_response(
            "SINGLE: explain-only question",
            "what is this",
            &TestCodingPack,
        )
        .unwrap();
        assert_eq!(d.mode, RouteMode::Single);
    }

    #[test]
    fn rejects_unknown_role() {
        assert!(parse_llm_route_response("WIZARD: magic", "do magic", &TestCodingPack).is_none());
    }

    /// Eval fixtures for router v5 regression (keyword fallback parses same lines).
    #[test]
    fn router_eval_fixtures() {
        let cases = [
            (
                "SINGLE: explain-only",
                "what is this crate",
                RouteMode::Single,
            ),
            (
                "PLANNER: design work",
                "plan the migration",
                spec("planner"),
            ),
            (
                "CODER: implement feature",
                "implement the handler",
                spec("coder"),
            ),
            (
                "DEBUGGER: fix test failure",
                "fix the failing test",
                spec("debugger"),
            ),
            (
                "REVIEWER: audit diff",
                "review my changes",
                spec("reviewer"),
            ),
        ];
        for (line, user, expected_mode) in cases {
            let d = parse_llm_route_response(line, user, &TestCodingPack)
                .unwrap_or_else(|| panic!("failed to parse router line: {line}"));
            assert_eq!(d.mode, expected_mode, "line={line}");
            assert_eq!(d.source, RouteSource::Llm);
        }
    }
}
