use super::*;
use tandem_types::EngineEvent;

fn assert_run_correlation(event: &EngineEvent, session_id: &str, run_id: &str) {
    assert_eq!(
        event.properties.get("sessionID").and_then(Value::as_str),
        Some(session_id)
    );
    assert_eq!(
        event.properties.get("runID").and_then(Value::as_str),
        Some(run_id)
    );
    let envelope = event.envelope.as_ref().expect("runtime event envelope");
    assert_eq!(envelope.session_id.as_deref(), Some(session_id));
    assert_eq!(envelope.run_id.as_deref(), Some(run_id));
}

#[tokio::test]
async fn plan_todo_fallback_preserves_run_id_for_live_and_persisted_events() {
    let base = std::env::temp_dir().join(format!("plan-todo-fallback-{}", Uuid::new_v4()));
    let storage = Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(
        Some("plan todo fallback".to_string()),
        Some(base.to_string_lossy().to_string()),
    );
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let bus = EventBus::new();
    let mut live_rx = bus.subscribe();
    let mut persistence_rx = bus
        .take_session_part_receiver()
        .expect("session part persistence receiver");
    emit_plan_todo_fallback(
        storage,
        &bus,
        &session_id,
        "message-plan-todo",
        Some("run-plan-todo"),
        "- [ ] Preserve fallback correlation",
    )
    .await;

    let mut live_events = Vec::new();
    while let Ok(event) = live_rx.try_recv() {
        live_events.push(event);
    }
    assert_eq!(
        live_events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "message.part.updated",
            "message.part.updated",
            "todo.updated"
        ]
    );
    for event in &live_events {
        assert_run_correlation(event, &session_id, "run-plan-todo");
    }

    let persisted_event = persistence_rx
        .try_recv()
        .expect("completed fallback tool result queued for persistence");
    assert_eq!(persisted_event.event_type, "message.part.updated");
    assert_run_correlation(&persisted_event, &session_id, "run-plan-todo");
    assert_eq!(
        persisted_event
            .properties
            .pointer("/part/tool")
            .and_then(Value::as_str),
        Some("todo_write")
    );
    assert!(
        persistence_rx.try_recv().is_err(),
        "running fallback invocation remains filtered from persistence"
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn plan_question_fallback_preserves_run_id_on_question_event() {
    let base = std::env::temp_dir().join(format!("plan-question-fallback-{}", Uuid::new_v4()));
    let storage = Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(
        Some("plan question fallback".to_string()),
        Some(base.to_string_lossy().to_string()),
    );
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let bus = EventBus::new();
    let mut live_rx = bus.subscribe();
    emit_plan_question_fallback(
        storage.clone(),
        &bus,
        &session_id,
        "message-plan-question",
        Some("run-plan-question"),
        "I need more detail before I can produce a task list.",
    )
    .await;

    let event = live_rx.try_recv().expect("question fallback event");
    assert_eq!(event.event_type, "question.asked");
    assert_run_correlation(&event, &session_id, "run-plan-question");
    assert!(
        live_rx.try_recv().is_err(),
        "only one question event emitted"
    );
    assert_eq!(storage.list_question_requests().await.len(), 1);
    let _ = std::fs::remove_dir_all(base);
}
