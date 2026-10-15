//! Shared runtime advertisements, also used when estimating plan overhead.
use super::*;

pub(crate) fn coordinator(keys: &[String]) -> Vec<ToolAdvertisement> {
    let finish = tetonic_tools::tool_defs()
        .into_iter()
        .find(|d| d.name == "finish")
        .expect("completion tool");
    vec![ToolAdvertisement {
        name: "finish".into(),
        description: "Return the final synthesis in summary after all contributions arrive, as the only tool call in this response. Preserve source citations and the requested length.".into(),
        parameters: finish.parameters,
    }, ToolAdvertisement {
        name: DISPATCH.into(),
        description: "Dispatch all outstanding agreed assignment_keys together, including dependents. The host handles parallelism, dependency order and capacity without more model calls. Completed-key retries reuse results.".into(),
        parameters: serde_json::json!({"type":"object","additionalProperties":false,"required":["assignment_keys"],"properties":{"assignment_keys":{"type":"array","minItems":1,"maxItems":tetonic_memory::PlanContent::MAX_ASSIGNMENTS,"uniqueItems":true,"items":{"type":"string","enum":keys}}}}),
    }]
}

pub(crate) fn human() -> ToolAdvertisement {
    ToolAdvertisement {name:ASK_HUMAN.into(),description:"Ask a blocking question within the host's response limit. Answers do not change permissions or budget.".into(),parameters:serde_json::json!({"type":"object","additionalProperties":false,"required":["question","why"],"properties":{"question":{"type":"string","maxLength":800},"why":{"type":"string","maxLength":1200},"options":{"type":"array","maxItems":4,"items":{"type":"string","maxLength":300}}}})}
}
