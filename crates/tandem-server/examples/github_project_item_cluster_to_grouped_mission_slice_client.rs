use anyhow::Context;
use serde_json::json;
use tandem_orchestrator::{TaskBoardItem, TaskIntakeRequest, TaskRouteKind, TaskSourceKind};
use tandem_plan_compiler::api::{
    summarize_mission_coder_run_handoffs, summarize_mission_execution_boundary,
    MissionBlueprintPreview,
};
use tandem_workflows::{
    ApprovalDecision, HumanApprovalGate, MissionBlueprint, MissionMilestoneBlueprint,
    MissionPhaseBlueprint, MissionPhaseExecutionMode, MissionTeamBlueprint,
    OutputContractBlueprint, ReviewStage, ReviewStageKind, WorkstreamBlueprint,
};

fn project_cluster() -> Vec<TaskBoardItem> {
    vec![
        TaskBoardItem::new("gh-project-item-41", "Normalize intake schema")
            .with_source_ref("proj-item-41")
            .with_description("Make GitHub Project items map into task intake consistently.")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_project_context("Release 2026-05", "In Progress")
            .with_acceptance_criteria(vec!["schema stays stable".to_string()])
            .with_labels(vec!["project".to_string(), "intake".to_string()])
            .with_related_task_ids(vec!["task-a".to_string()])
            .with_grouping_key("release-2026-05"),
        TaskBoardItem::new("gh-project-item-42", "Wire mission handoff metadata")
            .with_source_ref("proj-item-42")
            .with_description("Expose grouped mission workstreams to coder handoff summaries.")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_project_context("Release 2026-05", "In Review")
            .with_acceptance_criteria(vec!["handoff summary stays stable".to_string()])
            .with_labels(vec!["project".to_string(), "handoff".to_string()])
            .with_related_task_ids(vec!["task-b".to_string(), "task-c".to_string()])
            .with_grouping_key("release-2026-05"),
        TaskBoardItem::new("gh-project-item-43", "Keep governance separate")
            .with_source_ref("proj-item-43")
            .with_description("Preserve approval and validation as distinct mission nodes.")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_project_context("Release 2026-05", "Done")
            .with_acceptance_criteria(vec!["governance stays distinct".to_string()])
            .with_labels(vec!["project".to_string(), "governance".to_string()])
            .with_related_task_ids(vec!["task-d".to_string()])
            .with_grouping_key("release-2026-05"),
    ]
}

fn workstream_from_board_item(
    board_item: &TaskBoardItem,
    index: usize,
    total: usize,
    grouping_key: &str,
    previous_workstream_id: Option<&str>,
) -> WorkstreamBlueprint {
    WorkstreamBlueprint {
        workstream_id: board_item.board_item_id.clone(),
        title: board_item.title.clone(),
        objective: board_item
            .description
            .clone()
            .unwrap_or_else(|| format!("Complete {}", board_item.title)),
        role: "worker".to_string(),
        priority: Some((total - index) as i32),
        phase_id: Some("implementation".to_string()),
        lane: Some("coding".to_string()),
        milestone: Some(grouping_key.to_string()),
        template_id: None,
        prompt: format!(
            "Implement project item `{}`.\n\nSource ref: {}\nRepo: {}\nWorkspace: {}\nLabels: {}\nAcceptance criteria:\n{}",
            board_item.board_item_id,
            board_item
                .source_ref
                .clone()
                .unwrap_or_else(|| board_item.board_item_id.clone()),
            board_item.repo_slug.clone().unwrap_or_else(|| "unknown".to_string()),
            board_item
                .workspace_root
                .clone()
                .unwrap_or_else(|| "/workspace/repo".to_string()),
            if board_item.labels.is_empty() {
                "(none)".to_string()
            } else {
                board_item.labels.join(", ")
            },
            if board_item.acceptance_criteria.is_empty() {
                "- (none)".to_string()
            } else {
                board_item
                    .acceptance_criteria
                    .iter()
                    .map(|criterion| format!("- {criterion}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        ),
        model_override: None,
        tool_allowlist_override: Vec::new(),
        mcp_servers_override: Vec::new(),
        depends_on: previous_workstream_id
            .map(|previous| vec![previous.to_string()])
            .unwrap_or_default(),
        input_refs: Vec::new(),
        output_contract: OutputContractBlueprint {
            kind: "brief".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "validation": { "type": "string" }
                }
            })),
            summary_guidance: Some(
                "Summarize the implementation result and the validation state.".to_string(),
            ),
        },
        retry_policy: None,
        timeout_ms: Some(30 * 60 * 1000),
        metadata: Some(json!({
            "source_board_item_id": board_item.board_item_id,
            "source_ref": board_item.source_ref,
            "repo_slug": board_item.repo_slug,
            "workspace_root": board_item.workspace_root,
            "grouping_key": board_item.grouping_key,
            "related_task_ids": board_item.related_task_ids,
        })),
    }
}

fn mission_blueprint_from_cluster(board_items: &[TaskBoardItem]) -> MissionBlueprint {
    let primary = board_items.first().expect("at least one board item");
    let workspace_root = primary
        .workspace_root
        .clone()
        .unwrap_or_else(|| "/workspace/repo".to_string());
    let grouping_key = primary
        .grouping_key
        .clone()
        .unwrap_or_else(|| "grouped-slice".to_string());

    let intake_previews = board_items
        .iter()
        .map(|item| {
            TaskIntakeRequest::from_board_item(
                item,
                TaskSourceKind::GitHubProjectItem,
                TaskRouteKind::MissionPreview,
            )
            .preview()
        })
        .collect::<Vec<_>>();

    let workstreams = board_items
        .iter()
        .enumerate()
        .map(|(index, board_item)| {
            let previous_workstream_id = index
                .checked_sub(1)
                .and_then(|previous_index| board_items.get(previous_index))
                .map(|item| item.board_item_id.as_str());
            workstream_from_board_item(
                board_item,
                index,
                board_items.len(),
                &grouping_key,
                previous_workstream_id,
            )
        })
        .collect::<Vec<_>>();

    let approval_targets = workstreams
        .iter()
        .map(|workstream| workstream.workstream_id.clone())
        .collect::<Vec<_>>();

    MissionBlueprint {
        mission_id: format!("mission-{}", primary.board_item_id),
        title: format!("Grouped project slice for {}", primary.title),
        goal: format!(
            "Complete a grouped release slice of {} project items",
            board_items.len()
        ),
        success_criteria: vec![
            "The cluster becomes a single mission slice".to_string(),
            "Coder work remains separate from governance approval".to_string(),
            "Mission handoff metadata is preserved for each workstream".to_string(),
        ],
        shared_context: Some(format!(
            "Project items: {}\nIntake previews: {}\nGrouping key: {}",
            board_items
                .iter()
                .map(|item| item.board_item_id.clone())
                .collect::<Vec<_>>()
                .join(", "),
            intake_previews.len(),
            grouping_key
        )),
        workspace_root,
        orchestrator_template_id: None,
        phases: vec![MissionPhaseBlueprint {
            phase_id: "implementation".to_string(),
            title: "Implementation".to_string(),
            description: Some("Coder execution lane for grouped work".to_string()),
            execution_mode: Some(MissionPhaseExecutionMode::Soft),
        }],
        milestones: vec![MissionMilestoneBlueprint {
            milestone_id: grouping_key.clone(),
            title: format!("Mission slice: {}", grouping_key),
            description: Some("Grouped project item completion marker".to_string()),
            phase_id: Some("implementation".to_string()),
            required_stage_ids: vec!["approval".to_string()],
        }],
        team: MissionTeamBlueprint {
            allowed_template_ids: Vec::new(),
            default_model_policy: None,
            allowed_mcp_servers: Vec::new(),
            max_parallel_agents: Some(3),
            mission_budget: None,
            orchestrator_only_tool_calls: true,
        },
        workstreams,
        review_stages: vec![ReviewStage {
            stage_id: "approval".to_string(),
            stage_kind: ReviewStageKind::Approval,
            title: "Review grouped project slice".to_string(),
            priority: Some(1),
            phase_id: Some("implementation".to_string()),
            lane: Some("governance".to_string()),
            milestone: Some(grouping_key.clone()),
            target_ids: approval_targets,
            role: Some("orchestrator".to_string()),
            template_id: None,
            prompt: "Review the grouped project slice and approve or send back for rework."
                .to_string(),
            checklist: vec![
                "All project items became workstreams".to_string(),
                "Validation output is present".to_string(),
                "Governance stays separate from coder execution".to_string(),
            ],
            model_override: None,
            tool_allowlist_override: Vec::new(),
            mcp_servers_override: Vec::new(),
            gate: Some(HumanApprovalGate {
                required: true,
                decisions: vec![
                    ApprovalDecision::Approve,
                    ApprovalDecision::Rework,
                    ApprovalDecision::Cancel,
                ],
                rework_targets: board_items
                    .iter()
                    .map(|item| item.board_item_id.clone())
                    .collect(),
                instructions: Some(
                    "Approve only when the grouped project slice is ready for handoff.".to_string(),
                ),
            }),
        }],
        metadata: Some(json!({
            "source_board_items": board_items.iter().map(|item| item.board_item_id.clone()).collect::<Vec<_>>(),
            "grouping_key": grouping_key,
            "intake_preview_count": intake_previews.len(),
        })),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url =
        std::env::var("TANDEM_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let board_items = project_cluster();
    let mission_blueprint = mission_blueprint_from_cluster(&board_items);

    let preview = reqwest::Client::new()
        .post(format!("{base_url}/mission-builder/compile-preview"))
        .json(&json!({ "blueprint": mission_blueprint }))
        .send()
        .await
        .context("send mission-builder preview request")?
        .error_for_status()
        .context("mission-builder preview returned an error status")?
        .json::<MissionBlueprintPreview>()
        .await
        .context("decode mission-builder preview response")?;

    let boundary = summarize_mission_execution_boundary(&preview);
    let handoffs = summarize_mission_coder_run_handoffs(&preview);

    println!(
        "project_items={} coder_nodes={} governance_nodes={} handoffs={} validation={}",
        board_items.len(),
        boundary.coder_run_node_ids.len(),
        boundary.governance_node_ids.len(),
        handoffs.len(),
        preview.validation.len()
    );

    Ok(())
}
