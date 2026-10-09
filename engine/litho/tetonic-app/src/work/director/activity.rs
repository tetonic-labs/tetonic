//! Visible operation receipts, projected from the existing scoped audit transcript.
//! No timer-driven phase guesses and no private model reasoning exposed.
use crate::work::types::GuideActivity;
use std::collections::VecDeque;
use tetonic_memory::TranscriptEntry;

pub(in crate::work) fn project(history: &[TranscriptEntry], active: bool) -> Vec<GuideActivity> {
    let mut result: Vec<GuideActivity> = Vec::new();
    let mut pending = VecDeque::<Option<usize>>::new();
    for entry in history {
        if entry.role == "assistant" {
            // The runtime processes one assistant batch at a time. A new reply
            // means any unexecuted tail of the preceding batch was abandoned.
            for index in pending.drain(..).flatten() {
                result[index].state = "interrupted".into();
            }
            let calls: Vec<serde_json::Value> = entry
                .tool_calls_json
                .as_deref()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_default();
            for (index, call) in calls.iter().enumerate() {
                if call["function"]["name"] != crate::resources::work_director::CONTROL {
                    continue;
                }
                let mut arguments = call["function"]["arguments"].clone();
                if let Some(text) = arguments.as_str() {
                    arguments = serde_json::from_str(text).unwrap_or_default();
                }
                let Some(operation @ ("resources" | "work" | "inspect" | "propose")) =
                    arguments["operation"].as_str()
                else {
                    // Keep malformed/unknown calls in the serial result order
                    // so their failure cannot settle the next visible check.
                    pending.push_back(None);
                    continue;
                };
                pending.push_back(Some(result.len()));
                result.push(GuideActivity {
                    id: format!("{}:{index}", entry.sequence),
                    operation: operation.into(),
                    state: "requested".into(),
                });
            }
        } else if entry.role == "tool"
            && entry.tool_name.as_deref() == Some(crate::resources::work_director::CONTROL)
        {
            // The managed hook executes in call order, including providers
            // without call IDs. Never settle a call from an older batch.
            if let Some(Some(index)) = pending.pop_front() {
                result[index].state = match entry.tool_status.as_deref() {
                    Some("ok") => "completed",
                    Some(_) => "failed",
                    None => "unconfirmed",
                }
                .into();
            }
        }
    }
    if !active {
        for activity in &mut result {
            if activity.state == "requested" {
                activity.state = "interrupted".into();
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> TranscriptEntry {
        TranscriptEntry { sequence:1, role:"assistant".into(), content:String::new(), tool_calls_json:Some(serde_json::json!([{"function":{"name":"work_plan","arguments":{"operation":"resources"}}}]).to_string()), tool_name:None, tool_status:None }
    }
    #[test]
    fn operations_are_pending_until_a_durable_tool_result_confirms_them() {
        assert_eq!(project(&[request()], true)[0].state, "requested");
        assert_eq!(project(&[request()], false)[0].state, "interrupted");
        for (status, expected) in [
            (Some("ok"), "completed"),
            (Some("error"), "failed"),
            (None, "unconfirmed"),
        ] {
            let result = TranscriptEntry {
                sequence: 2,
                role: "tool".into(),
                content: "Do not trust prose claiming success".into(),
                tool_calls_json: None,
                tool_name: Some("work_plan".into()),
                tool_status: status.map(str::to_owned),
            };
            assert_eq!(project(&[request(), result], true)[0].state, expected);
        }
    }

    #[test]
    fn skipped_or_unknown_calls_cannot_borrow_another_operations_receipt() {
        let mut first = request();
        first.tool_calls_json = Some(
            serde_json::json!([
                {"function":{"name":"work_plan","arguments":{"operation":"unknown"}}},
                {"function":{"name":"work_plan","arguments":{"operation":"resources"}}}
            ])
            .to_string(),
        );
        let failed = TranscriptEntry {
            sequence: 2,
            role: "tool".into(),
            content: String::new(),
            tool_calls_json: None,
            tool_name: Some("work_plan".into()),
            tool_status: Some("error".into()),
        };
        assert_eq!(project(&[first, failed], true)[0].state, "requested");

        let mut next = request();
        next.sequence = 3;
        let success = TranscriptEntry {
            sequence: 4,
            role: "tool".into(),
            content: String::new(),
            tool_calls_json: None,
            tool_name: Some("work_plan".into()),
            tool_status: Some("ok".into()),
        };
        let activities = project(&[request(), next, success], false);
        assert_eq!(activities[0].state, "interrupted");
        assert_eq!(activities[1].state, "completed");
    }
}
