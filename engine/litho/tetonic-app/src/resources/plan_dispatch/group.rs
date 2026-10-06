//! Send explicit selections to the host dependency scheduler in one request.
//! No new admissions, dependencies, budgets or completion authority live here.
use super::*;
use serde::Deserialize;
use serde_json::Value;

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
            "Select 1–12 distinct agreed keys from: {}.",
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
    let (reply, receiver) = tokio::sync::oneshot::channel();
    if dispatch
        .sender
        .send(DispatchCall {
            keys,
            grouped,
            attempt,
            reply,
        })
        .await
        .is_err()
    {
        return ToolOutcome::fail("The plan dispatcher is unavailable", "unavailable");
    }
    receiver
        .await
        .unwrap_or_else(|_| ToolOutcome::fail("The plan dispatcher stopped", "unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
