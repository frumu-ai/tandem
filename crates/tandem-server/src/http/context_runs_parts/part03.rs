
fn routine_run_status_to_step(status: &crate::RoutineRunStatus) -> ContextStepStatus {
    match status {
        crate::RoutineRunStatus::Queued => ContextStepStatus::Pending,
        crate::RoutineRunStatus::PendingApproval
        | crate::RoutineRunStatus::Paused
        | crate::RoutineRunStatus::BlockedPolicy
        | crate::RoutineRunStatus::Denied
        | crate::RoutineRunStatus::Cancelled => ContextStepStatus::Blocked,
        crate::RoutineRunStatus::Running => ContextStepStatus::InProgress,
        crate::RoutineRunStatus::Completed => ContextStepStatus::Done,
        crate::RoutineRunStatus::Failed => ContextStepStatus::Failed,
    }
}

pub(crate) async fn sync_routine_run_blackboard(
    state: &AppState,
    run: &crate::RoutineRunRecord,
) -> Result<String, StatusCode> {
    let run_id = routine_context_run_id(&run.run_id);

    if load_context_run_state(state, &run_id).await.is_err() {
        let now = crate::now_ms();
        let context_run = ContextRunState {
            run_id: run_id.clone(),
            run_type: "routine".to_string(),
            tenant_context: TenantContext::local_implicit(),
            source_client: Some("routine_runtime".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: routine_run_status_to_context(&run.status),
            objective: format!("Routine {} ({})", run.routine_id, run.entrypoint),
            workspace: ContextWorkspaceLease::default(),
            steps: vec![ContextRunStep {
                step_id: "routine-run".to_string(),
                title: format!("Execute routine {}", run.entrypoint),
                status: routine_run_status_to_step(&run.status),
            }],
            tasks: Vec::new(),
            why_next_step: Some("Track routine run lifecycle and output artifacts".to_string()),
            revision: 1,
            last_event_seq: 0,
            created_at_ms: run.created_at_ms.max(now),
            started_at_ms: run.started_at_ms.or(run.fired_at_ms),
            ended_at_ms: run.finished_at_ms,
            last_error: run.detail.clone(),
            updated_at_ms: run.updated_at_ms.max(now),
        };
        save_context_run_state(state, &context_run).await?;
    }

    let mut run_state = load_context_run_state(state, &run_id).await?;
    let now = crate::now_ms();
    run_state.status = routine_run_status_to_context(&run.status);
    run_state.objective = format!("Routine {} ({})", run.routine_id, run.entrypoint);
    run_state.updated_at_ms = run.updated_at_ms.max(now);
    run_state.started_at_ms = run_state
        .started_at_ms
        .or(run.started_at_ms)
        .or(run.fired_at_ms);
    run_state.ended_at_ms = run.finished_at_ms;
    run_state.last_error = run.detail.clone();
    run_state.steps = vec![ContextRunStep {
        step_id: "routine-run".to_string(),
        title: format!("Execute routine {}", run.entrypoint),
        status: routine_run_status_to_step(&run.status),
    }];
    save_context_run_state(state, &run_state).await?;

    let mut blackboard = load_context_blackboard(state, &run_id);
    for artifact_row in &run.artifacts {
        let artifact_id = format!("routine-artifact-{}", artifact_row.artifact_id);
        if blackboard.artifacts.iter().any(|row| row.id == artifact_id) {
            continue;
        }
        let artifact = ContextBlackboardArtifact {
            id: artifact_id,
            ts_ms: artifact_row.created_at_ms,
            path: artifact_row.uri.clone(),
            artifact_type: artifact_row.kind.clone(),
            step_id: Some("routine-run".to_string()),
            source_event_id: None,
        };
        let _ = context_run_engine()
            .commit_blackboard_patch(
                state,
                &run_id,
                ContextBlackboardPatchOp::AddArtifact,
                serde_json::to_value(&artifact).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            )
            .await?;
        blackboard.artifacts.push(artifact);
    }

    Ok(run_id)
}

fn workflow_run_status_to_context(status: &crate::WorkflowRunStatus) -> ContextRunStatus {
    match status {
        crate::WorkflowRunStatus::Queued => ContextRunStatus::Queued,
        crate::WorkflowRunStatus::Running => ContextRunStatus::Running,
        crate::WorkflowRunStatus::Completed | crate::WorkflowRunStatus::DryRun => {
            ContextRunStatus::Completed
        }
        crate::WorkflowRunStatus::Failed => ContextRunStatus::Failed,
    }
}

fn automation_node_builder_string(node: &crate::AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_node_builder_bool(node: &crate::AutomationFlowNode, key: &str) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_task_payload(node: &crate::AutomationFlowNode, output: Option<&Value>) -> Value {
    let mut payload = json!({
        "node_id": node.node_id,
        "title": automation_node_builder_string(node, "title").unwrap_or_else(|| node.objective.clone()),
        "name": node.objective,
        "description": node.objective,
        "agent_id": node.agent_id,
        "task_kind": automation_node_builder_string(node, "task_kind"),
        "backlog_task_id": automation_node_builder_string(node, "task_id"),
        "repo_root": automation_node_builder_string(node, "repo_root"),
        "write_scope": automation_node_builder_string(node, "write_scope"),
        "acceptance_criteria": automation_node_builder_string(node, "acceptance_criteria"),
        "task_dependencies": automation_node_builder_string(node, "task_dependencies"),
        "verification_state": automation_node_builder_string(node, "verification_state"),
        "task_owner": automation_node_builder_string(node, "task_owner"),
        "verification_command": automation_node_builder_string(node, "verification_command"),
        "output_path": automation_node_builder_string(node, "output_path"),
        "projects_backlog_tasks": automation_node_builder_bool(node, "project_backlog_tasks"),
    });
    if let Some(object) = payload.as_object_mut() {
        if let Some(output) = output {
            if let Some(status) = output.get("status").and_then(Value::as_str) {
                object.insert("node_status".to_string(), json!(status));
            }
            if let Some(failure_kind) = output.get("failure_kind").and_then(Value::as_str) {
                object.insert("failure_kind".to_string(), json!(failure_kind));
            }
            if let Some(reason) = output
                .get("validator_summary")
                .and_then(|value| value.get("reason"))
                .and_then(Value::as_str)
                .or_else(|| output.get("blocked_reason").and_then(Value::as_str))
            {
                let reason = reason.trim();
                if !reason.is_empty() {
                    object.insert("validator_reason".to_string(), json!(reason));
                }
            }
            if let Some(unmet) = output
                .get("validator_summary")
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .filter(|value| !value.is_empty())
            {
                object.insert(
                    "unmet_requirements".to_string(),
                    Value::Array(unmet.clone()),
                );
            }
            if let Some(actions) = output
                .get("artifact_validation")
                .and_then(|value| value.get("required_next_tool_actions"))
                .and_then(Value::as_array)
                .filter(|value| !value.is_empty())
            {
                object.insert(
                    "required_next_tool_actions".to_string(),
                    Value::Array(actions.clone()),
                );
            }
            if let Some(classification) = output
                .get("artifact_validation")
                .and_then(|value| value.get("blocking_classification"))
                .and_then(Value::as_str)
            {
                let classification = classification.trim();
                if !classification.is_empty() {
                    object.insert("blocking_classification".to_string(), json!(classification));
                }
            }
            if let Some(validation_basis) = output
                .get("artifact_validation")
                .and_then(|value| value.get("validation_basis"))
                .cloned()
                .filter(|value| !value.is_null())
            {
                object.insert("validation_basis".to_string(), validation_basis);
            }
            if let Some(knowledge_preflight) = output
                .get("knowledge_preflight")
                .cloned()
                .filter(|value| !value.is_null())
            {
                object.insert("knowledge_preflight".to_string(), knowledge_preflight);
            }
            if let Some(knowledge_preflight) =
                output.get("knowledge_preflight").and_then(Value::as_object)
            {
                if let Some(reuse_reason) = knowledge_preflight
                    .get("reuse_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    object.insert("knowledge_reuse_reason".to_string(), json!(reuse_reason));
                }
                if let Some(skip_reason) = knowledge_preflight
                    .get("skip_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    object.insert("knowledge_skip_reason".to_string(), json!(skip_reason));
                }
                if let Some(freshness_reason) = knowledge_preflight
                    .get("freshness_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    object.insert(
                        "knowledge_freshness_reason".to_string(),
                        json!(freshness_reason),
                    );
                }
            }
            if let Some(blocker_category) = output
                .get("blocker_category")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                object.insert("blocker_category".to_string(), json!(blocker_category));
            }
            if let Some(receipt_timeline) = output
                .get("receipt_timeline")
                .or_else(|| {
                    output
                        .get("attempt_evidence")
                        .and_then(|value| value.get("receipt_timeline"))
                })
                .cloned()
                .filter(|value| !value.is_null())
            {
                object.insert("receipt_timeline".to_string(), receipt_timeline);
            }
            if let Some(repair_attempt) = output
                .get("artifact_validation")
                .and_then(|value| value.get("repair_attempt"))
                .and_then(Value::as_u64)
            {
                object.insert("repair_attempt".to_string(), json!(repair_attempt));
            }
            if let Some(repair_attempts_remaining) = output
                .get("artifact_validation")
                .and_then(|value| value.get("repair_attempts_remaining"))
                .and_then(Value::as_u64)
            {
                object.insert(
                    "repair_attempts_remaining".to_string(),
                    json!(repair_attempts_remaining),
                );
            }
        }
    }
    payload
}

fn extract_markdown_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remainder = text;
    while let Some(start) = remainder.find("```") {
        remainder = &remainder[start + 3..];
        let Some(line_end) = remainder.find('\n') else {
            break;
        };
        let lang = remainder[..line_end].trim().to_ascii_lowercase();
        remainder = &remainder[line_end + 1..];
        let Some(end) = remainder.find("```") else {
            break;
        };
        let block = remainder[..end].trim();
        if !block.is_empty() && (lang.is_empty() || lang == "json" || lang == "javascript") {
            blocks.push(block.to_string());
        }
        remainder = &remainder[end + 3..];
    }
    blocks
}

fn extract_backlog_task_values(candidate: &Value) -> Vec<Value> {
    match candidate {
        Value::Array(items) => items.clone(),
        Value::Object(map) => {
            if let Some(items) = map.get("backlog_tasks").and_then(Value::as_array) {
                return items.clone();
            }
            if let Some(items) = map.get("tasks").and_then(Value::as_array) {
                return items.clone();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn normalize_backlog_task_identifier(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            normalized.push(ch);
        } else if !normalized.ends_with('-') {
            normalized.push('-');
        }
    }
    normalized.trim_matches('-').to_string()
}

fn parse_backlog_dependencies(value: Option<&Value>) -> Vec<String> {
    if let Some(items) = value.and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    value
        .and_then(Value::as_str)
        .map(|text| {
            text.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn backlog_task_status_from_value(value: &Value) -> ContextBlackboardTaskStatus {
    let status = value
        .get("status")
        .or_else(|| value.get("verification_state"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("runnable")
        .to_ascii_lowercase();
    match status.as_str() {
        "blocked" => ContextBlackboardTaskStatus::Blocked,
        "failed" | "verify_failed" => ContextBlackboardTaskStatus::Failed,
        "done" | "completed" | "verified" => ContextBlackboardTaskStatus::Done,
        "in_progress" | "running" | "claimed" => ContextBlackboardTaskStatus::InProgress,
        "ready" | "runnable" | "todo" | "queued" => ContextBlackboardTaskStatus::Runnable,
        _ => ContextBlackboardTaskStatus::Pending,
    }
}

fn parse_backlog_projection_tasks(
    automation: &crate::AutomationV2Spec,
    node: &crate::AutomationFlowNode,
    run: &crate::AutomationV2RunRecord,
    now: u64,
) -> Vec<ContextBlackboardTask> {
    let projects_backlog_tasks = automation_node_builder_bool(node, "project_backlog_tasks")
        || automation_node_builder_string(node, "task_kind")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("repo_plan"));
    if !projects_backlog_tasks {
        return Vec::new();
    }
    let Some(output) = run.checkpoint.node_outputs.get(&node.node_id) else {
        return Vec::new();
    };
    let text_candidates = [
        output
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str),
        output
            .get("content")
            .and_then(|content| content.get("raw_text"))
            .and_then(Value::as_str),
    ];
    let mut parsed_items = Vec::new();
    for text in text_candidates.into_iter().flatten() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            parsed_items.extend(extract_backlog_task_values(&value));
        }
        for block in extract_markdown_json_blocks(trimmed) {
            if let Ok(value) = serde_json::from_str::<Value>(&block) {
                parsed_items.extend(extract_backlog_task_values(&value));
            }
        }
    }
    parsed_items
        .into_iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            let title = object
                .get("title")
                .or_else(|| object.get("objective"))
                .or_else(|| object.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?
                .to_string();
            let raw_task_id = object
                .get("task_id")
                .or_else(|| object.get("id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| title.to_ascii_lowercase().replace(' ', "-"));
            let normalized_task_id = normalize_backlog_task_identifier(&raw_task_id);
            if normalized_task_id.is_empty() {
                return None;
            }
            let task_dependencies =
                parse_backlog_dependencies(object.get("task_dependencies").or_else(|| object.get("dependencies")));
            let task_owner = object
                .get("task_owner")
                .or_else(|| object.get("owner"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| automation_node_builder_string(node, "task_owner"));
            Some(ContextBlackboardTask {
                id: format!("backlog-{}-{}", node.node_id, normalized_task_id),
                task_type: "automation_backlog_item".to_string(),
                payload: json!({
                    "title": title,
                    "description": object.get("description").or_else(|| object.get("summary")).and_then(Value::as_str).map(str::trim).unwrap_or_default(),
                    "task_id": raw_task_id,
                    "backlog_task_id": raw_task_id,
                    "task_kind": object.get("task_kind").and_then(Value::as_str).unwrap_or("code_change"),
                    "repo_root": object.get("repo_root").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "repo_root")),
                    "write_scope": object.get("write_scope").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "write_scope")),
                    "acceptance_criteria": object.get("acceptance_criteria").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "acceptance_criteria")),
                    "task_dependencies": task_dependencies,
                    "verification_state": object.get("verification_state").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "verification_state")),
                    "task_owner": task_owner,
                    "verification_command": object.get("verification_command").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "verification_command")),
                    "source_node_id": node.node_id,
                    "projects_backlog_tasks": true,
                }),
                status: backlog_task_status_from_value(&item),
                workflow_id: Some(automation.automation_id.clone()),
                workflow_node_id: Some(node.node_id.clone()),
                parent_task_id: Some(format!("node-{}", node.node_id)),
                depends_on_task_ids: task_dependencies
                    .iter()
                    .map(|dep| format!("backlog-{}-{}", node.node_id, normalize_backlog_task_identifier(dep)))
                    .collect(),
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                assigned_agent: task_owner,
                priority: object
                    .get("priority")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                attempt: 0,
                max_attempts: 3,
                last_error: None,
                next_retry_at_ms: None,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: 1,
                created_ts: now,
                updated_ts: now,
            })
        })
        .collect()
}

fn workflow_action_status_to_context(
    status: &WorkflowActionRunStatus,
) -> ContextBlackboardTaskStatus {
    match status {
        WorkflowActionRunStatus::Pending => ContextBlackboardTaskStatus::Pending,
        WorkflowActionRunStatus::Running => ContextBlackboardTaskStatus::InProgress,
        WorkflowActionRunStatus::Completed => ContextBlackboardTaskStatus::Done,
        WorkflowActionRunStatus::Failed => ContextBlackboardTaskStatus::Failed,
        WorkflowActionRunStatus::Skipped => ContextBlackboardTaskStatus::Blocked,
    }
}

pub(crate) async fn sync_workflow_run_blackboard(
    state: &AppState,
    run: &crate::WorkflowRunRecord,
) -> Result<String, StatusCode> {
    let run_id = workflow_context_run_id(&run.run_id);

    if load_context_run_state(state, &run_id).await.is_err() {
        let workspace_root = state.workspace_index.snapshot().await.root;
        let now = crate::now_ms();
        let context_run = ContextRunState {
            run_id: run_id.clone(),
            run_type: "workflow".to_string(),
            tenant_context: TenantContext::local_implicit(),
            source_client: Some("workflow_runtime".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: workflow_run_status_to_context(&run.status),
            objective: format!("Workflow {}", run.workflow_id),
            workspace: ContextWorkspaceLease {
                workspace_id: run.workflow_id.clone(),
                canonical_path: workspace_root,
                lease_epoch: 0,
            },
            steps: run
                .actions
                .iter()
                .map(|action| ContextRunStep {
                    step_id: action.action_id.clone(),
                    title: action.action.clone(),
                    status: match workflow_action_status_to_context(&action.status) {
                        ContextBlackboardTaskStatus::Pending => ContextStepStatus::Pending,
                        ContextBlackboardTaskStatus::Runnable => ContextStepStatus::Runnable,
                        ContextBlackboardTaskStatus::InProgress => ContextStepStatus::InProgress,
                        ContextBlackboardTaskStatus::Done => ContextStepStatus::Done,
                        ContextBlackboardTaskStatus::Failed => ContextStepStatus::Failed,
                        ContextBlackboardTaskStatus::Blocked => ContextStepStatus::Blocked,
                    },
                })
                .collect(),
            tasks: Vec::new(),
            why_next_step: Some(
                "Track workflow hook actions via blackboard tasks and artifacts".to_string(),
            ),
            revision: 1,
            last_event_seq: 0,
            created_at_ms: run.created_at_ms.max(now),
            started_at_ms: Some(run.created_at_ms.max(now)),
            ended_at_ms: run.finished_at_ms,
            last_error: run.actions.iter().find_map(|action| action.detail.clone()),
            updated_at_ms: run.updated_at_ms.max(now),
        };
        save_context_run_state(state, &context_run).await?;
    }

    let mut run_state = load_context_run_state(state, &run_id).await?;
    let now = crate::now_ms();
    run_state.status = workflow_run_status_to_context(&run.status);
    run_state.objective = format!("Workflow {}", run.workflow_id);
    run_state.updated_at_ms = run.updated_at_ms.max(now);
    run_state.ended_at_ms = run.finished_at_ms;
    run_state.last_error = run.actions.iter().find_map(|action| action.detail.clone());
    run_state.steps = run
        .actions
        .iter()
        .map(|action| ContextRunStep {
            step_id: action.action_id.clone(),
            title: action.action.clone(),
            status: match workflow_action_status_to_context(&action.status) {
                ContextBlackboardTaskStatus::Pending => ContextStepStatus::Pending,
                ContextBlackboardTaskStatus::Runnable => ContextStepStatus::Runnable,
                ContextBlackboardTaskStatus::InProgress => ContextStepStatus::InProgress,
                ContextBlackboardTaskStatus::Done => ContextStepStatus::Done,
                ContextBlackboardTaskStatus::Failed => ContextStepStatus::Failed,
                ContextBlackboardTaskStatus::Blocked => ContextStepStatus::Blocked,
            },
        })
        .collect();
    save_context_run_state(state, &run_state).await?;

    let mut blackboard = load_context_blackboard(state, &run_id);
    for action in &run.actions {
        let task_id = format!("workflow-action-{}", action.action_id);
        let artifact_id = format!("workflow-artifact-{}", action.action_id);
        let artifact_ids = if action.output.is_some() {
            vec![artifact_id.clone()]
        } else {
            Vec::new()
        };
        let status = workflow_action_status_to_context(&action.status);
        let existing = run_state
            .tasks
            .iter()
            .find(|row| row.id == task_id)
            .cloned();
        if let Some(task) = existing {
            if task.status != status
                || task.last_error != action.detail
                || task.artifact_ids != artifact_ids
            {
                let next_task = ContextBlackboardTask {
                    status: status.clone(),
                    last_error: action.detail.clone(),
                    artifact_ids: artifact_ids.clone(),
                    task_rev: task.task_rev.saturating_add(1),
                    updated_ts: action.updated_at_ms.max(now),
                    ..task.clone()
                };
                let _ = context_run_engine()
                    .commit_task_mutation(
                        state,
                        &run_id,
                        next_task.clone(),
                        ContextBlackboardPatchOp::UpdateTaskState,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "error": action.detail,
                            "artifact_ids": artifact_ids,
                            "task_rev": next_task.task_rev,
                        }),
                        context_task_status_event_name(&status).to_string(),
                        workflow_run_status_to_context(&run.status),
                        None,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "error": action.detail,
                            "artifact_ids": artifact_ids,
                            "task_rev": next_task.task_rev,
                            "source": "workflow_runtime",
                        }),
                    )
                    .await?;
                if let Some(existing_task) =
                    run_state.tasks.iter_mut().find(|row| row.id == task_id)
                {
                    *existing_task = next_task.clone();
                }
            }
        } else {
            let task = ContextBlackboardTask {
                id: task_id.clone(),
                task_type: "workflow_action".to_string(),
                payload: json!({
                    "action": action.action,
                    "action_id": action.action_id,
                    "workflow_run_id": run.run_id,
                    "workflow_id": run.workflow_id,
                    "task_id": action.task_id.clone().or(run.task_id.clone()),
                }),
                status: status.clone(),
                workflow_id: Some(run.workflow_id.clone()),
                workflow_node_id: Some(action.action_id.clone()),
                parent_task_id: run.task_id.clone(),
                depends_on_task_ids: Vec::new(),
                decision_ids: Vec::new(),
                artifact_ids: artifact_ids.clone(),
                assigned_agent: None,
                priority: 0,
                attempt: 0,
                max_attempts: 1,
                last_error: action.detail.clone(),
                next_retry_at_ms: None,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: 1,
                created_ts: run.created_at_ms.max(now),
                updated_ts: action.updated_at_ms.max(now),
            };
            let _ = context_run_engine()
                .commit_task_mutation(
                    state,
                    &run_id,
                    task.clone(),
                    ContextBlackboardPatchOp::AddTask,
                    serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    "context.task.created".to_string(),
                    workflow_run_status_to_context(&run.status),
                    None,
                    json!({
                        "task_id": task.id,
                        "task_type": task.task_type,
                        "task_rev": task.task_rev,
                        "source": "workflow_runtime",
                    }),
                )
                .await?;
            run_state.tasks.push(task.clone());
        }

        if let Some(output) = action.output.clone() {
            let artifact_exists = blackboard.artifacts.iter().any(|row| row.id == artifact_id);
            if !artifact_exists {
                let artifact = ContextBlackboardArtifact {
                    id: artifact_id.clone(),
                    ts_ms: action.updated_at_ms.max(now),
                    path: format!(
                        "workflow://{}/{}/{}",
                        run.workflow_id, run.run_id, action.action_id
                    ),
                    artifact_type: "workflow_action_output".to_string(),
                    step_id: Some(task_id.clone()),
                    source_event_id: run.source_event_id.clone(),
                };
                let _ = context_run_engine()
                    .commit_blackboard_patch(
                        state,
                        &run_id,
                        ContextBlackboardPatchOp::AddArtifact,
                        serde_json::to_value(&artifact)
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    )
                    .await?;
                blackboard.artifacts.push(artifact);
                let _ = output;
            }
        }
    }

    Ok(run_id)
}

pub(super) fn automation_node_task_status(
    run: &crate::AutomationV2RunRecord,
    node_id: &str,
    depends_on: &[String],
) -> ContextBlackboardTaskStatus {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|row| row == node_id);
    if completed {
        return ContextBlackboardTaskStatus::Done;
    }
    if matches!(
        run.status,
        crate::AutomationRunStatus::Cancelled | crate::AutomationRunStatus::Failed
    ) {
        return ContextBlackboardTaskStatus::Failed;
    }
    let deps_done = depends_on.iter().all(|dep_task_id| {
        let dep_node_id = dep_task_id.strip_prefix("node-").unwrap_or(dep_task_id);
        run.checkpoint
            .completed_nodes
            .iter()
            .any(|row| row == dep_node_id)
    });
    if !deps_done {
        return ContextBlackboardTaskStatus::Blocked;
    }
    if matches!(
        run.status,
        crate::AutomationRunStatus::Paused | crate::AutomationRunStatus::Pausing
    ) {
        return ContextBlackboardTaskStatus::Blocked;
    }
    if run
        .checkpoint
        .pending_nodes
        .iter()
        .any(|row| row == node_id)
    {
        return ContextBlackboardTaskStatus::Runnable;
    }
    ContextBlackboardTaskStatus::Pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn automation_node_task_payload_includes_repair_guidance_from_output() {
        let node = crate::AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "title": "Research Brief",
                    "output_path": "marketing-brief.md"
                }
            })),
        };
        let output = json!({
            "status": "needs_repair",
            "failure_kind": "research_missing_reads",
            "validator_summary": {
                "reason": "research brief did not read concrete workspace files, so source-backed validation is incomplete",
                "unmet_requirements": ["no_concrete_reads"]
            },
            "artifact_validation": {
                "blocking_classification": "tool_available_but_not_used",
                "required_next_tool_actions": [
                    "Use `read` on concrete workspace files before finalizing the brief."
                ],
                "repair_attempt": 1,
                "repair_attempts_remaining": 2
            }
        });

        let payload = automation_node_task_payload(&node, Some(&output));

        assert_eq!(
            payload.get("node_status").and_then(Value::as_str),
            Some("needs_repair")
        );
        assert_eq!(
            payload.get("failure_kind").and_then(Value::as_str),
            Some("research_missing_reads")
        );
        assert_eq!(
            payload.get("validator_reason").and_then(Value::as_str),
            Some(
                "research brief did not read concrete workspace files, so source-backed validation is incomplete"
            )
        );
        assert_eq!(
            payload
                .get("blocking_classification")
                .and_then(Value::as_str),
            Some("tool_available_but_not_used")
        );
        assert_eq!(
            payload
                .get("required_next_tool_actions")
                .and_then(Value::as_array)
                .and_then(|rows| rows.first())
                .and_then(Value::as_str),
            Some("Use `read` on concrete workspace files before finalizing the brief.")
        );
        assert_eq!(
            payload.get("repair_attempt").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            payload
                .get("repair_attempts_remaining")
                .and_then(Value::as_u64),
            Some(2)
        );
    }

    #[test]
    fn automation_node_task_payload_includes_knowledge_preflight_reasons() {
        let node = crate::AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "title": "Research Brief",
                    "output_path": "marketing-brief.md"
                }
            })),
        };
        let output = json!({
            "status": "needs_repair",
            "knowledge_preflight": {
                "decision": "reuse_promoted",
                "coverage_key": "project::marketing::research::brief",
                "reuse_reason": "reusing 2 promoted knowledge item(s) from 1 space(s)",
                "skip_reason": null,
                "freshness_reason": null,
                "items": []
            }
        });

        let payload = automation_node_task_payload(&node, Some(&output));

        assert_eq!(
            payload
                .get("knowledge_preflight")
                .and_then(|value| value.get("coverage_key"))
                .and_then(Value::as_str),
            Some("project::marketing::research::brief")
        );
        assert_eq!(
            payload
                .get("knowledge_reuse_reason")
                .and_then(Value::as_str),
            Some("reusing 2 promoted knowledge item(s) from 1 space(s)")
        );
        assert_eq!(
            payload.get("knowledge_skip_reason").and_then(Value::as_str),
            None
        );
        assert_eq!(
            payload
                .get("knowledge_freshness_reason")
                .and_then(Value::as_str),
            None
        );
    }
}
