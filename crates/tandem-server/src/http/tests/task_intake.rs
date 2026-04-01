use super::*;

#[tokio::test]
async fn task_intake_preview_roundtrip_for_grouped_task() {
    let state = test_state().await;
    let app = app_router(state);
    let request = tandem_orchestrator::TaskIntakeRequest::grouped_tasks_mission_preview(
        "task-42",
        "Sprint slice",
        tandem_orchestrator::TaskSourceKind::GitHubProjectItem,
        "release-2026-04",
    )
    .with_source_ref("proj-item-42")
    .with_repo_binding("org/repo", "/workspace/repo")
    .with_project_context("Release 2026", "In Review")
    .with_labels(vec!["sprint".to_string(), "backend".to_string()])
    .with_related_task_ids(vec!["task-a".to_string(), "task-b".to_string()])
    .with_preferred_route(tandem_orchestrator::TaskRouteKind::CoderRun);

    let req = Request::builder()
        .method("POST")
        .uri("/task-intake/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&request).expect("task json"),
        ))
        .expect("preview request");
    let resp = app.oneshot(req).await.expect("preview response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let payload: Value = serde_json::from_slice(&body).expect("preview json");

    assert_eq!(
        payload
            .get("task")
            .and_then(|task| task.get("task_id"))
            .and_then(|value| value.as_str()),
        Some("task-42")
    );
    assert_eq!(
        payload
            .get("preview")
            .and_then(|preview| preview.get("preferred_route"))
            .and_then(|value| value.as_str()),
        Some("coder_run")
    );
    assert_eq!(
        payload
            .get("preview")
            .and_then(|preview| preview.get("is_grouped"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        payload
            .get("preview")
            .and_then(|preview| preview.get("has_repo_binding"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        payload
            .get("task")
            .and_then(|task| task.get("project_name"))
            .and_then(|value| value.as_str()),
        Some("Release 2026")
    );
    assert_eq!(
        payload
            .get("task")
            .and_then(|task| task.get("project_column"))
            .and_then(|value| value.as_str()),
        Some("In Review")
    );
    assert_eq!(
        payload
            .get("grouping_signals")
            .and_then(|signals| signals.as_array())
            .map(|signals| signals.len()),
        Some(9)
    );
}
