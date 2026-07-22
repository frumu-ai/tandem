// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
    ensure_automation_v2_owner_or_admin(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
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
        state.can_mutate_automation(&id, &actor, false, &tenant_context).await,
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
    crate::audit::append_protected_audit_event(
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
    .await
    .map_err(super::protected_audit_error_response)?;
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
    ensure_automation_v2_owner_or_admin(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
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
        state.can_mutate_automation(&id, &actor, false, &tenant_context).await,
    )
    .await?;
    let before = automation.clone();
    automation.status = AutomationV2Status::Active;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    if let Err(error) = state
        .complete_tenant_ownership_quarantine_restore(
            &id,
            &actor,
            &tenant_context,
        )
        .await
    {
        let _ = state.put_automation_v2(before).await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_GOVERNANCE_QUARANTINE_RESTORE_FAILED",
            })),
        ));
    }
    crate::audit::append_protected_audit_event(
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
    .await
    .map_err(super::protected_audit_error_response)?;
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
            run.pause_reason = None;
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
        let session_ids = current.active_session_ids.clone();
        let instance_ids = current.active_instance_ids.clone();
        state.forget_automation_v2_sessions(&session_ids).await;
        let cleaned = if session_ids.is_empty() && instance_ids.is_empty() {
            current
        } else {
            state
                .update_automation_v2_run(&run_id, |run| {
                    run.active_session_ids.clear();
                    run.active_instance_ids.clear();
                    run.latest_session_id = None;
                })
                .await
                .unwrap_or(current)
        };
        spawn_automation_v2_run_cleanup(
            state.clone(),
            session_ids,
            instance_ids,
            "terminal automation run cleanup",
        );
        let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
        return Ok(Json(json!({
            "ok": true,
            "alreadyTerminal": true,
            "run": automation_v2_run_with_context_links(&state, &cleaned).await,
            "contextRunID": context_run_id,
            "linked_context_run_id": context_run_id,
        })));
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
    let AutomationV2GateDecisionInput {
        decision: input_decision,
        reason: input_reason,
        approval_request_id: requested_approval_request_id,
        transition_id: requested_transition_id,
    } = input;
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err(automation_v2_run_not_found(&run_id));
    };
    ensure_automation_v2_run_tenant(&tenant_context, &current)?;
    // GOV-B1/CT-21: the approval gate is a human-in-the-loop control. Only a
    // verified human (or a channel-verified Approve-tier user resolved to a
    // human actor) may decide it; an agent attempt is rejected and audited.
    if decider.kind != crate::automation_v2::governance::GovernanceActorKind::Human {
        audit_gate_decision_denial(
            &state,
            &current,
            None,
            &decider,
            "AUTOMATION_V2_GATE_REQUIRES_HUMAN",
            "Approval gate decisions require a verified human approver",
        )
        .await?;
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Approval gate decisions require a verified human approver",
                "code": "AUTOMATION_V2_GATE_REQUIRES_HUMAN",
            })),
        ));
    }
    // GOV-B1/B9: approving a gate is at least as privileged as resuming/cancelling
    // a run, so require owner-or-admin rather than mere read visibility.
    let automation_for_access =
        ensure_automation_v2_run_owner_or_admin(&state, &current, verified_tenant_context.as_ref())
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
                    && !crate::app::state::automation_gate_has_settled_decision(
                        &current,
                        &node.node_id,
                    )
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
    let decision = input_decision.trim().to_ascii_lowercase();
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
    let reason = input_reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let now_ms = crate::now_ms();
    if crate::app::state::automation_gate_rejects_late_human_decision(&gate, now_ms) {
        let detail = "Approval gate expired before this decision was submitted";
        audit_gate_decision_denial(
            &state,
            &current,
            Some(&gate),
            &decider,
            "AUTOMATION_V2_GATE_EXPIRED",
            detail,
        )
        .await?;
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": detail,
                "code": "AUTOMATION_V2_GATE_EXPIRED",
                "runID": current.run_id,
                "automationID": current.automation_id,
                "nodeID": gate.node_id,
                "decidedBy": decider,
            })),
        ));
    }
    if let Err((code, detail)) = authorize_gate_decider(
        &current,
        &automation,
        &gate,
        &decision,
        &decider,
        verified_tenant_context.as_ref(),
        now_ms,
    ) {
        audit_gate_decision_denial(&state, &current, Some(&gate), &decider, code, detail).await?;
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": detail,
                "code": code,
                "runID": current.run_id,
                "automationID": current.automation_id,
                "nodeID": gate.node_id,
                "decidedBy": decider,
            })),
        ));
    }
    let mut winning_decision = None;
    let mut decision_applied = false;
    let mut transition_guard_denial = None;
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            match crate::app::state::apply_automation_gate_decision_with_transition_guard(
                run,
                &automation,
                &gate,
                &decision,
                reason.clone(),
                Some(decider.clone()),
                requested_approval_request_id.as_deref(),
                requested_transition_id.as_deref(),
            ) {
                Ok(crate::app::state::AutomationGateDecisionOutcome::Applied) => {
                    decision_applied = true;
                }
                Ok(crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(winner)) => {
                    winning_decision = winner;
                }
                Err(denial) => {
                    transition_guard_denial = Some(denial);
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
    if let Some(denial) = transition_guard_denial {
        record_transition_guard_policy_decision(
            &state,
            &automation,
            &updated,
            &gate,
            &decider,
            &denial,
        )
        .await?;
        audit_gate_decision_denial(
            &state,
            &updated,
            Some(&gate),
            &decider,
            denial.code,
            &denial.detail,
        )
        .await?;
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": denial.detail,
                "code": denial.code,
                "runID": run_id,
                "automationID": automation.automation_id,
                "nodeID": gate.node_id,
                "transitionGuard": denial.metadata,
            })),
        ));
    }
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
    crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.gate_decided",
        &current.tenant_context,
        decider.actor_id.clone().or_else(|| decider.source.clone()),
        json!({
            "runID": run_id.clone(),
            "automationID": automation.automation_id.clone(),
            "nodeID": gate.node_id.clone(),
            "decision": decision.clone(),
            "reason": reason.clone(),
            "decidedBy": decider.clone(),
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)?;
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

fn authorize_gate_decider(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    gate: &crate::AutomationPendingGate,
    decision: &str,
    decider: &crate::automation_v2::governance::GovernanceActorRef,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    now_ms: u64,
) -> Result<(), (&'static str, &'static str)> {
    let policy = GateReviewerPolicy::from_gate(gate, automation);
    if decision == "approve" && policy.is_consequential() {
        if let (Some(requester), Some(reviewer)) = (
            gate_requester_actor_id(run, automation).as_deref(),
            decider.actor_id.as_deref(),
        ) {
            if actor_identity_matches(requester, None, reviewer, decider.source.as_deref()) {
                return Err((
                    "AUTOMATION_V2_GATE_SELF_APPROVAL_FORBIDDEN",
                    "Requester cannot approve their own consequential action",
                ));
            }
        }
    }

    if !policy.requires_reviewer_authority() {
        return Ok(());
    }
    if channel_verified_decider_satisfies_reviewer_authority(decider) {
        return Ok(());
    }
    if run.tenant_context.is_local_implicit() && verified_tenant_context.is_none() {
        return Ok(());
    }
    let Some(strict_context) =
        verified_tenant_context.and_then(|context| context.strict_projection.as_ref())
    else {
        return Err((
            "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED",
            "Reviewer authority could not be verified for this approval",
        ));
    };

    let permissions = policy.required_permissions();
    let data_classes = policy.required_data_classes();
    permissions
        .iter()
        .any(|permission| {
            data_classes.iter().any(|data_class| {
                matches!(
                    strict_context
                        .evaluate_access(&policy.resource, *permission, *data_class, now_ms)
                        .decision,
                    tandem_types::AccessDecision::Allow
                )
            })
        })
        .then_some(())
        .ok_or((
            "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED",
            "Reviewer lacks matching authority for this approval",
        ))
}

fn actor_identity_matches(
    left_actor_id: &str,
    left_source: Option<&str>,
    right_actor_id: &str,
    right_source: Option<&str>,
) -> bool {
    let left = canonical_actor_identity(left_actor_id, left_source);
    let right = canonical_actor_identity(right_actor_id, right_source);
    !left.is_empty() && left == right
}

fn canonical_actor_identity(actor_id: &str, source: Option<&str>) -> String {
    let actor_id = actor_id.trim();
    if actor_id.is_empty() {
        return String::new();
    }
    if actor_id
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("channel:"))
    {
        return actor_id.to_ascii_lowercase();
    }
    if let Some(kind) = channel_kind_from_source(source) {
        return format!("channel:{kind}:{}", actor_id.to_ascii_lowercase());
    }
    actor_id.to_ascii_lowercase()
}

fn channel_kind_from_source(source: Option<&str>) -> Option<&'static str> {
    match source?.trim().to_ascii_lowercase().as_str() {
        "slack" | "channel:slack" => Some("slack"),
        "discord" | "channel:discord" => Some("discord"),
        "telegram" | "channel:telegram" => Some("telegram"),
        _ => None,
    }
}

fn channel_verified_decider_satisfies_reviewer_authority(
    decider: &crate::automation_v2::governance::GovernanceActorRef,
) -> bool {
    decider.kind == crate::automation_v2::governance::GovernanceActorKind::Human
        && decider
            .actor_id
            .as_deref()
            .is_some_and(|actor_id| !actor_id.trim().is_empty())
        && channel_kind_from_source(decider.source.as_deref()).is_some()
}

struct GateReviewerPolicy {
    reviewer_eligibility: tandem_types::ReviewerEligibility,
    risk_tier: Option<tandem_types::ToolRiskTier>,
    data_classes: Vec<tandem_types::DataClass>,
    resource: tandem_types::ResourceRef,
}

impl GateReviewerPolicy {
    fn from_gate(gate: &crate::AutomationPendingGate, automation: &AutomationV2Spec) -> Self {
        let metadata = gate.metadata.as_ref();
        let reviewer_eligibility = metadata
            .and_then(metadata_reviewer_eligibility)
            .unwrap_or(tandem_types::ReviewerEligibility::None);
        let risk_tier = metadata.and_then(metadata_risk_tier);
        let data_classes = metadata.map(metadata_data_classes).unwrap_or_default();
        let resource = metadata
            .and_then(metadata_resource_ref)
            .unwrap_or_else(|| automation_gate_resource_ref(automation, gate));
        Self {
            reviewer_eligibility,
            risk_tier,
            data_classes,
            resource,
        }
    }

    fn is_consequential(&self) -> bool {
        self.reviewer_eligibility != tandem_types::ReviewerEligibility::None
            || self
                .risk_tier
                .map(tandem_types::ToolRiskTier::approval_required_by_default)
                .unwrap_or(false)
            || self
                .data_classes
                .iter()
                .any(|class| tandem_types::ApprovalGateMatrix::data_class_requires_elevated(*class))
    }

    fn requires_reviewer_authority(&self) -> bool {
        self.reviewer_eligibility.requires_elevated() || !self.data_classes.is_empty()
    }

    fn required_permissions(&self) -> Vec<tandem_types::AccessPermission> {
        if self.reviewer_eligibility.requires_elevated() {
            vec![
                tandem_types::AccessPermission::Admin,
                tandem_types::AccessPermission::Delegate,
            ]
        } else {
            vec![
                tandem_types::AccessPermission::View,
                tandem_types::AccessPermission::Read,
            ]
        }
    }

    fn required_data_classes(&self) -> Vec<tandem_types::DataClass> {
        if self.data_classes.is_empty() {
            vec![tandem_types::DataClass::Internal]
        } else {
            self.data_classes.clone()
        }
    }
}

fn gate_requester_actor_id(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    automation: &AutomationV2Spec,
) -> Option<String> {
    run.tenant_context
        .actor_id
        .clone()
        .or_else(|| Some(automation.creator_id.clone()))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn metadata_reviewer_eligibility(value: &Value) -> Option<tandem_types::ReviewerEligibility> {
    metadata_pointer(
        value,
        &["/gate/reviewer_eligibility", "/reviewer_eligibility"],
    )
    .cloned()
    .and_then(|value| serde_json::from_value(value).ok())
}

fn metadata_risk_tier(value: &Value) -> Option<tandem_types::ToolRiskTier> {
    metadata_pointer(
        value,
        &["/gate/risk_tier", "/policy/risk_tier", "/risk_tier"],
    )
    .cloned()
    .and_then(|value| serde_json::from_value(value).ok())
}

fn metadata_data_classes(value: &Value) -> Vec<tandem_types::DataClass> {
    metadata_pointer(
        value,
        &[
            "/gate/data_classes",
            "/policy/data_classes",
            "/data_classes",
        ],
    )
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|value| serde_json::from_value(value.clone()).ok())
    .chain(
        metadata_pointer(
            value,
            &["/gate/data_class", "/policy/data_class", "/data_class"],
        )
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok()),
    )
    .collect::<Vec<_>>()
}

fn metadata_resource_ref(value: &Value) -> Option<tandem_types::ResourceRef> {
    metadata_pointer(value, &["/gate/resource", "/policy/resource", "/resource"])
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn metadata_pointer<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a Value> {
    pointers.iter().find_map(|pointer| value.pointer(pointer))
}

fn automation_gate_resource_ref(
    automation: &AutomationV2Spec,
    gate: &crate::AutomationPendingGate,
) -> tandem_types::ResourceRef {
    let tenant = automation.tenant_context();
    tandem_types::ResourceRef::new(
        tenant.org_id,
        tenant.workspace_id,
        tandem_types::ResourceKind::Approval,
        format!("{}:{}", automation.automation_id, gate.node_id),
    )
}

async fn record_transition_guard_policy_decision(
    state: &AppState,
    automation: &AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    gate: &crate::AutomationPendingGate,
    actor: &crate::automation_v2::governance::GovernanceActorRef,
    denial: &crate::app::state::AutomationGateTransitionGuardDenial,
) -> Result<(), (StatusCode, Json<Value>)> {
    let decision = tandem_types::PolicyDecisionRecord {
        decision_id: format!("policy_decision_{}", uuid::Uuid::new_v4().simple()),
        tenant_context: run.tenant_context.clone(),
        requester_context: None,
        actor_id: actor.actor_id.clone().or_else(|| actor.source.clone()),
        session_id: None,
        message_id: None,
        run_id: Some(run.run_id.clone()),
        automation_id: Some(run.automation_id.clone()),
        node_id: Some(gate.node_id.clone()),
        tool: None,
        resource: Some(automation_gate_resource_ref(automation, gate)),
        data_classes: Vec::new(),
        risk_tier: None,
        decision: tandem_types::PolicyDecisionEffect::Deny,
        reason_code: denial.code.to_string(),
        reason: denial.detail.clone(),
        policy_id: Some("automation_v2_transition_guard".to_string()),
        grant_id: None,
        approval_id: denial
            .metadata
            .get("expected_approval_request_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        audit_event_id: None,
        created_at_ms: crate::now_ms(),
        metadata: denial.metadata.clone(),
    };
    state
        .record_policy_decision(decision)
        .await
        .map(|_| ())
        .map_err(|error| denial_receipt_error("transition guard policy decision", error))
}

async fn audit_gate_decision_denial(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    gate: Option<&crate::AutomationPendingGate>,
    actor: &crate::automation_v2::governance::GovernanceActorRef,
    code: &str,
    detail: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    crate::audit::append_protected_audit_event(
        state,
        "automation.governance.gate_decision_denied",
        &run.tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "runID": run.run_id,
            "automationID": run.automation_id,
            "nodeID": gate.map(|gate| gate.node_id.clone()),
            "resource": gate
                .map(|gate| json!({
                    "kind": "approval",
                    "id": format!("{}:{}", run.automation_id, gate.node_id),
                })),
            "decision": "denied",
            "code": code,
            "detail": detail,
            "actor": actor,
        }),
    )
    .await
    .map(|_| ())
    .map_err(|error| denial_receipt_error("approval gate denial", error))
}

fn denial_receipt_error(receipt: &str, error: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": format!("operation remained denied, but its required {receipt} receipt could not be written: {error}"),
            "code": "AUDIT_PERSISTENCE_FAILED",
        })),
    )
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
    let run_level_completion_assertion =
        automation_v2_run_has_run_level_completion_assertion(&current);
    let reset_nodes = if current.status == AutomationRunStatus::Failed {
        let mut roots = blocked_node_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        if let Some(failure_node_id) = automation_v2_recoverable_failure_node_id(&current) {
            roots.insert(failure_node_id);
        }
        if roots.is_empty() && run_level_completion_assertion {
            roots.extend(automation_v2_run_level_completion_recovery_node_ids(
                &automation,
            ));
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
    let recovering_from_pause = current.status == AutomationRunStatus::Paused;
    let reason = if recovering_from_pause {
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
                if reset_nodes.is_empty() && recovering_from_pause {
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
