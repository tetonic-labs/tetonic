//! Runtime invocation and candidate-outcome contracts (SUB-01 / SUB-04).
//!
//! Neutral loop consume/return types. `LoopDiscipline` is a product-supplied
//! table (empty `Default`). It is not coding, Session, or repository fields.
//! Not durable. Not an `ExecutionOutcome` alias.

/// Product-compiled inputs for one agent-loop turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInvocation {
    /// Resolved instruction text. The only instruction source the loop may use.
    pub instructions: String,
    pub user_input: String,
    /// Compiled read-only / explain mode, including Single / `role: None`.
    pub explain_turn: bool,
    /// Compiled empty-tool nudge eligibility. Current heuristic, not PRESERVE.
    pub empty_tool_nudge: bool,
    pub max_steps: usize,
    /// Product-supplied completion tool name (coding product sets `"finish"`).
    pub completion_tool: String,
    /// Product-supplied loop tables. Empty default is no coding law.
    pub discipline: LoopDiscipline,
}

/// Product-supplied loop tables. Empty `Default` has no tool names or copy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoopDiscipline {
    pub finish_min_chars: Option<usize>,
    pub empty_tool_nudge_text: Option<String>,
    pub spawn_tool: Option<String>,
    pub expand_tool: Option<String>,
    pub whole_file_tools: Vec<String>,
    pub search_tools: Vec<String>,
    pub compaction_system_prompt: Option<String>,
    pub notes: LoopNotes,
    pub limits: LoopDisciplineLimits,
}

/// Product-supplied feedback strings. Empty default is silent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoopNotes {
    pub finish_too_short: Option<String>,
    pub mutate_feedback: Option<String>,
    pub reread_feedback: Option<String>,
    pub size_feedback: Option<String>,
    pub retrieval_nudge: Option<String>,
    pub search_miss_nudge: Option<String>,
    pub write_repeat_feedback: Option<String>,
    pub explain_cap_nudge: Option<String>,
    pub explain_last_nudge: Option<String>,
    pub spawn_disabled: Option<String>,
    pub expand_failed: Option<String>,
    pub expand_disabled: Option<String>,
}

/// Product-supplied numeric limits. Empty default applies no extra gates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoopDisciplineLimits {
    pub max_whole_file_bytes: Option<usize>,
    pub search_miss_streak: Option<u32>,
    pub write_repeat: Option<u32>,
}

impl LoopDiscipline {
    pub fn is_whole_file_tool(&self, name: &str) -> bool {
        self.whole_file_tools.iter().any(|t| t == name)
    }

    pub fn is_search_tool(&self, name: &str) -> bool {
        self.search_tools.iter().any(|t| t == name)
    }
}

/// Semantic stop of one loop turn. Not `Result<()>`. `Failed` is an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateOutcome {
    Completed {
        summary: String,
        kind: CompletionKind,
    },
    Canceled {
        reason: String,
    },
    Limited {
        kind: LimitKind,
        message: String,
    },
    Failed {
        message: String,
    },
}

/// How a completed turn answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Answer,
    Finish,
}

/// Why the loop stopped without a completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    EffortCap,
    NoProgress,
    EmptyTools,
}

impl CandidateOutcome {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_constructs_without_session_or_repo_fields() {
        let inv = AgentInvocation {
            instructions: "rules".into(),
            user_input: "do the thing".into(),
            explain_turn: false,
            empty_tool_nudge: true,
            max_steps: 8,
            completion_tool: "finish".into(),
            discipline: LoopDiscipline::default(),
        };
        assert!(!inv.explain_turn);
        assert!(inv.empty_tool_nudge);
        assert_eq!(inv.completion_tool, "finish");
        assert_eq!(inv.max_steps, 8);
        assert!(inv.discipline.finish_min_chars.is_none());
        assert!(inv.discipline.spawn_tool.is_none());
    }

    #[test]
    fn sub04_loop_discipline_default_is_empty() {
        let d = LoopDiscipline::default();
        assert!(d.finish_min_chars.is_none());
        assert!(d.empty_tool_nudge_text.is_none());
        assert!(d.spawn_tool.is_none());
        assert!(d.expand_tool.is_none());
        assert!(d.whole_file_tools.is_empty());
        assert!(d.search_tools.is_empty());
        assert!(d.compaction_system_prompt.is_none());
        assert!(d.notes.finish_too_short.is_none());
        assert!(d.limits.max_whole_file_bytes.is_none());
        assert!(!d.is_whole_file_tool("read_file"));
        assert!(!d.is_search_tool("search_code"));
    }

    #[test]
    fn candidate_outcome_is_exhaustive() {
        fn classify(o: &CandidateOutcome) -> &'static str {
            match o {
                CandidateOutcome::Completed { kind, .. } => match kind {
                    CompletionKind::Answer => "answer",
                    CompletionKind::Finish => "finish",
                },
                CandidateOutcome::Canceled { .. } => "canceled",
                CandidateOutcome::Limited { kind, .. } => match kind {
                    LimitKind::EffortCap => "effort",
                    LimitKind::NoProgress => "progress",
                    LimitKind::EmptyTools => "empty",
                },
                CandidateOutcome::Failed { .. } => "failed",
            }
        }
        assert_eq!(
            classify(&CandidateOutcome::Completed {
                summary: "ok".into(),
                kind: CompletionKind::Answer,
            }),
            "answer"
        );
        assert_eq!(
            classify(&CandidateOutcome::Completed {
                summary: "done".into(),
                kind: CompletionKind::Finish,
            }),
            "finish"
        );
        assert_eq!(
            classify(&CandidateOutcome::Canceled {
                reason: "stop".into(),
            }),
            "canceled"
        );
        assert_eq!(
            classify(&CandidateOutcome::Limited {
                kind: LimitKind::EffortCap,
                message: "cap".into(),
            }),
            "effort"
        );
        assert_eq!(
            classify(&CandidateOutcome::Limited {
                kind: LimitKind::NoProgress,
                message: "stuck".into(),
            }),
            "progress"
        );
        assert_eq!(
            classify(&CandidateOutcome::Limited {
                kind: LimitKind::EmptyTools,
                message: "none".into(),
            }),
            "empty"
        );
        assert_eq!(
            classify(&CandidateOutcome::Failed {
                message: "boom".into(),
            }),
            "failed"
        );
    }
}
