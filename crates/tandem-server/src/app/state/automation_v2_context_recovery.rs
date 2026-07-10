use super::*;

const AUTOMATION_V2_CONTEXT_RUN_PREFIX: &str = "automation-v2-";

pub(super) async fn get_recovered_automation_v2_run(
    state: &AppState,
    run_id: &str,
) -> Option<AutomationV2RunRecord> {
    let mut context_ids = Vec::new();
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with(AUTOMATION_V2_CONTEXT_RUN_PREFIX) {
        context_ids.push(trimmed.to_string());
    }
    context_ids.push(format!("{AUTOMATION_V2_CONTEXT_RUN_PREFIX}{trimmed}"));

    for root in automation_v2_context_run_roots(state) {
        for context_id in &context_ids {
            let path = root.join(context_id).join("run_state.json");
            if let Some(run) = load_recovered_automation_v2_context_run(&path, 0).await {
                return Some(run);
            }
        }
    }
    None
}

pub(super) async fn list_recovered_automation_v2_runs(
    state: &AppState,
) -> Vec<AutomationV2RunRecord> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for root in automation_v2_context_run_roots(state) {
        let Ok(mut entries) = fs::read_dir(root).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if !entry
                .file_type()
                .await
                .map(|kind| kind.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let context_run_id = entry.file_name().to_string_lossy().to_string();
            if !context_run_id.starts_with(AUTOMATION_V2_CONTEXT_RUN_PREFIX)
                || !seen.insert(context_run_id)
            {
                continue;
            }
            let path = entry.path().join("run_state.json");
            let modified_at_ms = file_modified_at_ms(&path).await;
            candidates.push((modified_at_ms, path));
        }
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    let mut out = Vec::new();
    for (modified_at_ms, path) in candidates {
        if let Some(run) = load_recovered_automation_v2_context_run(&path, modified_at_ms).await {
            out.push(run);
        }
    }
    out
}

pub(super) async fn merge_recovered_automation_v2_runs(
    state: &AppState,
    merged: &mut std::collections::HashMap<String, AutomationV2RunRecord>,
) -> usize {
    let mut recovered_count = 0usize;
    for recovered_run in list_recovered_automation_v2_runs(state).await {
        match merged.get(&recovered_run.run_id) {
            Some(existing) if existing.updated_at_ms >= recovered_run.updated_at_ms => {}
            _ => {
                merged.insert(recovered_run.run_id.clone(), recovered_run);
                recovered_count += 1;
            }
        }
    }
    recovered_count
}

async fn load_recovered_automation_v2_context_run(
    path: &Path,
    fallback_updated_at_ms: u64,
) -> Option<AutomationV2RunRecord> {
    let raw = fs::read_to_string(path).await.ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    if value.get("run_type").and_then(Value::as_str).map(str::trim) != Some("automation_v2") {
        return None;
    }

    let context_run_id = value.get("run_id").and_then(Value::as_str)?.trim();
    let run_id = context_run_id
        .strip_prefix(AUTOMATION_V2_CONTEXT_RUN_PREFIX)
        .unwrap_or(context_run_id)
        .to_string();
    if run_id.trim().is_empty() {
        return None;
    }

    let tenant_context = value
        .get("tenant_context")
        .cloned()
        .and_then(|value| serde_json::from_value::<TenantContext>(value).ok())
        .unwrap_or_else(TenantContext::local_implicit);
    let automation_id = find_first_string_key(&value, &["automation_id", "workflow_id"])
        .unwrap_or_else(|| format!("recovered-{run_id}"));
    let created_at_ms = value
        .get("created_at_ms")
        .and_then(Value::as_u64)
        .unwrap_or(fallback_updated_at_ms);
    let updated_at_ms = value
        .get("updated_at_ms")
        .and_then(Value::as_u64)
        .unwrap_or(fallback_updated_at_ms.max(created_at_ms));
    let status = automation_status_from_context_value(value.get("status").and_then(Value::as_str));
    if !automation_run_is_terminal(&status) {
        return None;
    }
    let finished_at_ms = value
        .get("ended_at_ms")
        .and_then(Value::as_u64)
        .or_else(|| automation_run_is_terminal(&status).then_some(updated_at_ms));
    let checkpoint = recovered_checkpoint_from_context_run(&value);
    let automation_snapshot = recovered_automation_snapshot(
        &value,
        &automation_id,
        &tenant_context,
        created_at_ms,
        updated_at_ms,
    );

    let mut run = AutomationV2RunRecord {
        run_id,
        automation_id,
        tenant_context,
        trigger_type: "recovered_context_run".to_string(),
        status,
        created_at_ms,
        updated_at_ms,
        started_at_ms: value.get("started_at_ms").and_then(Value::as_u64),
        finished_at_ms,
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint,
        runtime_context: None,
        automation_snapshot: Some(automation_snapshot),
        workflow_definition_version: None,
        workflow_definition_snapshot_hash: None,
        execution_claim: None,
        execution_claim_epoch: 0,
        pause_reason: None,
        resume_reason: None,
        detail: value
            .get("last_error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string),
        stop_kind: None,
        stop_reason: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
        scheduler: None,
        trigger_reason: None,
        consumed_handoff_id: None,
        learning_summary: None,
        effective_execution_profile:
            crate::automation_v2::execution_profile::ExecutionProfile::Strict,
        requested_execution_profile: None,
    };
    crate::stateful_runtime::ensure_automation_run_definition_metadata(&mut run);
    Some(run)
}

fn recovered_checkpoint_from_context_run(value: &Value) -> AutomationRunCheckpoint {
    let mut completed_nodes = Vec::new();
    let mut pending_nodes = Vec::new();
    let mut blocked_nodes = Vec::new();

    if let Some(steps) = value.get("steps").and_then(Value::as_array) {
        for step in steps {
            let Some(step_id) = step
                .get("step_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            match step_status_str(step.get("status").and_then(Value::as_str)).as_str() {
                "done" | "completed" | "complete" => completed_nodes.push(step_id.to_string()),
                "blocked" | "failed" | "error" => blocked_nodes.push(step_id.to_string()),
                _ => pending_nodes.push(step_id.to_string()),
            }
        }
    }

    AutomationRunCheckpoint {
        completed_nodes,
        pending_nodes,
        node_outputs: std::collections::HashMap::new(),
        node_attempts: std::collections::HashMap::new(),
        node_attempt_verdicts: std::collections::HashMap::new(),
        blocked_nodes,
        awaiting_gate: None,
        gate_history: Vec::new(),
        lifecycle_history: Vec::new(),
        last_failure: None,
    }
}

fn recovered_automation_snapshot(
    value: &Value,
    automation_id: &str,
    tenant_context: &TenantContext,
    created_at_ms: u64,
    updated_at_ms: u64,
) -> AutomationV2Spec {
    let objective = value
        .get("objective")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("Recovered automation run");
    let nodes = recovered_nodes_from_context_run(value);
    let workspace_root = value
        .get("workspace")
        .and_then(|workspace| workspace.get("canonical_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string);

    AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: objective.to_string(),
        description: Some(objective.to_string()),
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![AutomationAgentProfile {
            agent_id: "recovered".to_string(),
            template_id: None,
            display_name: "Recovered".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec { nodes },
        execution: AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms,
        updated_at_ms,
        creator_id: "context-run-recovery".to_string(),
        workspace_root,
        metadata: Some(json!({
            "recovered_from": "context_run",
            AUTOMATION_TENANT_CONTEXT_METADATA_KEY: tenant_context,
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn recovered_nodes_from_context_run(value: &Value) -> Vec<AutomationFlowNode> {
    let mut nodes = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    if let Some(steps) = value.get("steps").and_then(Value::as_array) {
        for step in steps {
            let Some(node_id) = step
                .get("step_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            if !seen.insert(node_id.to_string()) {
                continue;
            }
            let objective = step
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or(node_id);
            nodes.push(recovered_node(node_id, objective));
        }
    }

    if nodes.is_empty() {
        if let Some(tasks) = value.get("tasks").and_then(Value::as_array) {
            for task in tasks {
                let payload = task.get("payload").unwrap_or(task);
                let Some(node_id) = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                if !seen.insert(node_id.to_string()) {
                    continue;
                }
                let objective = payload
                    .get("title")
                    .or_else(|| payload.get("name"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .unwrap_or(node_id);
                nodes.push(recovered_node(node_id, objective));
            }
        }
    }

    nodes
}

fn recovered_node(node_id: &str, objective: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        node_id: node_id.to_string(),
        agent_id: "recovered".to_string(),
        objective: objective.to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
        metadata: Some(json!({ "recovered_from": "context_run" })),
    }
}

fn automation_status_from_context_value(raw: Option<&str>) -> AutomationRunStatus {
    match step_status_str(raw).as_str() {
        "queued" => AutomationRunStatus::Queued,
        "running" | "planning" | "in_progress" | "in-progress" | "active" => {
            AutomationRunStatus::Running
        }
        "paused" => AutomationRunStatus::Paused,
        "awaiting_approval" | "pending_approval" => AutomationRunStatus::AwaitingApproval,
        "completed" | "complete" | "done" => AutomationRunStatus::Completed,
        "blocked" => AutomationRunStatus::Blocked,
        "failed" | "error" => AutomationRunStatus::Failed,
        "cancelled" | "canceled" => AutomationRunStatus::Cancelled,
        _ => AutomationRunStatus::Running,
    }
}

fn step_status_str(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("pending")
        .to_ascii_lowercase()
}

fn find_first_string_key(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    return Some(text.to_string());
                }
            }
            for child in map.values() {
                if let Some(found) = find_first_string_key(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(rows) => rows
            .iter()
            .find_map(|child| find_first_string_key(child, keys)),
        _ => None,
    }
}

fn automation_v2_context_run_roots(state: &AppState) -> Vec<PathBuf> {
    let data_root = context_runs_data_root_from_state(state);
    let mut roots = vec![data_root.join("hot")];
    let legacy_root = data_root
        .parent()
        .and_then(|data_dir| data_dir.parent())
        .map(|root| root.join("context_runs"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("context_runs"));
    if !roots.contains(&legacy_root) {
        roots.push(legacy_root);
    }
    roots
}

fn context_runs_data_root_from_state(state: &AppState) -> PathBuf {
    if let Some(parent) = state.shared_resources_path.parent() {
        if parent.file_name().and_then(|value| value.to_str()) == Some("system") {
            if let Some(data_dir) = parent.parent() {
                return data_dir.join("context-runs");
            }
        }
        return parent.join("context-runs");
    }
    PathBuf::from(".tandem").join("data").join("context-runs")
}

async fn file_modified_at_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .await
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| {
            modified
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .ok()
        })
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}
