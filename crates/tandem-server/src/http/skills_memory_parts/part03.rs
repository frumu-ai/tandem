pub(super) async fn workflow_learning_candidate_promote(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
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
    let capability_subject = crate::memory::subject::request_memory_subject(
        &tenant_context,
        verified_tenant_context.as_deref(),
        None,
    )
    .map_err(|_| StatusCode::FORBIDDEN)?
    .subject;
    let capability = issue_run_memory_capability(
        &run_id,
        Some(&capability_subject),
        &session_partition,
        RunMemoryCapabilityPolicy::CoderWorkflow,
    );
    let project_partition = workflow_learning_candidate_partition(
        &tenant_context,
        &candidate,
        tandem_memory::GovernedMemoryTier::Project,
    );
    let make_authority_job_context =
        |partition: &tandem_memory::MemoryPartition,
         operation: tandem_memory::MemoryAuthorityOperation,
         source_memory_ids: Vec<String>| {
            tandem_memory::MemoryAuthorityJobContext {
                org_id: tenant_context.org_id.clone(),
                workspace_id: tenant_context.workspace_id.clone(),
                deployment_id: tenant_context.deployment_id.clone(),
                project_id: partition.project_id.clone(),
                actor_id: capability.subject.clone(),
                run_id: run_id.clone(),
                node_id: candidate.node_id.clone(),
                task_id: Some(candidate.candidate_id.clone()),
                purpose: "promote approved workflow learning candidate".to_string(),
                source_binding_id: Some(format!("workflow:{}", candidate.workflow_id)),
                data_class: Some(tandem_types::DataClass::Internal),
                classification: tandem_memory::MemoryClassification::Internal,
                operation,
                source_memory_ids,
                artifact_refs: candidate.artifact_refs.clone(),
                policy_decision_id: input.approval_id.clone(),
                grant_decision_id: input.approval_id.clone(),
            }
        };
    let source_memory_id = if let Some(memory_id) = candidate.source_memory_id.clone() {
        let authority_job_context = make_authority_job_context(
            &session_partition,
            tandem_memory::MemoryAuthorityOperation::Write,
            vec![memory_id.clone()],
        );
        let knowledge_scope_policy =
            tandem_memory::knowledge_scope_policy_from_authority_job_context(
                &session_partition,
                &authority_job_context,
                format!(
                    "workflow-learning:{}:{}",
                    candidate.workflow_id, candidate.candidate_id
                ),
                vec![tandem_memory::GovernedMemoryTier::Session],
                vec![tandem_memory::GovernedMemoryTier::Project],
                true,
            )
            .ok_or(StatusCode::FORBIDDEN)?;
        backfill_workflow_learning_source_memory_scope(
            &state,
            &tenant_context,
            verified_tenant_context.as_deref(),
            &capability.subject,
            &memory_id,
            &knowledge_scope_policy,
        )
        .await?;
        memory_id
    } else {
        let content = workflow_learning_candidate_memory_content(&candidate)
            .ok_or(StatusCode::BAD_REQUEST)?;
        let authority_job_context = make_authority_job_context(
            &session_partition,
            tandem_memory::MemoryAuthorityOperation::Write,
            Vec::new(),
        );
        let knowledge_scope_policy =
            tandem_memory::knowledge_scope_policy_from_authority_job_context(
                &session_partition,
                &authority_job_context,
                format!(
                    "workflow-learning:{}:{}",
                    candidate.workflow_id, candidate.candidate_id
                ),
                vec![tandem_memory::GovernedMemoryTier::Session],
                vec![tandem_memory::GovernedMemoryTier::Project],
                true,
            )
            .ok_or(StatusCode::FORBIDDEN)?;
        let response = memory_put_impl_with_verified(
            &state,
            &tenant_context,
            verified_tenant_context.as_deref(),
            MemoryPutRequest {
                private: false,
                run_id: run_id.clone(),
                partition: session_partition.clone(),
                kind: tandem_memory::MemoryContentKind::Fact,
                content,
                artifact_refs: candidate.artifact_refs.clone(),
                classification: tandem_memory::MemoryClassification::Internal,
                authority_job_context: Some(authority_job_context),
                metadata: tandem_memory::metadata_with_knowledge_scope(
                    Some(json!({
                        "origin": "workflow_learning_candidate",
                        "candidate_id": candidate.candidate_id,
                        "workflow_id": candidate.workflow_id,
                        "kind": workflow_learning_kind_label(candidate.kind),
                    })),
                    &knowledge_scope_policy,
                ),
            },
            Some(capability.clone()),
        )
        .await?;
        response.id
    };
    let promote_response = memory_promote_impl_with_verified(
        &state,
        &tenant_context,
        verified_tenant_context.as_deref(),
        MemoryPromoteRequest {
            run_id: run_id.clone(),
            source_memory_id: source_memory_id.clone(),
            from_tier: tandem_memory::GovernedMemoryTier::Session,
            to_tier: tandem_memory::GovernedMemoryTier::Project,
            partition: project_partition.clone(),
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
            authority_job_context: Some(make_authority_job_context(
                &project_partition,
                tandem_memory::MemoryAuthorityOperation::Promote,
                vec![source_memory_id.clone()],
            )),
            source_outcome: Some(tandem_memory::PromotionSourceOutcome {
                status: Some("approved".to_string()),
                approved: Some(true),
                source_run_id: Some(candidate.source_run_id.clone()),
                approval_id: input.approval_id.clone(),
                policy_decision_id: None,
                audit_id: None,
            }),
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

async fn backfill_workflow_learning_source_memory_scope(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    caller_subject: &str,
    memory_id: &str,
    knowledge_scope_policy: &tandem_memory::KnowledgeScopePolicy,
) -> Result<(), StatusCode> {
    let store = open_global_memory_store_for_state(state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let local_unrestricted = crate::memory::subject::local_memory_subjects_are_unrestricted(
        tenant_context,
        verified_tenant_context,
    );
    let mut scope = if local_unrestricted {
        tandem_memory::MemoryReadScope::trusted_unrestricted(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        })
    } else {
        let (owner_org_unit_id, resolved_subject) = trusted_memory_database_scope(
            tenant_context,
            verified_tenant_context,
            Some(caller_subject),
        )?;
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        });
        scope.org_unit = owner_org_unit_id;
        scope.subject = resolved_subject;
        scope
    };
    let source = match with_verified_memory_decrypt_principal(
        verified_tenant_context,
        store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {
            scope: scope.clone(),
            id: memory_id.to_string(),
        }),
    )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        tandem_memory::MemoryStoreReadResult::GlobalRecord(record) => record,
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let Some(source) = source else {
        return Ok(());
    };
    if tandem_memory::metadata_has_knowledge_scope(source.metadata.as_ref()) {
        return Ok(());
    }
    let metadata = tandem_memory::metadata_with_knowledge_scope(
        source.metadata.clone(),
        knowledge_scope_policy,
    )
    .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let updated = with_verified_memory_decrypt_principal(
        verified_tenant_context,
        store.mutate(tandem_memory::MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope,
            id: source.id.clone(),
            visibility: source.visibility.clone(),
            demoted: source.demoted,
            metadata: Some(metadata),
            provenance: source.provenance.clone(),
        }),
    )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !matches!(updated, tandem_memory::MemoryStoreMutationResult::Changed(true)) {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(())
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
        source_pack_id: None,
        source_pack_version: None,
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
        planning: None,
        operation: None,
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
    let mut entries = load_memory_audit_events(&state).await;
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
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<MemoryListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let q = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0);
    let verified_tenant_context = verified_tenant_context.as_deref();
    let user_id = if crate::memory::subject::local_memory_subjects_are_unrestricted(
        &tenant_context,
        verified_tenant_context,
    ) {
        query
            .user_id
            .as_deref()
            .map(str::trim)
            .filter(|requested| !requested.is_empty())
            .unwrap_or("default")
            .to_string()
    } else {
        let resolution = crate::memory::subject::request_memory_subject(
            &tenant_context,
            verified_tenant_context,
            None,
        )
        .map_err(|_| StatusCode::FORBIDDEN)?;
        if let Some(requested) = query
            .user_id
            .as_deref()
            .map(str::trim)
            .filter(|requested| !requested.is_empty())
        {
            if requested != resolution.subject {
                return Err(StatusCode::FORBIDDEN);
            }
        }
        resolution.subject
    };
    let page = if let Some(store) = open_global_memory_store_for_state(&state).await {
        let (owner_org_unit_id, caller_subject) = trusted_memory_database_scope(
            &tenant_context,
            verified_tenant_context,
            Some(&user_id),
        )?;
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        });
        scope.org_unit = owner_org_unit_id;
        scope.subject = caller_subject;
        let source_access_filter = crate::memory::read_policy::governed_memory_read_filter(
            crate::config::env::resolve_runtime_auth_mode(),
            verified_tenant_context,
            false,
            crate::now_ms(),
        )
        .map(|filter| filter.with_caller_subject(user_id.clone()));
        const STORAGE_PAGE_SIZE: i64 = 1_000;
        let authorized_end = offset.saturating_add(limit);
        let mut storage_offset = 0i64;
        let mut authorized_seen = 0usize;
        let mut authorized_page = Vec::with_capacity(limit);
        while authorized_seen < authorized_end {
            let rows = match with_verified_memory_decrypt_principal(
                verified_tenant_context,
                store.query(tandem_memory::MemoryStoreQueryRequest::ListGlobalRecords {
                    scope: scope.clone(),
                    user_id: user_id.clone(),
                    query: Some(q.clone()),
                    project_tag: query.project_id.clone(),
                    channel_tag: query.channel_tag.clone(),
                    limit: STORAGE_PAGE_SIZE,
                    offset: storage_offset,
                }),
            )
                .await
            {
                Ok(tandem_memory::MemoryStoreQueryResult::GlobalRecords(rows)) => rows,
                _ => Vec::new(),
            };
            let row_count = rows.len();
            for row in rows {
                if !global_memory_record_visible_to_access_filter(
                    &row,
                    source_access_filter.as_ref(),
                ) {
                    continue;
                }
                if authorized_seen >= offset && authorized_page.len() < limit {
                    authorized_page.push(row);
                }
                authorized_seen = authorized_seen.saturating_add(1);
                if authorized_seen >= authorized_end {
                    break;
                }
            }
            if row_count < STORAGE_PAGE_SIZE as usize {
                break;
            }
            storage_offset = storage_offset.saturating_add(STORAGE_PAGE_SIZE);
        }
        authorized_page
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
                "governance": memory_promotion_governance_payload(
                    row.metadata.as_ref(),
                    row.provenance.as_ref(),
                ),
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

fn trusted_memory_database_scope(
    _tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    trusted_subject: Option<&str>,
) -> Result<(Option<String>, Option<String>), StatusCode> {
    Ok((
        crate::memory::subject::required_active_org_unit(verified)
            .map_err(|_| StatusCode::FORBIDDEN)?,
        trusted_subject.map(ToString::to_string),
    ))
}

/// Enforce per-user ownership before an admin-style mutation (demote/delete)
/// of a global memory record. Mirrors `memory_list`'s governed-mode behavior:
/// outside local-unrestricted mode, the caller's resolved memory subject must
/// own the record, so one user cannot demote/delete another user's memory
/// within a tenant (TAN-604). A dedicated operator/admin scope that can manage
/// other users' memory is a separate follow-up; today no such scope exists, so
/// ownership is required and the fail-safe is to deny.
fn enforce_memory_record_ownership_for_mutation(
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    record_user_id: &str,
) -> Result<(), StatusCode> {
    if crate::memory::subject::local_memory_subjects_are_unrestricted(tenant_context, verified) {
        return Ok(());
    }
    let resolution = crate::memory::subject::request_memory_subject(tenant_context, verified, None)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if record_user_id != resolution.subject {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

pub(super) async fn memory_delete(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
    Query(query): Query<MemoryDeleteQuery>,
) -> Result<Json<Value>, StatusCode> {
    let store = open_global_memory_store_for_state(&state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let caller_subject = if crate::memory::subject::local_memory_subjects_are_unrestricted(
        &tenant_context,
        verified_tenant_context.as_deref(),
    ) {
        None
    } else {
        Some(
            crate::memory::subject::request_memory_subject(
                &tenant_context,
                verified_tenant_context.as_deref(),
                None,
            )
            .map_err(|_| StatusCode::FORBIDDEN)?
            .subject,
        )
    };
    let local_unrestricted = crate::memory::subject::local_memory_subjects_are_unrestricted(
        &tenant_context,
        verified_tenant_context.as_deref(),
    );
    // Legacy tenant-authenticated callers retain the historical ownership-error
    // semantics (403 rather than a scoped 404); verified enterprise callers use
    // the fail-closed owner/department predicate below.
    let legacy_unverified = !local_unrestricted && verified_tenant_context.is_none();
    let scope = if local_unrestricted || legacy_unverified {
        tandem_memory::MemoryReadScope::trusted_unrestricted(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        })
    } else {
        let (owner_org_unit_id, resolved_subject) = trusted_memory_database_scope(
            &tenant_context,
            verified_tenant_context.as_deref(),
            caller_subject.as_deref(),
        )?;
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        });
        scope.org_unit = owner_org_unit_id;
        scope.subject = resolved_subject;
        scope
    };
    let record = match with_verified_memory_decrypt_principal(
        verified_tenant_context.as_deref(),
        store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {
            scope: scope.clone(),
            id: id.clone(),
        }),
    )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        tandem_memory::MemoryStoreReadResult::GlobalRecord(record) => record,
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let Some(record) = record else {
        emit_missing_memory_delete_audit(&state, &tenant_context, &id, "memory not found").await?;
        return Err(StatusCode::NOT_FOUND);
    };
    enforce_memory_record_ownership_for_mutation(
        &tenant_context,
        verified_tenant_context.as_deref(),
        &record.user_id,
    )?;
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
    let deleted = match store
        .mutate(tandem_memory::MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope,
            id: id.clone(),
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        tandem_memory::MemoryStoreMutationResult::Changed(deleted) => deleted,
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    if !deleted {
        return Err(StatusCode::NOT_FOUND);
    }
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
