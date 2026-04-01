use anyhow::Context;
use serde::Deserialize;
use serde_json::json;
use tandem_orchestrator::{
    TaskGroupingSignal, TaskIntakePreview, TaskIntakeRequest, TaskRouteKind, TaskSourceKind,
};
use tandem_workflows::{
    MissionBlueprint, MissionTeamBlueprint, OutputContractBlueprint, WorkstreamBlueprint,
};

#[derive(Debug, Deserialize)]
struct TaskIntakePreviewResponse {
    task: TaskIntakeRequest,
    preview: TaskIntakePreview,
    grouping_signals: Vec<TaskGroupingSignal>,
}

#[derive(Debug, Deserialize)]
struct MissionBlueprintPreviewResponse {
    work_items: serde_json::Value,
    node_previews: serde_json::Value,
    validation: serde_json::Value,
}

fn mission_blueprint_from_preview(response: &TaskIntakePreviewResponse) -> MissionBlueprint {
    let workspace_root = response
        .task
        .workspace_root
        .clone()
        .unwrap_or_else(|| "/workspace/repo".to_string());
    let grouped_ids = if response.task.related_task_ids.is_empty() {
        vec![response.task.task_id.clone()]
    } else {
        response.task.related_task_ids.clone()
    };
    let workstreams = grouped_ids
        .iter()
        .enumerate()
        .map(|(index, task_id)| WorkstreamBlueprint {
            workstream_id: format!("workstream-{}", index + 1),
            title: format!("Execute {task_id}"),
            objective: format!("Complete task {task_id} from grouped intake"),
            role: "worker".to_string(),
            priority: Some((grouped_ids.len() - index) as i32),
            phase_id: Some("implementation".to_string()),
            lane: Some("coding".to_string()),
            milestone: response.task.grouping_key.clone(),
            template_id: None,
            prompt: format!(
                "Work on task `{task_id}` for grouped intake `{}`.\n\nGrouping signals:\n{}",
                response.task.task_id,
                response
                    .grouping_signals
                    .iter()
                    .map(|signal| {
                        format!(
                            "- {}: {}",
                            serde_json::to_string(&signal.kind)
                                .unwrap_or_default()
                                .trim_matches('"'),
                            signal.value
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            model_override: None,
            tool_allowlist_override: Vec::new(),
            mcp_servers_override: Vec::new(),
            depends_on: if index == 0 {
                Vec::new()
            } else {
                vec![format!("workstream-{}", index)]
            },
            input_refs: Vec::new(),
            output_contract: OutputContractBlueprint {
                kind: "brief".to_string(),
                schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string" }
                    }
                })),
                summary_guidance: Some(
                    "Summarize the work done and the validation result.".to_string(),
                ),
            },
            retry_policy: None,
            timeout_ms: Some(30 * 60 * 1000),
            metadata: Some(json!({
                "source_task_id": response.task.task_id,
                "task_id": task_id,
                "preferred_route": response.preview.preferred_route,
                "grouping_signal_count": response.preview.grouping_signal_count,
            })),
        })
        .collect::<Vec<_>>();

    MissionBlueprint {
        mission_id: format!("mission-{}", response.task.task_id),
        title: format!("Grouped tasks for {}", response.task.title),
        goal: format!("Complete grouped task slice {}", response.task.task_id),
        success_criteria: vec![
            "All grouped tasks are represented as workstreams".to_string(),
            "The mission preview separates coder work from governance nodes".to_string(),
        ],
        shared_context: Some(format!(
            "Source task: {}\nRoute hint: {:?}",
            response.task.task_id, response.preview.preferred_route
        )),
        workspace_root,
        orchestrator_template_id: None,
        phases: vec![tandem_workflows::MissionPhaseBlueprint {
            phase_id: "implementation".to_string(),
            title: "Implementation".to_string(),
            description: Some("Coder execution lane for grouped work".to_string()),
            execution_mode: Some(tandem_workflows::MissionPhaseExecutionMode::Soft),
        }],
        milestones: vec![tandem_workflows::MissionMilestoneBlueprint {
            milestone_id: "grouped-slice".to_string(),
            title: "Grouped task slice".to_string(),
            description: Some("Completion marker for the grouped intake slice".to_string()),
            phase_id: Some("implementation".to_string()),
            required_stage_ids: Vec::new(),
        }],
        team: MissionTeamBlueprint {
            allowed_template_ids: Vec::new(),
            default_model_policy: None,
            allowed_mcp_servers: Vec::new(),
            max_parallel_agents: Some(2),
            mission_budget: None,
            orchestrator_only_tool_calls: true,
        },
        workstreams,
        review_stages: Vec::new(),
        metadata: Some(json!({
            "source_task_id": response.task.task_id,
            "grouping_signals": response.grouping_signals,
            "preferred_route": response.preview.preferred_route,
        })),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url =
        std::env::var("TANDEM_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let task_request = TaskIntakeRequest::grouped_tasks_mission_preview(
        "task-42",
        "Sprint slice",
        TaskSourceKind::GitHubProjectItem,
        "release-2026-04",
    )
    .with_source_ref("proj-item-42")
    .with_repo_binding("org/repo", "/workspace/repo")
    .with_labels(vec!["sprint".to_string(), "backend".to_string()])
    .with_related_task_ids(vec!["task-a".to_string(), "task-b".to_string()])
    .with_preferred_route(TaskRouteKind::CoderRun);

    let intake = reqwest::Client::new()
        .post(format!("{base_url}/task-intake/preview"))
        .json(&task_request)
        .send()
        .await
        .context("send task-intake preview request")?
        .error_for_status()
        .context("task-intake preview returned an error status")?
        .json::<TaskIntakePreviewResponse>()
        .await
        .context("decode task-intake preview response")?;

    let mission_blueprint = mission_blueprint_from_preview(&intake);
    let mission_preview = reqwest::Client::new()
        .post(format!("{base_url}/mission-builder/compile-preview"))
        .json(&json!({ "blueprint": mission_blueprint }))
        .send()
        .await
        .context("send mission-builder preview request")?
        .error_for_status()
        .context("mission-builder preview returned an error status")?
        .json::<MissionBlueprintPreviewResponse>()
        .await
        .context("decode mission-builder preview response")?;

    println!(
        "mission_preview nodes={} work_items={} validation_entries={}",
        mission_preview
            .node_previews
            .as_array()
            .map(|nodes| nodes.len())
            .unwrap_or(0),
        mission_preview
            .work_items
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0),
        mission_preview
            .validation
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0)
    );

    Ok(())
}
