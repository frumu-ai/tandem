// Continuation of routines_automations handlers (split from part02.rs to satisfy
// the 2000-line file-size gate). Included into the same module via routines_automations.rs.


pub(super) async fn automations_v2_pause(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_owner_or_admin(&automation, verified_tenant_context.as_ref().map(|context| &context.0))?;
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        &state,
        &tenant_context,
        &id,
        &actor,
        state.can_mutate_automation(&id, &actor, false).await,
    )
    .await?;
    automation.status = AutomationV2Status::Paused;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    let reason = reason_or_default(input.reason, "paused by operator");
    let runs = state.list_automation_v2_runs(Some(&id), 100).await;
    for run in runs {
        if run.status == AutomationRunStatus::Running {
            let session_ids = run.active_session_ids.clone();
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Pausing;
                    row.pause_reason = Some(reason.clone());
                })
                .await;
            for session_id in run.active_session_ids {
                let _ = state.cancellations.cancel(&session_id).await;
            }
            for instance_id in run.active_instance_ids {
                let _ = state
                    .agent_teams
                    .cancel_instance(&state, &instance_id, "paused by operator")
                    .await;
            }
            state.forget_automation_v2_sessions(&session_ids).await;
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Paused;
                    row.active_session_ids.clear();
                    row.active_instance_ids.clear();
                    crate::record_automation_lifecycle_event(
                        row,
                        "run_paused",
                        row.pause_reason.clone(),
                        None,
                    );
                })
                .await;
        }
    }
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.paused",
        &tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone()),
        json!({
            "automationID": id,
            "reason": reason,
            "automation": stored.clone(),
        }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

pub(super) async fn automations_v2_resume(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_owner_or_admin(&automation, verified_tenant_context.as_ref().map(|context| &context.0))?;
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        &state,
        &tenant_context,
        &id,
        &actor,
        state.can_mutate_automation(&id, &actor, false).await,
    )
    .await?;
    automation.status = AutomationV2Status::Active;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.resumed",
        &tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone()),
        json!({
            "automationID": id,
            "automation": stored.clone(),
        }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

/// GET /automations/v2/{id}/handoffs
///
/// Returns the inbox, approved, and archived handoff artifacts for a given automation.
/// Scans the directories defined in the automation's `handoff_config` (or defaults)
/// relative to the automation's `workspace_root`.
///
/// Response shape:
/// ```json
/// { "inbox": [...], "approved": [...], "archived": [...],
///   "counts": { "inbox": 0, "approved": 0, "archived": 0 } }
/// ```
pub(super) async fn automations_v2_handoffs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::automation_v2::types::HandoffArtifact;

    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_visible_to_context(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;

    let workspace_root = match automation.workspace_root.as_deref() {
        Some(root) if !root.is_empty() => root.to_string(),
        _ => state.workspace_index.snapshot().await.root,
    };

    let handoff_cfg = automation.effective_handoff_config();
    let root = std::path::Path::new(&workspace_root);

    let inbox_dir = root.join(&handoff_cfg.inbox_dir);
    let approved_dir = root.join(&handoff_cfg.approved_dir);
    let archived_dir = root.join(&handoff_cfg.archived_dir);

    async fn scan_dir(dir: &std::path::Path) -> Vec<HandoffArtifact> {
        if !dir.exists() {
            return vec![];
        }
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return vec![];
        };
        let mut items: Vec<HandoffArtifact> = Vec::new();
        let mut scanned = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            scanned += 1;
            if scanned > 512 {
                break; // cap scan
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = tokio::fs::read(&path).await {
                if let Ok(artifact) = serde_json::from_slice::<HandoffArtifact>(&bytes) {
                    items.push(artifact);
                }
            }
        }
        // Sort oldest-first by created_at_ms
        items.sort_by_key(|a| a.created_at_ms);
        items
    }

    let (inbox, approved, archived) = tokio::join!(
        scan_dir(&inbox_dir),
        scan_dir(&approved_dir),
        scan_dir(&archived_dir),
    );

    let inbox_count = inbox.len();
    let approved_count = approved.len();
    let archived_count = archived.len();

    Ok(Json(json!({
        "automation_id": id,
        "workspace_root": workspace_root,
        "handoff_config": {
            "inbox_dir":    handoff_cfg.inbox_dir,
            "approved_dir": handoff_cfg.approved_dir,
            "archived_dir": handoff_cfg.archived_dir,
            "auto_approve": handoff_cfg.auto_approve,
        },
        "inbox":    inbox,
        "approved": approved,
        "archived": archived,
        "counts": {
            "inbox":    inbox_count,
            "approved": approved_count,
            "archived": archived_count,
            "total":    inbox_count + approved_count + archived_count,
        },
    })))
}

pub(super) async fn automations_v2_runs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_visible_to_context(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
    let limit = query.limit.unwrap_or(50);
    let rows = state.list_automation_v2_runs(Some(&id), limit).await;
    for run in &rows {
        let _ =
            super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, run).await;
    }
    let mut runs = Vec::with_capacity(rows.len());
    for run in &rows {
        runs.push(automation_v2_run_with_context_links(&state, run).await);
    }
    Ok(Json(
        json!({ "automationID": id, "runs": runs, "count": rows.len() }),
    ))
}

pub(super) async fn automations_v2_runs_all(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let candidate_rows = state
        .list_automation_v2_runs(None, limit)
        .await
        .into_iter()
        .filter(|run| super::tenant_matches(&tenant_context, &run.tenant_context))
        .collect::<Vec<_>>();
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let mut rows = Vec::with_capacity(candidate_rows.len());
    for run in candidate_rows {
        if ensure_automation_v2_run_visible_to_context(&state, &run, verified)
            .await
            .is_ok()
        {
            rows.push(run);
        }
    }
    for run in &rows {
        if let Some(automation) = state.get_automation_v2(&run.automation_id).await {
            let _ =
                super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, run)
                    .await;
        }
    }
    let mut runs = Vec::with_capacity(rows.len());
    for run in &rows {
        runs.push(automation_v2_run_with_context_links(&state, run).await);
    }
    Json(json!({ "runs": runs, "count": rows.len() }))
}

pub(super) async fn automations_v2_run_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &run)?;
    let automation = ensure_automation_v2_run_visible_to_context(
        &state,
        &run,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )
    .await?;
    let _ = super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &run).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "run": automation_v2_run_with_context_links(&state, &run).await,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_pause(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    let _automation = ensure_automation_v2_run_owner_or_admin(
        &state,
        &current,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )
    .await?;
    if !matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not pausable", "code":"AUTOMATION_V2_RUN_NOT_PAUSABLE", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let session_ids = current.active_session_ids.clone();
    let instance_ids = current.active_instance_ids.clone();
    let _ = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Paused;
            run.pause_reason = Some(reason.clone());
            run.active_session_ids.clear();
            run.active_instance_ids.clear();
            crate::record_automation_lifecycle_event(
                run,
                "run_pause_requested",
                Some(reason.clone()),
                None,
            );
            crate::record_automation_lifecycle_event(
                run,
                "run_paused",
                run.pause_reason.clone(),
                None,
            );
        })
        .await;
    state.forget_automation_v2_sessions(&session_ids).await;
    spawn_automation_v2_run_cleanup(
        state.clone(),
        session_ids,
        instance_ids,
        "paused by operator",
    );
    let updated = state.get_automation_v2_run(&run_id).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"})),
        )
    })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_resume(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    let _automation = ensure_automation_v2_run_owner_or_admin(
        &state,
        &current,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )
    .await?;
    if current.status != AutomationRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not paused", "code":"AUTOMATION_V2_RUN_NOT_PAUSED", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            crate::record_automation_lifecycle_event(
                run,
                "run_resumed",
                Some(reason.clone()),
                None,
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_cancel(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    let _automation = ensure_automation_v2_run_owner_or_admin(
        &state,
        &current,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )
    .await?;
    if matches!(
        current.status,
        AutomationRunStatus::Cancelled
            | AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run already terminal", "code":"AUTOMATION_V2_RUN_TERMINAL", "runID": run_id}),
            ),
        ));
    }
    let session_ids = current.active_session_ids.clone();
    let instance_ids = current.active_instance_ids.clone();
    state.forget_automation_v2_sessions(&session_ids).await;
    let reason = reason_or_default(input.reason, "cancelled by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Cancelled;
            run.detail = Some(reason.clone());
            run.stop_kind = Some(crate::AutomationStopKind::OperatorStopped);
            run.stop_reason = Some(reason.clone());
            run.active_session_ids.clear();
            run.active_instance_ids.clear();
            crate::record_automation_lifecycle_event(
                run,
                "run_stopped",
                Some(reason.clone()),
                Some(crate::AutomationStopKind::OperatorStopped),
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    spawn_automation_v2_run_cleanup(
        state.clone(),
        session_ids,
        instance_ids,
        "cancelled by operator",
    );
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

/// Axum entrypoint for the approval gate-decision endpoint. Resolves the calling
/// principal into a governance actor before delegating to the shared inner handler,
/// so the human-in-the-loop control is always attributed to a verified decider
/// (GOV-B1).
pub(crate) async fn automations_v2_run_gate_decide(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<AutomationV2GateDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let decider =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    automations_v2_run_gate_decide_inner(
        state,
        tenant_context,
        verified_tenant_context.map(|context| context.0),
        run_id,
        input,
        decider,
    )
    .await
}

/// Shared gate-decision logic used by the HTTP endpoint and the channel
/// interaction handlers. `decider` is the verified actor recording the decision;
/// it MUST be a human (or channel-verified Approve-tier user, which the channel
/// handlers resolve to a human actor). Agents cannot decide their own gates.
pub(crate) async fn automations_v2_run_gate_decide_inner(
    state: AppState,
    tenant_context: TenantContext,
    verified_tenant_context: Option<VerifiedTenantContext>,
    run_id: String,
    input: AutomationV2GateDecisionInput,
    decider: crate::automation_v2::governance::GovernanceActorRef,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // GOV-B1: the approval gate is a human-in-the-loop control. Only a verified
    // human (or a channel-verified Approve-tier user resolved to a human actor)
    // may decide it — an agent must never be able to approve its own run.
    if decider.kind != crate::automation_v2::governance::GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Approval gate decisions require a verified human approver",
                "code": "AUTOMATION_V2_GATE_REQUIRES_HUMAN",
            })),
        ));
    }
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    // GOV-B1/B9: approving a gate is at least as privileged as resuming/cancelling
    // a run, so require owner-or-admin rather than mere read visibility.
    let automation_for_access = ensure_automation_v2_run_owner_or_admin(
        &state,
        &current,
        verified_tenant_context.as_ref(),
    )
    .await?;
    if current.status != AutomationRunStatus::AwaitingApproval {
        // Race UX: when a second surface tries to decide a gate that has just
        // been resolved by another surface (Slack click + control-panel click,
        // etc.), surface the winner's decision so the loser's UI can render
        // "already decided by …" instead of a raw error. The winner's record
        // is the most recently appended gate_history entry.
        let winner = current.checkpoint.gate_history.last();
        let winner_payload = winner.map(|record| {
            json!({
                "node_id": record.node_id,
                "decision": record.decision,
                "reason": record.reason,
                "decided_at_ms": record.decided_at_ms,
            })
        });
        let mut body = json!({
            "error": "Run is not awaiting approval",
            "code": "AUTOMATION_V2_RUN_NOT_AWAITING_APPROVAL",
            "runID": run_id,
            "currentStatus": current.status,
        });
        if let Some(winner_value) = winner_payload {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("winningDecision".to_string(), winner_value);
            }
        }
        return Err((StatusCode::CONFLICT, Json(body)));
    }
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
        .or(Some(automation_for_access))
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let recovered_gate = || {
        let pending_nodes = current
            .checkpoint
            .pending_nodes
            .iter()
            .collect::<std::collections::HashSet<_>>();
        automation
            .flow
            .nodes
            .iter()
            .find(|node| {
                pending_nodes.contains(&node.node_id)
                    && !current
                        .checkpoint
                        .gate_history
                        .iter()
                        .any(|record| record.node_id == node.node_id)
                    && crate::app::state::is_automation_approval_node(node)
            })
            .and_then(crate::app::state::build_automation_pending_gate)
            .map(|mut gate| {
                gate.requested_at_ms = current.updated_at_ms.max(current.created_at_ms);
                gate
            })
    };
    let Some(gate) = current
        .checkpoint
        .awaiting_gate
        .clone()
        .or_else(recovered_gate)
    else {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run has no pending gate", "code":"AUTOMATION_V2_RUN_GATE_MISSING", "runID": run_id}),
            ),
        ));
    };
    let decision = input.decision.trim().to_ascii_lowercase();
    if !["approve", "rework", "cancel"].contains(&decision.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error":"decision must be approve, rework, or cancel", "code":"AUTOMATION_V2_GATE_INVALID_DECISION"}),
            ),
        ));
    }
    let Some(node) = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == gate.node_id)
        .cloned()
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Gate node not found", "code":"AUTOMATION_V2_GATE_NODE_NOT_FOUND", "nodeID": gate.node_id}),
            ),
        ));
    };
    let reason = input
        .reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut winning_decision = None;
    let mut decision_applied = false;
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            match crate::app::state::apply_automation_gate_decision(
                run,
                &automation,
                &gate,
                &decision,
                reason.clone(),
                Some(decider.clone()),
            ) {
                crate::app::state::AutomationGateDecisionOutcome::Applied => {
                    decision_applied = true;
                }
                crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(winner) => {
                    winning_decision = winner;
                }
            }
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    if !decision_applied {
        let winner_payload = winning_decision.map(|record| {
            json!({
                "node_id": record.node_id,
                "decision": record.decision,
                "reason": record.reason,
                "decided_at_ms": record.decided_at_ms,
            })
        });
        let mut body = json!({
            "error": "Run is not awaiting approval",
            "code": "AUTOMATION_V2_RUN_NOT_AWAITING_APPROVAL",
            "runID": run_id,
            "currentStatus": updated.status,
        });
        if let Some(winner_value) = winner_payload {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("winningDecision".to_string(), winner_value);
            }
        }
        return Err((StatusCode::CONFLICT, Json(body)));
    }
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    // GOV-B1/B8: every gate decision (allow path) writes tamper-evident audit
    // evidence attributing WHO decided.
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.gate_decided",
        &current.tenant_context,
        decider
            .actor_id
            .clone()
            .or_else(|| decider.source.clone()),
        json!({
            "runID": run_id.clone(),
            "automationID": automation.automation_id.clone(),
            "nodeID": gate.node_id.clone(),
            "decision": decision.clone(),
            "reason": reason.clone(),
            "decidedBy": decider.clone(),
        }),
    )
    .await;
    state.event_bus.publish(tandem_types::EngineEvent::new(
        "approval.decision.recorded",
        json!({
            "run_id": run_id,
            "automation_id": automation.automation_id.clone(),
            "node_id": gate.node_id.clone(),
            "decision": decision.clone(),
            "reason": reason.clone(),
            "executed_as": "approval_gate",
            "decided_by": decider.clone(),
            "timestamp": crate::now_ms(),
            "tenantContext": current.tenant_context.clone(),
        }),
    ));
    spawn_channel_approval_decision_update(
        state.clone(),
        super::approvals::automation_v2_run_to_approval_request(&current, &gate, None),
        decision.clone(),
        reason.clone(),
    );
    let _ = node;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

fn spawn_channel_approval_decision_update(
    state: AppState,
    request: tandem_types::ApprovalRequest,
    decision: String,
    reason: Option<String>,
) {
    tokio::spawn(async move {
        if let Err(error) = update_channel_approval_decision(state, request, decision, reason).await
        {
            tracing::warn!(
                target: "tandem_server::approval_outbound",
                %error,
                "failed to update channel approval card after gate decision"
            );
        }
    });
}

async fn update_channel_approval_decision(
    state: AppState,
    request: tandem_types::ApprovalRequest,
    decision: String,
    reason: Option<String>,
) -> anyhow::Result<()> {
    let message_map = crate::app::state::approval_message_map::ApprovalMessageMap::load_or_default(
        crate::config::paths::resolve_approval_message_map_path(),
    )
    .await;
    let Some(record) = message_map.get(&request.request_id).await else {
        return Ok(());
    };

    let card = crate::app::notifiers::approval_request_to_card(&request, record.recipient.clone());
    let decided_by_display = format!("{} by Tandem operator", decision_label(&decision));
    let decision_summary = match reason.as_deref().filter(|value| !value.trim().is_empty()) {
        Some(reason) => format!(
            "*{}.*\nReason: {}",
            decision_label(&decision),
            reason.trim()
        ),
        None => format!("*{}.*", decision_label(&decision)),
    };
    let effective = state.config.get_effective_value().await;
    match record.channel.as_str() {
        "slack" => {
            let Some(slack_value) = effective.pointer("/channels/slack").cloned() else {
                return Ok(());
            };
            let cfg: crate::SlackConfigFile = serde_json::from_value(slack_value)?;
            if cfg.bot_token.trim().is_empty() {
                return Ok(());
            }

            let slack_config = tandem_channels::config::SlackConfig {
                bot_token: cfg.bot_token,
                channel_id: record.recipient.clone(),
                allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                    cfg.allowed_users,
                ),
                mention_only: cfg.mention_only,
                security_profile: cfg.security_profile,
            };
            let channel = tandem_channels::slack::SlackChannel::new(slack_config);
            channel
                .update_card_for_decision(
                    &card,
                    &record.message_id,
                    &decided_by_display,
                    &decision_summary,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            send_approval_thread_reply(&channel, &record, &request, &decision, reason.as_deref())
                .await?;
        }
        "discord" => {
            let Some(discord_value) = effective.pointer("/channels/discord").cloned() else {
                return Ok(());
            };
            let cfg: crate::DiscordConfigFile = serde_json::from_value(discord_value)?;
            if cfg.bot_token.trim().is_empty() {
                return Ok(());
            }

            let discord_config = tandem_channels::config::DiscordConfig {
                bot_token: cfg.bot_token,
                guild_id: cfg.guild_id,
                allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                    cfg.allowed_users,
                ),
                mention_only: cfg.mention_only,
                security_profile: cfg.security_profile,
            };
            let channel = tandem_channels::discord::DiscordChannel::new(discord_config);
            channel
                .update_card_for_decision(
                    &card,
                    &record.message_id,
                    discord_decision_outcome(&decision),
                    &decided_by_display,
                    &decision_summary,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            send_approval_thread_reply(&channel, &record, &request, &decision, reason.as_deref())
                .await?;
        }
        "telegram" => {
            let Some(telegram_value) = effective.pointer("/channels/telegram").cloned() else {
                return Ok(());
            };
            let cfg: crate::TelegramConfigFile = serde_json::from_value(telegram_value)?;
            if cfg.bot_token.trim().is_empty() {
                return Ok(());
            }

            let telegram_config = tandem_channels::config::TelegramConfig {
                bot_token: cfg.bot_token,
                allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                    cfg.allowed_users,
                ),
                mention_only: cfg.mention_only,
                style_profile: cfg.style_profile,
                security_profile: cfg.security_profile,
            };
            let channel = tandem_channels::telegram::TelegramChannel::new(telegram_config);
            channel
                .update_card_for_decision(
                    &card,
                    &record.message_id,
                    &decided_by_display,
                    &decision_summary,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            send_approval_thread_reply(&channel, &record, &request, &decision, reason.as_deref())
                .await?;
        }
        _ => {}
    }
    Ok(())
}

async fn send_approval_thread_reply(
    channel: &dyn tandem_channels::traits::Channel,
    record: &crate::app::state::approval_message_map::ApprovalMessageRecord,
    request: &tandem_types::ApprovalRequest,
    decision: &str,
    reason: Option<&str>,
) -> anyhow::Result<()> {
    let thread_id = record
        .thread_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(record.message_id.as_str())
        .to_string();
    let node = request
        .node_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("approval gate");
    let mut content = format!(
        "{} `{}` for run `{}`.",
        decision_label(decision),
        node,
        request.run_id
    );
    if let Some(reason) = reason.map(str::trim).filter(|value| !value.is_empty()) {
        content.push_str(&format!("\nReason: {reason}"));
    }
    channel
        .send_thread_reply(&tandem_channels::traits::ThreadReply {
            content,
            recipient: record.recipient.clone(),
            thread_id,
        })
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn discord_decision_outcome(decision: &str) -> tandem_channels::discord_blocks::DecisionOutcome {
    match decision {
        "approve" => tandem_channels::discord_blocks::DecisionOutcome::Approved,
        "rework" => tandem_channels::discord_blocks::DecisionOutcome::Reworked,
        "cancel" => tandem_channels::discord_blocks::DecisionOutcome::Cancelled,
        _ => tandem_channels::discord_blocks::DecisionOutcome::Cancelled,
    }
}

fn decision_label(decision: &str) -> &'static str {
    match decision {
        "approve" => "Approved",
        "rework" => "Sent back for rework",
        "cancel" => "Cancelled",
        _ => "Decided",
    }
}

pub(super) async fn automations_v2_run_recover(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    let automation = ensure_automation_v2_run_owner_or_admin(
        &state,
        &current,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )
    .await?;
    let blocked_node_ids = automation_v2_blocked_node_ids(&current);
    let blocked_run_is_recoverable = matches!(current.status, AutomationRunStatus::Blocked)
        || (matches!(current.status, AutomationRunStatus::Completed)
            && !blocked_node_ids.is_empty());
    if !matches!(
        current.status,
        AutomationRunStatus::Failed | AutomationRunStatus::Paused
    ) && !blocked_run_is_recoverable
    {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not recoverable", "code":"AUTOMATION_V2_RUN_NOT_RECOVERABLE", "runID": run_id}),
            ),
        ));
    }
    let runtime_context_failure = current.status == AutomationRunStatus::Failed
        && current.detail.as_deref()
            == Some("runtime context partition missing for automation run");
    let reset_nodes = if current.status == AutomationRunStatus::Failed {
        let mut roots = blocked_node_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        if let Some(failure_node_id) = automation_v2_recoverable_failure_node_id(&current) {
            roots.insert(failure_node_id);
        }
        if roots.is_empty() {
            return Err((
                StatusCode::CONFLICT,
                Json(
                    json!({"error":"Run has no recoverable failed node", "code":"AUTOMATION_V2_RUN_FAILURE_CONTEXT_MISSING", "runID": run_id}),
                ),
            ));
        }
        crate::collect_automation_descendants(&automation, &roots)
    } else if blocked_run_is_recoverable {
        if blocked_node_ids.is_empty() {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error":"Run has no recoverable blocked node",
                    "code":"AUTOMATION_V2_RUN_BLOCKED_CONTEXT_MISSING",
                    "runID": run_id
                })),
            ));
        }
        let roots = blocked_node_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        crate::collect_automation_descendants(&automation, &roots)
    } else {
        std::collections::HashSet::new()
    };
    let reset_nodes = reset_nodes
        .into_iter()
        .filter(|node_id| {
            automation
                .flow
                .nodes
                .iter()
                .any(|node| node.node_id == *node_id)
        })
        .collect::<std::collections::HashSet<_>>();
    let reason = if current.status == AutomationRunStatus::Paused {
        reason_or_default(input.reason, "recovered from paused state by operator")
    } else {
        reason_or_default(input.reason, "recovered by operator")
    };
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.checkpoint.awaiting_gate = None;
            clear_automation_run_execution_handles(run);
            if run.pause_reason.as_deref() == Some("stale_no_provider_activity")
                && reset_nodes.is_empty()
            {
                for node_id in run.checkpoint.pending_nodes.clone() {
                    run.checkpoint.node_outputs.remove(&node_id);
                    run.checkpoint.node_attempts.remove(&node_id);
                }
            }
            if !reset_nodes.is_empty() {
                for node_id in &reset_nodes {
                    run.checkpoint.node_outputs.remove(node_id);
                    run.checkpoint.node_attempts.remove(node_id);
                }
                run.checkpoint
                    .blocked_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                run.checkpoint
                    .completed_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                let mut pending = run.checkpoint.pending_nodes.clone();
                for node_id in &reset_nodes {
                    if !pending.iter().any(|existing| existing == node_id) {
                        pending.push(node_id.clone());
                    }
                }
                pending.sort();
                pending.dedup();
                run.checkpoint.pending_nodes = pending;
                run.checkpoint.last_failure = None;
            } else if runtime_context_failure {
                run.checkpoint.last_failure = None;
            }
            crate::record_automation_lifecycle_event(
                run,
                if reset_nodes.is_empty() {
                    "run_recovered_from_pause"
                } else {
                    "run_recovered"
                },
                Some(reason.clone()),
                None,
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_repair(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
    Json(input): Json<AutomationV2RunRepairInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = input.node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error":"node_id is required", "code":"AUTOMATION_V2_REPAIR_NODE_REQUIRED"}),
            ),
        ));
    }
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    let automation_for_access = ensure_automation_v2_run_owner_or_admin(
        &state,
        &current,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )
    .await?;
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run must be paused, failed, awaiting approval, or cancelled before repair", "code":"AUTOMATION_V2_RUN_NOT_REPAIRABLE", "runID": run_id}),
            ),
        ));
    }
    let Some(mut automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
        .or(Some(automation_for_access))
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let Some(node) = automation
        .flow
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Node not found", "code":"AUTOMATION_V2_REPAIR_NODE_NOT_FOUND", "nodeID": node_id}),
            ),
        ));
    };
    let agent_id = node.agent_id.clone();
    let previous_prompt = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("prompt"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let prompt = input
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let template_id = input
        .template_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let model_policy = input.model_policy.clone();
    if let Some(prompt_value) = prompt.as_ref() {
        let metadata = node.metadata.get_or_insert_with(|| json!({}));
        let builder = metadata
            .as_object_mut()
            .and_then(|root| root.entry("builder").or_insert_with(|| json!({})).as_object_mut())
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"Node metadata is not repairable", "code":"AUTOMATION_V2_REPAIR_METADATA_INVALID"})),
                )
            })?;
        builder.insert("prompt".to_string(), Value::String(prompt_value.clone()));
    }
    let previous_agent = automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == agent_id)
        .cloned();
    if template_id.is_some() || model_policy.is_some() {
        let Some(agent) = automation
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(
                    json!({"error":"Node agent not found", "code":"AUTOMATION_V2_REPAIR_AGENT_NOT_FOUND", "agentID": agent_id}),
                ),
            ));
        };
        if let Some(template_value) = template_id.clone() {
            agent.template_id = Some(template_value);
        }
        if let Some(model_policy_value) = model_policy.clone() {
            agent.model_policy = Some(model_policy_value);
        }
    }
    automation.updated_at_ms = crate::now_ms();
    let stored_automation = state.put_automation_v2(automation.clone()).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_REPAIR_PERSIST_FAILED"})),
        )
    })?;
    let roots = std::iter::once(node_id.clone()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&stored_automation, &roots);
    let cleared_outputs = crate::clear_automation_subtree_outputs(
        &state,
        &stored_automation,
        &run_id,
        &reset_nodes,
    )
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_REPAIR_OUTPUT_RESET_FAILED"})),
            )
        })?;
    let reason = reason_or_default(
        input.reason,
        &format!("repaired node `{}` and reset affected subtree", node_id),
    );
    let updated_agent = stored_automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == agent_id)
        .cloned();
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
            for reset_node_id in &reset_nodes {
                run.checkpoint.node_outputs.remove(reset_node_id);
                run.checkpoint.node_attempts.remove(reset_node_id);
            }
            run.checkpoint
                .blocked_nodes
                .retain(|blocked_id| !reset_nodes.contains(blocked_id));
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| !reset_nodes.contains(completed_id));
            let mut pending = run.checkpoint.pending_nodes.clone();
            for reset_node_id in &reset_nodes {
                if !pending.iter().any(|existing| existing == reset_node_id) {
                    pending.push(reset_node_id.clone());
                }
            }
            pending.sort();
            pending.dedup();
            run.checkpoint.pending_nodes = pending;
            run.checkpoint.last_failure = None;
            run.automation_snapshot = Some(stored_automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                "run_step_repaired",
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": reset_nodes.iter().cloned().collect::<Vec<_>>(),
                    "prompt_updated": prompt.is_some(),
                    "template_updated": template_id.is_some(),
                    "model_policy_updated": model_policy.is_some(),
                    "reset_only": prompt.is_none() && template_id.is_none() && model_policy.is_none(),
                    "cleared_outputs": cleared_outputs,
                    "previous_prompt": previous_prompt,
                    "new_prompt": prompt,
                    "previous_template_id": previous_agent.as_ref().and_then(|agent| agent.template_id.clone()),
                    "new_template_id": updated_agent.as_ref().and_then(|agent| agent.template_id.clone()),
                    "previous_model_policy": previous_agent.as_ref().and_then(|agent| agent.model_policy.clone()),
                    "new_model_policy": updated_agent.as_ref().and_then(|agent| agent.model_policy.clone()),
                })),
            );
            crate::refresh_automation_runtime_state(&stored_automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ = super::context_runs::sync_automation_v2_run_blackboard(
        &state,
        &stored_automation,
        &updated,
    )
    .await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "automation": stored_automation, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

async fn automation_v2_reset_task_subtree(
    state: &AppState,
    tenant_context: &TenantContext,
    run_id: &str,
    node_id: &str,
    reason: String,
    lifecycle_event: &str,
) -> Result<
    (
        AutomationV2Spec,
        crate::AutomationV2RunRecord,
        Vec<String>,
        Vec<String>,
    ),
    (StatusCode, Json<Value>),
> {
    let Some(current) = state.get_automation_v2_run(run_id).await else {
        return Err(automation_v2_run_not_found(run_id));
    };
    ensure_automation_v2_run_tenant(tenant_context, &current)?;
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Run must be paused, blocked, failed, awaiting approval, completed, or cancelled before task reset",
                "code":"AUTOMATION_V2_RUN_TASK_NOT_MUTABLE",
                "runID": run_id
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
    let roots = std::iter::once(node_id.to_string()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
    let cleared_outputs =
        crate::clear_automation_subtree_outputs(state, &automation, run_id, &reset_nodes)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code":"AUTOMATION_V2_TASK_RESET_OUTPUT_CLEAR_FAILED"
                    })),
                )
            })?;
    let mut reset_nodes_list = reset_nodes.iter().cloned().collect::<Vec<_>>();
    reset_nodes_list.sort();
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            clear_automation_run_execution_handles(run);
            for reset_node_id in &reset_nodes {
                run.checkpoint.node_outputs.remove(reset_node_id);
                run.checkpoint.node_attempts.remove(reset_node_id);
            }
            run.checkpoint
                .blocked_nodes
                .retain(|blocked_id| !reset_nodes.contains(blocked_id));
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| !reset_nodes.contains(completed_id));
            let mut pending = run.checkpoint.pending_nodes.clone();
            for reset_node_id in &reset_nodes {
                if !pending.iter().any(|existing| existing == reset_node_id) {
                    pending.push(reset_node_id.clone());
                }
            }
            pending.sort();
            pending.dedup();
            run.checkpoint.pending_nodes = pending;
            run.checkpoint.last_failure = None;
            run.automation_snapshot = Some(automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                lifecycle_event,
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": reset_nodes_list.clone(),
                    "cleared_outputs": cleared_outputs.clone(),
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
    Ok((automation, updated, cleared_outputs, reset_nodes_list))
}
