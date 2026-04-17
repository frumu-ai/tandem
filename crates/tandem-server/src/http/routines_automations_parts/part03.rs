
async fn automation_v2_task_reset_preview(
    state: &AppState,
    run_id: &str,
    node_id: &str,
) -> Result<AutomationV2TaskResetPreview, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_V2_NOT_FOUND",
                "automationID": current.automation_id
            })),
        ));
    };
    if !automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == node_id)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Node not found",
                "code":"AUTOMATION_V2_TASK_NODE_NOT_FOUND",
                "nodeID": node_id
            })),
        ));
    }
    let roots = std::iter::once(node_id.to_string()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
    let mut reset_nodes_list = reset_nodes.iter().cloned().collect::<Vec<_>>();
    reset_nodes_list.sort();
    let mut cleared_outputs = automation
        .flow
        .nodes
        .iter()
        .filter(|node| reset_nodes.contains(&node.node_id))
        .filter_map(crate::automation_node_required_output_path)
        .collect::<Vec<_>>();
    cleared_outputs.sort();
    cleared_outputs.dedup();
    Ok(AutomationV2TaskResetPreview {
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
        reset_nodes: reset_nodes_list,
        cleared_outputs,
        preserves_upstream_outputs: true,
    })
}

async fn load_automation_v2_backlog_task(
    state: &AppState,
    run_id: &str,
    task_id: &str,
) -> Result<crate::http::context_types::ContextBlackboardTask, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_automation_v2_run(run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    let blackboard = super::context_runs::load_projected_context_blackboard(state, &context_run_id);
    let Some(task) = blackboard.tasks.into_iter().find(|task| task.id == task_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Backlog task not found",
                "code":"AUTOMATION_V2_BACKLOG_TASK_NOT_FOUND",
                "taskID": task_id
            })),
        ));
    };
    if task.task_type != "automation_backlog_item" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Task is not a projected backlog item",
                "code":"AUTOMATION_V2_BACKLOG_TASK_INVALID_TYPE",
                "taskID": task_id
            })),
        ));
    }
    Ok(task)
}

fn automation_v2_backlog_claim_agent(
    task: &crate::http::context_types::ContextBlackboardTask,
    requested_agent_id: Option<String>,
) -> String {
    requested_agent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            task.payload
                .get("task_owner")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| task.assigned_agent.clone())
        .unwrap_or_else(|| "backlog-worker".to_string())
}

pub(super) async fn automations_v2_run_task_reset_preview(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let preview = automation_v2_task_reset_preview(&state, &run_id, &node_id).await?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "preview": preview,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_task_continue(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Run must be blocked, paused, failed, awaiting approval, completed, or cancelled before continue",
                "code":"AUTOMATION_V2_RUN_TASK_NOT_CONTINUEABLE",
                "runID": run_id
            })),
        ));
    }
    let is_blocked = automation_v2_blocked_node_ids(&current)
        .iter()
        .any(|blocked| blocked == &node_id);
    if !is_blocked {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Task is not blocked",
                "code":"AUTOMATION_V2_TASK_NOT_BLOCKED",
                "nodeID": node_id
            })),
        ));
    }
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_V2_NOT_FOUND",
                "automationID": current.automation_id
            })),
        ));
    };
    if !automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == node_id)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Node not found",
                "code":"AUTOMATION_V2_TASK_NODE_NOT_FOUND",
                "nodeID": node_id
            })),
        ));
    }
    let reset_nodes = std::iter::once(node_id.clone()).collect::<std::collections::HashSet<_>>();
    let cleared_outputs =
        crate::clear_automation_subtree_outputs(&state, &automation, &run_id, &reset_nodes)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code":"AUTOMATION_V2_TASK_CONTINUE_OUTPUT_CLEAR_FAILED"
                    })),
                )
            })?;
    let reason = reason_or_default(
        input.reason,
        &format!("continued blocked task `{}` with minimal reset", node_id),
    );
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            clear_automation_run_execution_handles(run);
            run.checkpoint.node_outputs.remove(&node_id);
            run.checkpoint.node_attempts.remove(&node_id);
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| completed_id != &node_id);
            if !run
                .checkpoint
                .pending_nodes
                .iter()
                .any(|pending| pending == &node_id)
            {
                run.checkpoint.pending_nodes.push(node_id.clone());
            }
            run.checkpoint.pending_nodes.sort();
            run.checkpoint.pending_nodes.dedup();
            if run
                .checkpoint
                .last_failure
                .as_ref()
                .map(|failure| failure.node_id == node_id)
                .unwrap_or(false)
            {
                run.checkpoint.last_failure = None;
            }
            run.automation_snapshot = Some(automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                "run_task_continued",
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": vec![node_id.clone()],
                    "cleared_outputs": cleared_outputs,
                    "mode": "minimal_reset",
                })),
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Run update failed",
                    "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"
                })),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "node_id": node_id, "reset_nodes": vec![node_id], "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_task_retry(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let reason = reason_or_default(
        input.reason,
        &format!("retried task `{}` and reset affected subtree", node_id),
    );
    let (automation, updated, cleared_outputs, reset_nodes) =
        automation_v2_reset_task_subtree(&state, &run_id, &node_id, reason, "run_task_retried")
            .await?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "node_id": node_id, "reset_nodes": reset_nodes, "cleared_outputs": cleared_outputs, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_task_requeue(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let reason = reason_or_default(
        input.reason,
        &format!("requeued task `{}` and reset affected subtree", node_id),
    );
    let (automation, updated, cleared_outputs, reset_nodes) =
        automation_v2_reset_task_subtree(&state, &run_id, &node_id, reason, "run_task_requeued")
            .await?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "node_id": node_id, "reset_nodes": reset_nodes, "cleared_outputs": cleared_outputs, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_backlog_task_claim(
    State(state): State<AppState>,
    Path((run_id, task_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2BacklogClaimInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let task_id = task_id.trim().to_string();
    if task_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"task_id is required",
                "code":"AUTOMATION_V2_BACKLOG_TASK_REQUIRED"
            })),
        ));
    }
    let task = load_automation_v2_backlog_task(&state, &run_id, &task_id).await?;
    let agent_id = automation_v2_backlog_claim_agent(&task, input.agent_id);
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    let command_id = Some(format!(
        "automation-v2-backlog-claim:{run_id}:{task_id}:{agent_id}"
    ));
    let claimed = super::context_runs::claim_context_task_by_id(
        &state,
        &context_run_id,
        &task_id,
        &agent_id,
        input.lease_ms,
        command_id,
    )
    .await
    .map_err(|status| {
        (
            status,
            Json(json!({
                "error":"Backlog claim failed",
                "code":"AUTOMATION_V2_BACKLOG_TASK_CLAIM_FAILED",
                "taskID": task_id
            })),
        )
    })?;
    let Some(task) = claimed else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Backlog task is not claimable",
                "code":"AUTOMATION_V2_BACKLOG_TASK_NOT_CLAIMABLE",
                "taskID": task_id
            })),
        ));
    };
    let blackboard =
        super::context_runs::load_projected_context_blackboard(&state, &context_run_id);
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "agent_id": agent_id,
        "reason": reason_or_default(
            input.reason,
            &format!("claimed backlog task `{}`", task_id),
        ),
        "blackboard": blackboard,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_backlog_task_requeue(
    State(state): State<AppState>,
    Path((run_id, task_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let task_id = task_id.trim().to_string();
    if task_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"task_id is required",
                "code":"AUTOMATION_V2_BACKLOG_TASK_REQUIRED"
            })),
        ));
    }
    let task = load_automation_v2_backlog_task(&state, &run_id, &task_id).await?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    let reason = reason_or_default(
        input.reason,
        &format!("requeued backlog task `{}`", task_id),
    );
    let requeued = super::context_runs::requeue_context_task_by_id(
        &state,
        &context_run_id,
        &task_id,
        Some(format!("automation-v2-backlog-requeue:{run_id}:{task_id}")),
        Some(reason.clone()),
    )
    .await
    .map_err(|status| {
        (
            status,
            Json(json!({
                "error":"Backlog requeue failed",
                "code":"AUTOMATION_V2_BACKLOG_TASK_REQUEUE_FAILED",
                "taskID": task_id
            })),
        )
    })?;
    let Some(task) = requeued else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Backlog task is not requeueable",
                "code":"AUTOMATION_V2_BACKLOG_TASK_NOT_REQUEUEABLE",
                "taskID": task_id,
                "status": task.status,
            })),
        ));
    };
    let blackboard =
        super::context_runs::load_projected_context_blackboard(&state, &context_run_id);
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "reason": reason,
        "blackboard": blackboard,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_events(
    State(state): State<AppState>,
    Query(query): Query<AutomationEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "automations_v2",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("automation.v2.") {
                return None;
            }
            if let Some(automation_id) = query.automation_id.as_deref() {
                let value = event
                    .properties
                    .get("automationID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if value != automation_id {
                    return None;
                }
            }
            if let Some(run_id) = query.run_id.as_deref() {
                let value = event
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if value != run_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    Sse::new(ready.chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_v2_node_repair_guidance_includes_knowledge_preflight_reasons() {
        let output = json!({
            "status": "needs_repair",
            "failure_kind": "knowledge_refresh_required",
            "knowledge_preflight": {
                "decision": "refresh_required",
                "coverage_key": "project::ops::workflow::incident-response",
                "reuse_reason": null,
                "skip_reason": "prior knowledge exists but is not fresh enough to reuse",
                "freshness_reason": "coverage `project::ops::workflow::incident-response` in space `project-default` expired at 1234",
                "items": []
            }
        });

        let guidance = automation_v2_node_repair_guidance(&output).expect("guidance");

        assert_eq!(
            guidance
                .get("knowledgePreflight")
                .and_then(|value| value.get("coverage_key"))
                .and_then(Value::as_str),
            Some("project::ops::workflow::incident-response")
        );
        assert_eq!(
            guidance.get("knowledgeSkipReason").and_then(Value::as_str),
            Some("prior knowledge exists but is not fresh enough to reuse")
        );
        assert_eq!(
            guidance
                .get("knowledgeFreshnessReason")
                .and_then(Value::as_str),
            Some(
                "coverage `project::ops::workflow::incident-response` in space `project-default` expired at 1234"
            )
        );
    }

    #[test]
    fn automation_v2_node_repair_guidance_includes_exact_required_source_reads() {
        let output = json!({
            "status": "needs_repair",
            "validator_summary": {
                "reason": "research completed without reading the exact required source files",
                "unmet_requirements": ["required_source_paths_not_read"]
            },
            "artifact_validation": {
                "blocking_classification": "tool_available_but_not_used",
                "required_next_tool_actions": [
                    "Use `read` on the exact required source files before finalizing: RESUME.md, docs/resume.md. Similar backup or copy filenames do not satisfy the requirement."
                ],
                "validation_basis": {
                    "authority": "filesystem_and_receipts",
                    "required_source_read_paths": ["RESUME.md", "docs/resume.md"],
                    "missing_required_source_read_paths": ["RESUME.md", "docs/resume.md"]
                }
            }
        });

        let guidance = automation_v2_node_repair_guidance(&output).expect("guidance");

        assert_eq!(
            guidance
                .get("requiredSourceReadPaths")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str),
            Some("RESUME.md")
        );
        assert_eq!(
            guidance
                .get("missingRequiredSourceReadPaths")
                .and_then(Value::as_array)
                .and_then(|values| values.get(1))
                .and_then(Value::as_str),
            Some("docs/resume.md")
        );
    }

    #[test]
    fn automation_v2_node_repair_guidance_includes_upstream_synthesis_paths() {
        let output = json!({
            "status": "needs_repair",
            "validator_summary": {
                "reason": "final artifact does not adequately synthesize the available upstream evidence",
                "unmet_requirements": ["upstream_evidence_not_synthesized"]
            },
            "artifact_validation": {
                "blocking_classification": "artifact_contract_unmet",
                "required_next_tool_actions": [
                    "Read and synthesize the strongest upstream artifacts before finalizing: .tandem/runs/run-1/artifacts/collect-inputs.json, .tandem/runs/run-1/artifacts/analyze-findings.md. Rewrite the final report as a substantive multi-section synthesis that reuses the concrete terminology, named entities, objections, risks, and proof points already present upstream, and mention at least 2 distinct upstream evidence anchors in the body."
                ],
                "validation_basis": {
                    "authority": "filesystem_and_receipts",
                    "upstream_read_paths": [
                        ".tandem/runs/run-1/artifacts/collect-inputs.json",
                        ".tandem/runs/run-1/artifacts/analyze-findings.md"
                    ]
                }
            }
        });

        let guidance = automation_v2_node_repair_guidance(&output).expect("guidance");

        assert_eq!(
            guidance
                .get("upstreamReadPaths")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str),
            Some(".tandem/runs/run-1/artifacts/collect-inputs.json")
        );
    }

    #[test]
    fn shared_context_pack_ids_extracts_binding_shapes_and_dedupes() {
        let metadata = json!({
            "shared_context_bindings": [
                { "pack_id": "context-pack-a", "required": true },
                { "packId": "context-pack-b", "required": false },
                "context-pack-c",
                { "context_pack_id": "context-pack-a" }
            ],
            "shared_context_pack_ids": [
                "context-pack-d",
                "context-pack-b"
            ]
        });

        let pack_ids =
            crate::http::context_packs::shared_context_pack_ids_from_metadata(Some(&metadata));

        assert_eq!(
            pack_ids,
            vec![
                "context-pack-a".to_string(),
                "context-pack-b".to_string(),
                "context-pack-c".to_string(),
                "context-pack-d".to_string(),
            ]
        );
    }
}
