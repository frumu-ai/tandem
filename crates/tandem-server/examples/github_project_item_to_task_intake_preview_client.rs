use anyhow::Context;
use serde::Deserialize;
use tandem_orchestrator::{
    TaskBoardItem, TaskGroupingSignal, TaskIntakePreview, TaskIntakeRequest, TaskRouteKind,
    TaskSourceKind,
};

#[derive(Debug, Deserialize)]
struct TaskIntakePreviewResponse {
    task: TaskIntakeRequest,
    preview: TaskIntakePreview,
    grouping_signals: Vec<TaskGroupingSignal>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url =
        std::env::var("TANDEM_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let board_item = TaskBoardItem::new("gh-project-item-17", "Ship the adapter")
        .with_source_ref("proj-item-17")
        .with_description("Normalize a GitHub Project item into task intake.")
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_acceptance_criteria(vec!["preview returns task shape".to_string()])
        .with_labels(vec!["project".to_string(), "adapter".to_string()])
        .with_related_task_ids(vec!["task-a".to_string(), "task-b".to_string()])
        .with_grouping_key("release-2026-04");

    let request = TaskIntakeRequest::from_board_item(
        &board_item,
        TaskSourceKind::GitHubProjectItem,
        TaskRouteKind::MissionPreview,
    );

    let response = reqwest::Client::new()
        .post(format!("{base_url}/task-intake/preview"))
        .json(&request)
        .send()
        .await
        .context("send task-intake preview request")?
        .error_for_status()
        .context("task-intake preview returned an error status")?
        .json::<TaskIntakePreviewResponse>()
        .await
        .context("decode task-intake preview response")?;

    println!(
        "task={} route={:?} grouped={} repo_bound={} signals={}",
        response.task.task_id,
        response.preview.preferred_route,
        response.preview.is_grouped,
        response.preview.has_repo_binding,
        response.grouping_signals.len()
    );

    Ok(())
}
