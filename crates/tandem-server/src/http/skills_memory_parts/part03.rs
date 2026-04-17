
pub(super) async fn workflow_learning_candidate_promote(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(candidate_id): Path<String>,
    Json(input): Json<WorkflowLearningCandidatePromoteRequest>,
) -> Result<Json<Value>, StatusCode> {
    let Some(candidate) = state.get_workflow_learning_candidate(&candidate_id).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    if candidate.kind != WorkflowLearningCandidateKind::MemoryFact {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(
        candidate.status,
        WorkflowLearningCandidateStatus::Approved | WorkflowLearningCandidateStatus::Applied
    ) {
        return Err(StatusCode::CONFLICT);
    }
    let run_id = input
        .run_id
        .clone()
        .unwrap_or_else(|| candidate.source_run_id.clone());
    let session_partition = workflow_learning_candidate_partition(
        &tenant_context,
        &candidate,
        tandem_memory::GovernedMemoryTier::Session,
    );
    let capability = issue_run_memory_capability(
        &run_id,
        tenant_context.actor_id.as_deref(),
        &session_partition,
        RunMemoryCapabilityPolicy::CoderWorkflow,
    );
    let source_memory_id = if let Some(memory_id) = candidate.source_memory_id.clone() {
        memory_id
    } else {
        let content = workflow_learning_candidate_memory_content(&candidate)
            .ok_or(StatusCode::BAD_REQUEST)?;
        let response = memory_put_impl(
            &state,
            &tenant_context,
            MemoryPutRequest {
                run_id: run_id.clone(),
                partition: session_partition.clone(),
                kind: tandem_memory::MemoryContentKind::Fact,
                content,
                artifact_refs: candidate.artifact_refs.clone(),
                classification: tandem_memory::MemoryClassification::Internal,
                metadata: Some(json!({
                    "origin": "workflow_learning_candidate",
                    "candidate_id": candidate.candidate_id,
                    "workflow_id": candidate.workflow_id,
                    "kind": workflow_learning_kind_label(candidate.kind),
                })),
            },
            Some(capability.clone()),
        )
        .await?;
        response.id
    };
    let promote_response = memory_promote_impl(
        &state,
        &tenant_context,
        MemoryPromoteRequest {
            run_id: run_id.clone(),
            source_memory_id: source_memory_id.clone(),
            from_tier: tandem_memory::GovernedMemoryTier::Session,
            to_tier: tandem_memory::GovernedMemoryTier::Project,
            partition: workflow_learning_candidate_partition(
                &tenant_context,
                &candidate,
                tandem_memory::GovernedMemoryTier::Project,
            ),
            reason: input.reason.unwrap_or_else(|| {
                format!(
                    "approved workflow learning candidate {}",
                    candidate.candidate_id
                )
            }),
            review: tandem_memory::PromotionReview {
                required: true,
                reviewer_id: input
                    .reviewer_id
                    .clone()
                    .or_else(|| tenant_context.actor_id.clone()),
                approval_id: input.approval_id.clone(),
            },
        },
        Some(capability),
    )
    .await?;
    let updated = state
        .update_workflow_learning_candidate(&candidate_id, |candidate| {
            candidate.source_memory_id = Some(source_memory_id.clone());
            candidate.promoted_memory_id = promote_response
                .new_memory_id
                .clone()
                .or_else(|| Some(source_memory_id.clone()));
        })
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({
        "ok": true,
        "candidate": updated,
        "promotion": promote_response,
    })))
}

pub(super) async fn workflow_learning_candidate_spawn_revision(
    State(state): State<AppState>,
    Path(candidate_id): Path<String>,
    Json(input): Json<WorkflowLearningCandidateSpawnRevisionRequest>,
) -> impl IntoResponse {
    let Some(candidate) = state.get_workflow_learning_candidate(&candidate_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !matches!(
        candidate.kind,
        WorkflowLearningCandidateKind::PromptPatch | WorkflowLearningCandidateKind::GraphPatch
    ) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !matches!(
        candidate.status,
        WorkflowLearningCandidateStatus::Approved | WorkflowLearningCandidateStatus::Applied
    ) {
        return StatusCode::CONFLICT.into_response();
    }
    let Some(automation) = state.get_automation_v2(&candidate.workflow_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let metadata = automation.metadata.as_ref();
    let bundle = metadata
        .and_then(|value| value.get("plan_package_bundle").cloned())
        .and_then(|value| {
            serde_json::from_value::<compiler_api::PlanPackageImportBundle>(value).ok()
        })
        .or_else(|| {
            metadata
                .and_then(|value| value.get("plan_package").cloned())
                .and_then(|value| serde_json::from_value::<compiler_api::PlanPackage>(value).ok())
                .map(|plan_package| {
                    let exported = compiler_api::export_plan_package_bundle(&plan_package);
                    compiler_api::PlanPackageImportBundle {
                        bundle_version: exported.bundle_version,
                        plan: exported.plan,
                        scope_snapshot: Some(exported.scope_snapshot),
                    }
                })
        });
    let Some(bundle) = bundle else {
        let _ = state
            .update_workflow_learning_candidate(&candidate_id, |candidate| {
                candidate.needs_plan_bundle = true;
            })
            .await;
        let updated = state.get_workflow_learning_candidate(&candidate_id).await;
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "error": "needs_plan_bundle",
                "detail": format!(
                    "Workflow `{}` must retain `plan_package` or `plan_package_bundle` metadata before `{}` learnings can spawn a planner revision.",
                    candidate.workflow_id,
                    workflow_learning_kind_label(candidate.kind),
                ),
                "candidate": updated,
            })),
        )
            .into_response();
    };
    let validation = compiler_api::validate_plan_package_bundle(&bundle);
    if !validation.compatible {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "error": "incompatible_plan_bundle",
                "detail": "Stored workflow plan bundle is not compatible with the current planner revision import path.",
                "validation": validation,
            })),
        )
            .into_response();
    }
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = automation
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or(default_workspace_root);
    let preview = compiler_api::preview_plan_package_import_bundle(
        &bundle,
        &workspace_root,
        input.reviewer_id.as_deref().unwrap_or("workflow_learning"),
    );
    let draft =
        crate::http::workflow_planner::workflow_plan_import_draft(&preview, &workspace_root);
    let now = crate::now_ms();
    let notes = format!(
        "Workflow learning candidate `{}` requested a `{}` revision.\n\nSummary:\n{}\n\nFingerprint:\n{}\n\nAffected runs:\n{}\n\nEvidence:\n{}\n\nConstraint:\nPreserve validated parts of the existing workflow and do not regress completion rate or validation pass rate.",
        candidate.candidate_id,
        workflow_learning_kind_label(candidate.kind),
        candidate.summary,
        candidate.fingerprint,
        candidate.run_ids.join(", "),
        serde_json::to_string_pretty(&candidate.evidence_refs).unwrap_or_default(),
    );
    let session = crate::http::workflow_planner::WorkflowPlannerSessionRecord {
        session_id: format!("wfplan-session-{}", Uuid::new_v4()),
        project_slug: candidate.project_id.clone(),
        title: input.title.unwrap_or_else(|| {
            workflow_learning_candidate_title(
                &candidate.summary,
                &format!(
                    "Revise {} workflow",
                    workflow_learning_kind_label(candidate.kind)
                ),
            )
        }),
        workspace_root: workspace_root.clone(),
        source_kind: "workflow_learning_revision".to_string(),
        source_bundle_digest: Some(preview.source_bundle_digest.clone()),
        current_plan_id: Some(draft.current_plan.plan_id.clone()),
        draft: Some(draft),
        goal: format!(
            "Revise workflow `{}` using approved {} candidate.",
            automation.name,
            workflow_learning_kind_label(candidate.kind)
        ),
        notes,
        planner_provider: String::new(),
        planner_model: String::new(),
        plan_source: "workflow_learning_revision".to_string(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({
            "candidate_id": candidate.candidate_id,
            "requested_change_type": workflow_learning_kind_label(candidate.kind),
            "fingerprint": candidate.fingerprint,
            "run_ids": candidate.run_ids,
        })),
        import_validation: Some(validation),
        import_transform_log: preview.import_transform_log.clone(),
        import_scope_snapshot: Some(preview.derived_scope_snapshot.clone()),
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let stored = state
        .put_workflow_planner_session(session)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST);
    let Ok(stored) = stored else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let baseline = state
        .workflow_learning_metrics_for_workflow(&candidate.workflow_id)
        .await;
    let updated = state
        .update_workflow_learning_candidate(&candidate_id, |candidate| {
            candidate.last_revision_session_id = Some(stored.session_id.clone());
            if candidate.baseline_before.is_none() {
                candidate.baseline_before = Some(baseline.clone());
            }
        })
        .await
        .ok_or(StatusCode::NOT_FOUND);
    let Ok(updated) = updated else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(json!({
        "ok": true,
        "candidate": updated,
        "session": stored,
    }))
    .into_response()
}

pub(super) async fn memory_audit(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<MemoryAuditQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut entries = load_memory_audit_events(&state.memory_audit_path).await;
    if entries.is_empty() {
        entries = state.memory_audit_log.read().await.clone();
    }
    entries.retain(|event| event.tenant_context == tenant_context);
    if let Some(run_id) = query.run_id {
        entries.retain(|event| event.run_id == run_id);
    }
    entries.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    entries.truncate(limit);
    Json(json!({
        "events": entries,
        "count": entries.len(),
    }))
}

pub(super) async fn memory_list(
    State(_state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<MemoryListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let q = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0);
    let user_id = match (query.user_id.as_deref(), tenant_context.actor_id.as_deref()) {
        (Some(requested), Some(actor)) if requested != actor => {
            return Err(StatusCode::FORBIDDEN);
        }
        (Some(requested), _) => requested.to_string(),
        (None, Some(actor)) => actor.to_string(),
        (None, None) => "default".to_string(),
    };
    let page = if let Some(db) = open_global_memory_db().await {
        db.list_global_memory(
            &user_id,
            Some(&q),
            query.project_id.as_deref(),
            query.channel_tag.as_deref(),
            limit as i64,
            offset as i64,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            json!({
                "id": row.id,
                "user_id": row.user_id,
                "run_id": row.run_id,
                "tier": memory_tier_for_visibility(&row.visibility),
                "classification": memory_classification_label(row.metadata.as_ref()),
                "kind": memory_kind_label(&row.source_type),
                "source_type": row.source_type,
                "content": row.content,
                "artifact_refs": memory_artifact_refs(row.metadata.as_ref()),
                "linkage": memory_linkage(&row),
                "metadata": row.metadata,
                "provenance": row.provenance,
                "created_at_ms": row.created_at_ms,
                "updated_at_ms": row.updated_at_ms,
                "visibility": row.visibility,
                "demoted": row.demoted,
            })
        })
        .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let total = page.len();
    Ok(Json(json!({
        "items": page,
        "count": total,
        "offset": offset,
        "limit": limit,
    })))
}

pub(super) async fn memory_delete(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Query(query): Query<MemoryDeleteQuery>,
) -> Result<Json<Value>, StatusCode> {
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let record = db
        .get_global_memory(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(record) = record else {
        emit_missing_memory_delete_audit(&state, &tenant_context, &id, "memory not found").await?;
        return Err(StatusCode::NOT_FOUND);
    };
    if query
        .project_id
        .as_deref()
        .is_some_and(|project_id| record.project_tag.as_deref() != Some(project_id))
    {
        emit_missing_memory_delete_audit(&state, &tenant_context, &id, "memory not found").await?;
        return Err(StatusCode::NOT_FOUND);
    }
    if query
        .channel_tag
        .as_deref()
        .is_some_and(|channel_tag| record.channel_tag.as_deref() != Some(channel_tag))
    {
        emit_missing_memory_delete_audit(&state, &tenant_context, &id, "memory not found").await?;
        return Err(StatusCode::NOT_FOUND);
    }
    let deleted = db
        .delete_global_memory(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !deleted {
        emit_missing_memory_delete_audit(&state, &tenant_context, &id, "memory not found").await?;
        return Err(StatusCode::NOT_FOUND);
    }
    let now = crate::now_ms();
    let audit_id = Uuid::new_v4().to_string();
    let run_id = record.run_id.clone();
    let delete_detail = format!(
        "kind={} classification={} artifact_refs={} visibility={} tier={} partition_key={} demoted={}{}",
        memory_kind_label(&record.source_type),
        memory_classification_label(record.metadata.as_ref()),
        memory_artifact_refs(record.metadata.as_ref())
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(","),
        record.visibility,
        memory_tier_for_visibility(&record.visibility),
        memory_linkage(&record)
            .get("partition_key")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        record.demoted,
        memory_linkage_detail(&memory_linkage(&record))
    );
    append_memory_audit(
        &state,
        &tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_delete".to_string(),
            run_id: run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(id.clone()),
            source_memory_id: None,
            to_tier: None,
            partition_key: record
                .project_tag
                .clone()
                .unwrap_or_else(|| "global".to_string()),
            actor: "admin".to_string(),
            status: "ok".to_string(),
            detail: Some(delete_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.deleted",
        json!({
            "memoryID": id,
            "runID": run_id,
            "kind": memory_kind_label(&record.source_type),
            "classification": memory_classification_label(record.metadata.as_ref()),
            "artifactRefs": memory_artifact_refs(record.metadata.as_ref()),
            "visibility": record.visibility,
            "tier": memory_tier_for_visibility(&record.visibility),
            "partitionKey": memory_linkage(&record)
                .get("partition_key")
                .and_then(Value::as_str),
            "demoted": record.demoted,
            "linkage": memory_linkage(&record),
            "auditID": audit_id,
        }),
    );
    Ok(Json(json!({
        "ok": true,
        "audit_id": audit_id,
    })))
}
