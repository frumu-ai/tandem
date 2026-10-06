// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn context_distill(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    input: Result<Json<ContextDistillRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, StatusCode> {
    let Json(input) = input.map_err(|_| StatusCode::BAD_REQUEST)?;
    let run_id = input
        .run_id
        .clone()
        .unwrap_or_else(|| format!("distill-{}", input.session_id));
    let provider_auth_run_id = run_id.clone();
    let project_id = input
        .project_id
        .clone()
        .or_else(|| input.workflow_id.clone())
        .unwrap_or_else(|| input.session_id.clone());
    let subject = crate::memory::subject::request_memory_subject(
        &tenant_context,
        verified_tenant_context.as_deref(),
        input
            .subject
            .as_deref()
            .or(tenant_context.actor_id.as_deref()),
    )
    .map_err(|_| StatusCode::FORBIDDEN)?
    .subject;
    workflow_learning_distillation_source_binding(
        &state,
        &tenant_context,
        verified_tenant_context.as_deref(),
        input.workflow_id.as_deref(),
        &input.session_id,
        &subject,
    )
    .await?;
    let batches = resolve_distillation_batches(
        &state, &tenant_context, verified_tenant_context.as_deref(), &subject, &input,
    ).await?;
    let providers = Arc::new(state.runtime.wait().providers.clone());
    let partition = tandem_memory::MemoryPartition {
        org_id: tenant_context.org_id.clone(), workspace_id: tenant_context.workspace_id.clone(),
        project_id, tier: tandem_memory::GovernedMemoryTier::Session,
    };
    let capability = issue_run_memory_capability(
        &run_id, Some(subject.as_str()), &partition, RunMemoryCapabilityPolicy::CoderWorkflow,
    );
    let threshold = input.importance_threshold.unwrap_or(0.5);
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) { return Err(StatusCode::BAD_REQUEST); }
    let mut reports = Vec::new();
    for batch in batches {
        let provider_egress = crate::provider_egress::memory_egress_context(
            &state, Some(&tenant_context), verified_tenant_context.as_deref(), Some(&run_id), Some(&input.session_id),
        ).with_additional_data_classes(distillation_egress_classes(&batch.lineage));
        let writer = GovernedDistillationWriter {
            state: state.clone(), tenant_context: tenant_context.clone(),
            verified_tenant_context: verified_tenant_context.as_deref().cloned(),
            partition: partition.clone(), capability: capability.clone(), run_id: run_id.clone(),
            workflow_id: input.workflow_id.clone(), artifact_refs: input.artifact_refs.clone(),
            subject: subject.clone(), lineage: batch.lineage,
        };
        writer.ensure_current_lineage().await.map_err(|_| StatusCode::FORBIDDEN)?;
        let distiller = tandem_memory::SessionDistiller::with_threshold(providers.clone(), threshold)
            .with_provider_egress(provider_egress).with_source_message_ids(batch.source_message_ids);
        let future = distiller.distill_with_writer(&input.session_id, &batch.conversation, &writer);
        let report = crate::http::session_run_retry::scope_provider_auth_for_tenant(
            &state, &tenant_context, verified_tenant_context.as_deref(),
            crate::http::session_run_retry::PromptExecutionSurface::KnowledgeBase,
            Some(&input.session_id), Some(&provider_auth_run_id), None, future,
        ).await.map_err(|error| {
            tracing::warn!("Failed to distill canonical session: {}", error);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        // A distiller can report extracted-only after a writer refuses a fact.
        // Recheck the source/identity before returning a successful HTTP report.
        writer.ensure_current_lineage().await.map_err(|_| StatusCode::FORBIDDEN)?;
        reports.push(report);
    }
    let batch_reports = reports.clone();
    let mut reports = reports.into_iter();
    let mut report = reports.next().ok_or(StatusCode::BAD_REQUEST)?;
    for next in reports {
        report.facts_extracted += next.facts_extracted;
        report.stored_count += next.stored_count;
        report.deduped_count += next.deduped_count;
        report.user_memory_updated |= next.user_memory_updated;
        report.agent_memory_updated |= next.agent_memory_updated;
        report.memory_ids.extend(next.memory_ids);
        report.candidate_ids.extend(next.candidate_ids);
        if next.distilled_at > report.distilled_at { report.distilled_at = next.distilled_at; }
        if report.stored_count > 0 || report.deduped_count > 0 { report.status = "stored".to_string(); }
        else if report.facts_extracted > 0 { report.status = "facts_extracted_only".to_string(); }
    }
    let distillation_id = report.distillation_id.clone();
    let session_id = report.session_id.clone();
    let facts_extracted = report.facts_extracted;
    let stored_count = report.stored_count;
    let deduped_count = report.deduped_count;
    let memory_ids = report.memory_ids.clone();
    let candidate_ids = report.candidate_ids.clone();
    let status = report.status.clone();

    Ok(Json(json!({
        "ok": true,
        "distillation_id": distillation_id,
        "session_id": session_id,
        "facts_extracted": facts_extracted,
        "stored_count": stored_count,
        "deduped_count": deduped_count,
        "memory_ids": memory_ids,
        "candidate_ids": candidate_ids,
        "status": status,
        "report": report,
        "batch_reports": batch_reports,
    })))
}
