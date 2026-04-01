use anyhow::Context;
use serde_json::json;
use tandem_orchestrator::TaskBoardItem;
use tandem_plan_compiler::api::{
    summarize_mission_coder_run_handoffs, summarize_mission_execution_boundary,
    MissionBlueprintPreview,
};
use tandem_workflows::{
    ApprovalDecision, HumanApprovalGate, MissionBlueprint, MissionMilestoneBlueprint,
    MissionPhaseBlueprint, MissionPhaseExecutionMode, MissionTeamBlueprint,
    OutputContractBlueprint, ReviewStage, ReviewStageKind, WorkstreamBlueprint,
};

fn workstream_from_board_item(
    board_item: &TaskBoardItem,
    index: usize,
    total: usize,
    grouping_key: &str,
    previous_workstream_id: Option<&str>,
) -> WorkstreamBlueprint {
    let dependency = previous_workstream_id
        .map(|previous| vec![previous.to_string()])
        .unwrap_or_default();

    WorkstreamBlueprint {
        workstream_id: board_item.board_item_id.clone(),
        title: board_item.title.clone(),
        objective: board_item
            .description
            .clone()
            .unwrap_or_else(|| board_item.title.clone()),
        role: "worker".to_string(),
        priority: Some((total - index) as i32),
        phase_id: Some("implementation".to_string()),
        lane: Some("coding".to_string()),
        milestone: Some(grouping_key.to_string()),
        template_id: None,
        prompt: format!(
            "Complete grouped project item `{}`.\n\nSource ref: {}\nRepo: {}\nWorkspace: {}\nAcceptance criteria:\n{}",
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
        depends_on: dependency,
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
                "Summarize the item result and the validation state.".to_string(),
            ),
        },
        retry_policy: None,
        timeout_ms: Some(30 * 60 * 1000),
        metadata: Some(json!({
            "source_board_item_id": board_item.board_item_id,
            "grouping_key": board_item.grouping_key,
            "related_task_ids": board_item.related_task_ids,
        })),
    }
}

fn mission_blueprint_from_board_items(board_items: &[TaskBoardItem]) -> MissionBlueprint {
    let primary = board_items.first().expect("at least one board item");
    let workspace_root = primary
        .workspace_root
        .clone()
        .unwrap_or_else(|| "/workspace/repo".to_string());
    let grouping_key = primary
        .grouping_key
        .clone()
        .unwrap_or_else(|| "grouped-slice".to_string());

    let workstreams = board_items
        .iter()
        .enumerate()
        .map(|(index, board_item)| {
            let previous = index
                .checked_sub(1)
                .and_then(|previous_index| board_items.get(previous_index))
                .map(|item| item.board_item_id.as_str());
            workstream_from_board_item(
                board_item,
                index,
                board_items.len(),
                &grouping_key,
                previous,
            )
        })
        .collect::<Vec<_>>();
    let approval_targets = workstreams
        .iter()
        .map(|workstream| workstream.workstream_id.clone())
        .collect::<Vec<_>>();

    MissionBlueprint {
        mission_id: format!("mission-{}", grouping_key),
        title: format!("Grouped project slice for {}", primary.title),
        goal: format!(
            "Complete a grouped GitHub Project slice for {}",
            primary.title
        ),
        success_criteria: vec![
            "All grouped project items are represented as workstreams".to_string(),
            "The mission preview keeps coder work separate from governance approval".to_string(),
        ],
        shared_context: Some(format!(
            "Grouped items: {}\nGrouping key: {}",
            board_items
                .iter()
                .map(|item| item.board_item_id.clone())
                .collect::<Vec<_>>()
                .join(", "),
            grouping_key
        )),
        workspace_root,
        orchestrator_template_id: None,
        phases: vec![MissionPhaseBlueprint {
            phase_id: "implementation".to_string(),
            title: "Implementation".to_string(),
            description: Some("Coder execution lane for grouped project items".to_string()),
            execution_mode: Some(MissionPhaseExecutionMode::Soft),
        }],
        milestones: vec![MissionMilestoneBlueprint {
            milestone_id: grouping_key.clone(),
            title: format!("Grouped mission slice: {}", grouping_key),
            description: Some("Shared marker for the grouped project slice".to_string()),
            phase_id: Some("implementation".to_string()),
            required_stage_ids: vec!["approval".to_string()],
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
        review_stages: vec![ReviewStage {
            stage_id: "approval".to_string(),
            stage_kind: ReviewStageKind::Approval,
            title: "Approve grouped project slice".to_string(),
            priority: Some(1),
            phase_id: Some("implementation".to_string()),
            lane: Some("governance".to_string()),
            milestone: Some(grouping_key.clone()),
            target_ids: approval_targets,
            role: Some("orchestrator".to_string()),
            template_id: None,
            prompt: "Approve the grouped project slice or send it back for rework.".to_string(),
            checklist: vec![
                "Each item became a workstream".to_string(),
                "The preview separates coder and governance nodes".to_string(),
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
                    "Approve the grouped slice only when the workstream plan is ready.".to_string(),
                ),
            }),
        }],
        metadata: Some(json!({
            "source_board_items": board_items.iter().map(|item| item.board_item_id.clone()).collect::<Vec<_>>(),
            "grouping_key": grouping_key,
        })),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url =
        std::env::var("TANDEM_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let board_items = vec![
        TaskBoardItem::new("gh-project-item-21", "Ship grouped intake")
            .with_source_ref("proj-item-21")
            .with_description("Group project items into one mission slice.")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_acceptance_criteria(vec!["mission preview compiles".to_string()])
            .with_labels(vec!["grouped".to_string(), "project".to_string()])
            .with_grouping_key("release-2026-04"),
        TaskBoardItem::new("gh-project-item-22", "Wire the lane")
            .with_source_ref("proj-item-22")
            .with_description("Keep coder work in a separate lane from governance.")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_acceptance_criteria(vec!["lane separation is visible".to_string()])
            .with_labels(vec!["grouped".to_string(), "lane".to_string()])
            .with_grouping_key("release-2026-04"),
        TaskBoardItem::new("gh-project-item-23", "Add approval gate")
            .with_source_ref("proj-item-23")
            .with_description("Add a human approval checkpoint for the grouped slice.")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_acceptance_criteria(vec!["approval stage exists".to_string()])
            .with_labels(vec!["grouped".to_string(), "approval".to_string()])
            .with_grouping_key("release-2026-04"),
    ];

    let mission_blueprint = mission_blueprint_from_board_items(&board_items);
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
        "mission={} workstreams={} coder_nodes={} governance_nodes={} handoffs={}",
        preview.blueprint.mission_id,
        preview.blueprint.workstreams.len(),
        boundary.coder_run_node_ids.len(),
        boundary.governance_node_ids.len(),
        handoffs.len(),
    );

    Ok(())
}
