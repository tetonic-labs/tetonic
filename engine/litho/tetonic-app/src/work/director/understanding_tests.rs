use super::*;

#[tokio::test]
async fn guide_retains_corrected_understanding_across_turns_and_restart_without_planning() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("understanding.db");
    let source = uuid::Uuid::new_v4().to_string();
    let followup = uuid::Uuid::new_v4().to_string();
    tokio::task::LocalSet::new().run_until(async {
        for (index, (turn, parent, notes)) in [
            (source.clone(), None, "Compare a workshop and a reading group. Cost is uncertain; no work approved."),
            (followup.clone(), Some(source.clone()), "The owner rejected the workshop. Explore the reading group; cost is still uncertain."),
        ].into_iter().enumerate() {
            let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
                index == 0, "work_plan", json!({"operation":"remember", "direction":notes, "plan":null, "work_id":null, "allocations":null}), false,
            ).await;
            let workspace = LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url).await.unwrap();
            workspace.submit_with_purpose(turn.clone(), notes.into(), shaping::GUIDE.into(), parent, WorkPurpose::Explore).await.unwrap();
            done(&workspace, &turn).await;
            let view = workspace.plan_view(&source).await.unwrap();
            assert!(view.plans.is_empty() && view.execution.is_none() && view.generation.is_none());
            let brief = view.understanding.unwrap();
            assert_eq!(brief.body, notes);
            assert_eq!(brief.revision, (index + 1) as i64);
            let task = workspace.task(&turn).await.unwrap();
            assert_eq!(task.guide_activity.last().unwrap().operation, "remember");
            assert_eq!(task.guide_activity.last().unwrap().state, "completed");
            let prompt = workspace.conversation_input(&uuid::Uuid::new_v4().to_string(), shaping::GUIDE, Some(&turn), "What should I consider next?").await.unwrap();
            assert!(prompt.contains(notes));
            assert!(prompt.contains("draft; not execution authority"));
            assert!(calls.lock().unwrap()[1]["messages"].to_string().contains("work_launched"));
            assert_eq!(workspace.snapshot().await.unwrap().tasks.len(), index + 1);
            let (_, session) = workspace.bind_director(&turn).await.unwrap();
            assert!(workspace.director_command(&session.binding, "late-save", Command::Remember { direction: "Late update".into() }).await.is_err());
            drop(workspace);
            server.abort();
        }
    }).await;
}

#[test]
fn understanding_tool_validates_notes_without_requiring_a_plan() {
    use crate::resources::work_director::command;
    assert!(matches!(
        command(
            json!({"operation":"remember","direction":"Keep B; A was rejected.","plan":null,"work_id":null,"allocations":null})
        ),
        Ok(Command::Remember { .. })
    ));
    for bad in ["".into(), "x".repeat(6001), "null\0character".into()] {
        assert!(command(json!({"operation":"remember","direction":bad})).is_err());
    }
    assert!(command(json!({"operation":"remember","direction":"Notes","work_id":uuid::Uuid::new_v4().to_string()})).is_err());
    assert!(command(
        json!({"operation":"remember","direction":"Notes","plan":plan("Not allowed")})
    )
    .is_err());
}
