//! Batch the model's explicit selections through the same single-key dispatcher.
//! No new admissions, dependencies, budgets or completion authority live here.
use super::*;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(untagged)]
enum Selection {
    One(One),
    Many(Many),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct One {
    assignment_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Many {
    assignment_keys: Vec<String>,
}

pub(super) fn keys(args: Value, allowed: &[String]) -> Result<(Vec<String>, bool), String> {
    let selection = serde_json::from_value(args).map_err(|_| {
        "Provide assignment_keys as an array of agreed keys, without extra fields.".to_string()
    })?;
    let (keys, grouped) = match selection {
        Selection::One(one) => (vec![one.assignment_key], false),
        Selection::Many(many) => (many.assignment_keys, true),
    };
    let unique: std::collections::HashSet<_> = keys.iter().collect();
    if keys.is_empty()
        || keys.len() > 12
        || unique.len() != keys.len()
        || keys.iter().any(|key| !allowed.contains(key))
    {
        return Err(format!(
            "Select 1–12 distinct agreed keys in dependency order from: {}.",
            allowed.join(", ")
        ));
    }
    Ok((keys, grouped))
}

pub(super) async fn dispatch(
    dispatch: &PlanDispatch,
    request: tetonic_core::SpawnRequest,
) -> ToolOutcome {
    let Some(attempt) = request.attempt_id else {
        return ToolOutcome::fail("Managed parent required", "denied");
    };
    let (keys, grouped) = match keys(request.arguments, &dispatch.assignment_keys) {
        Ok(selection) if request.tool_name == DISPATCH => selection,
        _ => return ToolOutcome::fail("Choose agreed assignment keys", "bad_args"),
    };
    let mut results = vec![];
    for key in keys {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let sent = dispatch
            .sender
            .send(DispatchCall {
                key: key.clone(),
                attempt: attempt.clone(),
                reply,
            })
            .await;
        let mut outcome = if sent.is_err() {
            ToolOutcome::fail("The plan dispatcher is unavailable", "unavailable")
        } else {
            receiver
                .await
                .unwrap_or_else(|_| ToolOutcome::fail("The plan dispatcher stopped", "unavailable"))
        };
        if !grouped {
            return outcome;
        }
        let payload = serde_json::from_str::<Value>(&outcome.content).ok();
        let completed = outcome.ok && payload.as_ref().is_some_and(|p| p["state"] == "completed");
        results.push(json!({"assignment_key":key,"ok":outcome.ok,"result":payload,"message":outcome.summary}));
        // Do not spend the next allocation when a dependency is blocked or a
        // human is needed. Return prior contributions and pending keys together.
        if !completed {
            if outcome.ok && payload.is_none() {
                outcome =
                    ToolOutcome::fail("The dispatch receipt could not be read", "unavailable");
            }
            return grouped_result(dispatch, results, outcome);
        }
    }
    grouped_result(
        dispatch,
        results,
        ToolOutcome::ok("Requested contributions are ready", ""),
    )
}

fn grouped_result(
    dispatch: &PlanDispatch,
    results: Vec<Value>,
    mut outcome: ToolOutcome,
) -> ToolOutcome {
    let Ok(remaining) = dispatch.remaining.lock() else {
        return ToolOutcome::fail("Cannot confirm outstanding assignments", "unavailable");
    };
    outcome.content = json!({
        "message":outcome.summary,
        "results":results,
        "outstanding_assignments":dispatch.assignment_keys.iter().filter(|key| remaining.contains(*key)).collect::<Vec<_>>()
    }).to_string();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_selection_cannot_smuggle_unknown_duplicate_or_mixed_keys() {
        let allowed = vec!["a".into(), "b".into()];
        assert_eq!(
            keys(json!({"assignment_key":"a"}), &allowed).unwrap(),
            (vec!["a".into()], false)
        );
        assert_eq!(
            keys(json!({"assignment_keys":["a","b"]}), &allowed).unwrap(),
            (allowed.clone(), true)
        );
        for invalid in [
            json!({"assignment_keys":[]}),
            json!({"assignment_keys":["a","a"]}),
            json!({"assignment_keys":["a","unknown"]}),
            json!({"assignment_key":"unknown"}),
            json!({"assignment_key":"a","assignment_keys":["b"]}),
            json!({"assignment_keys":["a"],"budget":999}),
            json!({"assignment_key":null}),
        ] {
            assert!(keys(invalid.clone(), &allowed).is_err(), "{invalid}");
        }
        let too_many: Vec<_> = (0..13).map(|i| i.to_string()).collect();
        assert!(keys(json!({"assignment_keys":too_many}), &too_many).is_err());
    }

    #[tokio::test]
    async fn a_human_wait_or_failure_stops_the_group_and_retains_prior_results() {
        for wait in [true, false] {
            let (sender, mut receiver) = tokio::sync::mpsc::channel::<DispatchCall>(1);
            let keys = vec!["a".to_string(), "b".into(), "c".into()];
            let dispatcher = PlanDispatch {
                human: None,
                binding: "root".into(),
                sender,
                assignment_keys: keys.clone(),
                remaining: Arc::new(Mutex::new(keys.iter().cloned().collect())),
            };
            let request = tetonic_core::SpawnRequest {
                tool_name: DISPATCH.into(),
                call_id: "call".into(),
                arguments: json!({"assignment_keys":keys}),
                attempt_id: Some("parent".into()),
                role: String::new(),
                task: String::new(),
                parent_agent_id: "coordinator".into(),
            };
            let replies = async {
                let first = receiver.recv().await.unwrap();
                assert_eq!(first.key, "a");
                assert_eq!(first.attempt, "parent");
                if wait {
                    first
                        .reply
                        .send(ToolOutcome::ok(
                            "Waiting for owner",
                            json!({"state":"waiting_human","question":"Audience?"}).to_string(),
                        ))
                        .unwrap();
                } else {
                    dispatcher.remaining.lock().unwrap().remove("a");
                    first
                        .reply
                        .send(ToolOutcome::ok(
                            "Ready",
                            json!({"state":"completed","contribution":"KEPT_RESULT"}).to_string(),
                        ))
                        .unwrap();
                    let second = receiver.recv().await.unwrap();
                    assert_eq!(second.key, "b");
                    second
                        .reply
                        .send(ToolOutcome::fail("Dependency unavailable", "blocked"))
                        .unwrap();
                }
            };
            let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::join!(dispatch(&dispatcher, request), replies)
            })
            .await
            .unwrap();
            assert!(
                receiver.try_recv().is_err(),
                "no later key may start after a pause"
            );
            let payload: Value = serde_json::from_str(&result.content).unwrap();
            if wait {
                assert!(result.ok);
                assert_eq!(payload["results"][0]["result"]["state"], "waiting_human");
                assert_eq!(payload["outstanding_assignments"], json!(["a", "b", "c"]));
            } else {
                assert!(!result.ok);
                assert!(payload.to_string().contains("KEPT_RESULT"));
                assert!(payload["message"]
                    .as_str()
                    .unwrap()
                    .contains("Dependency unavailable"));
                assert_eq!(payload["outstanding_assignments"], json!(["b", "c"]));
            }
        }
    }
}
