fn validate_memory_capability_guardrail_context(
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, (String, &'static str, StatusCode)> {
    let cap = match capability {
        Some(cap) => cap,
        None => default_memory_capability_for_request(
            run_id,
            partition,
            tenant_context,
            verified_tenant_context,
        )
        .map_err(|detail| ("".to_string(), detail, StatusCode::FORBIDDEN))?,
    };
    if cap.run_id != run_id
        || cap.org_id != partition.org_id
        || cap.workspace_id != partition.workspace_id
        || cap.project_id != partition.project_id
    {
        return Err((
            cap.subject.clone(),
            "capability context mismatch",
            StatusCode::FORBIDDEN,
        ));
    }
    if cap.expires_at < crate::now_ms() {
        return Err((
            cap.subject.clone(),
            "capability expired",
            StatusCode::UNAUTHORIZED,
        ));
    }
    if !memory_capability_subject_matches_request_actor(
        tenant_context,
        verified_tenant_context,
        &cap.subject,
    )
    .map_err(|detail| (cap.subject.clone(), detail, StatusCode::FORBIDDEN))?
    {
        return Err((
            cap.subject.clone(),
            "capability subject actor mismatch",
            StatusCode::FORBIDDEN,
        ));
    }
    Ok(cap)
}

fn default_memory_capability_for_request(
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> Result<MemoryCapabilityToken, &'static str> {
    let resolution = crate::memory::subject::request_memory_subject(
        tenant_context,
        verified_tenant_context,
        None,
    )
    .map_err(|error| error.as_str())?;
    Ok(issue_run_memory_capability(
        run_id,
        Some(resolution.subject.as_str()),
        partition,
        RunMemoryCapabilityPolicy::Default,
    ))
}

fn memory_capability_subject_matches_request_actor(
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    subject: &str,
) -> Result<bool, &'static str> {
    if crate::memory::subject::local_memory_subjects_are_unrestricted(
        tenant_context,
        verified_tenant_context,
    ) {
        return Ok(true);
    }
    let resolution = crate::memory::subject::request_memory_subject(
        tenant_context,
        verified_tenant_context,
        None,
    )
    .map_err(|error| error.as_str())?;
    let subject = subject.trim();
    Ok(subject == resolution.subject)
}

struct MemoryAuthorityRequestValidation<'a> {
    tenant_context: &'a TenantContext,
    capability: &'a MemoryCapabilityToken,
    run_id: &'a str,
    partition: &'a tandem_memory::MemoryPartition,
    operation: tandem_memory::MemoryAuthorityOperation,
    classification: Option<tandem_memory::MemoryClassification>,
    source_memory_id: Option<&'a str>,
    authority_job_context: Option<&'a tandem_memory::MemoryAuthorityJobContext>,
}

fn validate_memory_authority_job_context_for_request(
    validation: MemoryAuthorityRequestValidation<'_>,
) -> Result<(), &'static str> {
    let MemoryAuthorityRequestValidation {
        tenant_context,
        capability,
        run_id,
        partition,
        operation,
        classification,
        source_memory_id,
        authority_job_context,
    } = validation;
    let (org_id, workspace_id, deployment_id) = if tenant_context.is_local_implicit() {
        (
            partition.org_id.as_str(),
            partition.workspace_id.as_str(),
            None,
        )
    } else {
        (
            tenant_context.org_id.as_str(),
            tenant_context.workspace_id.as_str(),
            tenant_context.deployment_id.as_deref(),
        )
    };
    tandem_memory::validate_memory_authority_job_context(
        tandem_memory::MemoryAuthorityJobValidation {
            context: authority_job_context,
            require_context: false,
            org_id,
            workspace_id,
            deployment_id,
            actor_id: Some(capability.subject.as_str()),
            run_id,
            partition,
            operation,
            classification,
            source_memory_id,
        },
    )
    .map_err(|error| error.as_str())
}

async fn validate_memory_put_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    request: &MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        tenant_context,
        verified_tenant_context,
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            emit_blocked_memory_put_guardrail(state, tenant_context, request, actor, detail)
                .await?;
            return Err(status);
        }
    };
    if !memory_partition_matches_request_tenant(tenant_context, &request.partition) {
        emit_blocked_memory_put_guardrail(
            state,
            tenant_context,
            request,
            cap.subject.clone(),
            "partition tenant mismatch",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    if let Err(detail) =
        validate_memory_authority_job_context_for_request(MemoryAuthorityRequestValidation {
            tenant_context,
            capability: &cap,
            run_id: &request.run_id,
            partition: &request.partition,
            operation: tandem_memory::MemoryAuthorityOperation::Write,
            classification: Some(request.classification),
            source_memory_id: None,
            authority_job_context: request.authority_job_context.as_ref(),
        })
    {
        emit_blocked_memory_put_guardrail(
            state,
            tenant_context,
            request,
            cap.subject.clone(),
            detail,
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(cap)
}

async fn validate_memory_promote_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    request: &MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        tenant_context,
        verified_tenant_context,
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            emit_blocked_memory_promote_guardrail(state, tenant_context, request, actor, detail)
                .await?;
            return Err(status);
        }
    };
    if !memory_partition_matches_request_tenant(tenant_context, &request.partition) {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            request,
            cap.subject.clone(),
            "partition tenant mismatch",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    if let Err(detail) =
        validate_memory_authority_job_context_for_request(MemoryAuthorityRequestValidation {
            tenant_context,
            capability: &cap,
            run_id: &request.run_id,
            partition: &request.partition,
            operation: tandem_memory::MemoryAuthorityOperation::Promote,
            classification: None,
            source_memory_id: Some(&request.source_memory_id),
            authority_job_context: request.authority_job_context.as_ref(),
        })
    {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            request,
            cap.subject.clone(),
            detail,
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(cap)
}

async fn validate_memory_search_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    request: &MemorySearchRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        tenant_context,
        verified_tenant_context,
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            let requested_scopes = if request.read_scopes.is_empty() {
                default_memory_capability_for_request(
                    &request.run_id,
                    &request.partition,
                    tenant_context,
                    verified_tenant_context,
                )
                .map(|capability| capability.memory.read_tiers)
                .unwrap_or_default()
            } else {
                request.read_scopes.clone()
            };
            return emit_blocked_memory_search_guardrail(
                status,
                detail,
                actor,
                state,
                tenant_context,
                request,
                &requested_scopes,
                &request.partition.key(),
            )
            .await;
        }
    };
    if !memory_partition_matches_request_tenant(tenant_context, &request.partition) {
        let requested_scopes = if request.read_scopes.is_empty() {
            cap.memory.read_tiers.clone()
        } else {
            request.read_scopes.clone()
        };
        return emit_blocked_memory_search_guardrail(
            StatusCode::FORBIDDEN,
            "partition tenant mismatch",
            cap.subject.clone(),
            state,
            tenant_context,
            request,
            &requested_scopes,
            &request.partition.key(),
        )
        .await;
    }
    if let Err(detail) =
        validate_memory_authority_job_context_for_request(MemoryAuthorityRequestValidation {
            tenant_context,
            capability: &cap,
            run_id: &request.run_id,
            partition: &request.partition,
            operation: tandem_memory::MemoryAuthorityOperation::Read,
            classification: None,
            source_memory_id: None,
            authority_job_context: request.authority_job_context.as_ref(),
        })
    {
        let requested_scopes = if request.read_scopes.is_empty() {
            cap.memory.read_tiers.clone()
        } else {
            request.read_scopes.clone()
        };
        return emit_blocked_memory_search_guardrail(
            StatusCode::FORBIDDEN,
            detail,
            cap.subject.clone(),
            state,
            tenant_context,
            request,
            &requested_scopes,
            &request.partition.key(),
        )
        .await;
    }
    Ok(cap)
}

pub(super) async fn memory_put(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<MemoryPutInput>,
) -> Result<Json<MemoryPutResponse>, StatusCode> {
    let response = memory_put_impl_with_verified(
        &state,
        &tenant_context,
        verified_tenant_context.as_deref(),
        input.request,
        input.capability,
    )
    .await?;
    Ok(Json(response))
}

pub(crate) async fn memory_put_impl(
    state: &AppState,
    tenant_context: &TenantContext,
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPutResponse, StatusCode> {
    memory_put_impl_with_verified(state, tenant_context, None, request, capability).await
}

pub(super) async fn memory_put_impl_with_verified(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPutResponse, StatusCode> {
    let capability = validate_memory_put_capability_with_guardrail(
        state,
        tenant_context,
        verified_tenant_context,
        &request,
        capability,
    )
    .await?;
    if !capability
        .memory
        .write_tiers
        .contains(&request.partition.tier)
    {
        emit_blocked_memory_put_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "write tier not allowed by capability",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    // Team/Curated exist in the governance contract ahead of storage backing:
    // nothing distinguishes such records beyond a self-declared partition-key
    // label, so writes fail closed until real tier semantics land (TAN-607).
    if matches!(
        request.partition.tier,
        tandem_memory::GovernedMemoryTier::Team | tandem_memory::GovernedMemoryTier::Curated
    ) {
        emit_blocked_memory_put_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "tier_not_storage_backed",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    let now = crate::now_ms();
    let require_scope_metadata =
        crate::memory::policy_status::current_memory_context_policy_status().strict_required;
    let scope_decision =
        tandem_memory::memory_write_scope_decision_for_context_with_enterprise_mode(
            &request.partition,
            request.metadata.as_ref(),
            request.authority_job_context.as_ref(),
            require_scope_metadata,
            now,
        )
    .map_err(|error| {
        tracing::warn!("invalid knowledge scope metadata on memory put: {error}");
        StatusCode::FORBIDDEN
    })?;
    if !scope_decision.allowed {
        emit_blocked_memory_put_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            &scope_decision.reason_code,
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    // A writer may department-restrict a record only to an org unit they are a
    // member of; otherwise ownership could be forged onto units the writer does
    // not belong to. Enforced only when the runtime carries org-unit identity —
    // local single-tenant mode has no org model and no verified context.
    if let Some(owner_org_unit_id) =
        tandem_memory::types::owner_org_unit_id_from_metadata(request.metadata.as_ref())
    {
        let requires_membership = crate::config::env::resolve_runtime_auth_mode()
            != tandem_types::RuntimeAuthMode::LocalSingleTenant
            || verified_tenant_context.is_some();
        let is_member = verified_tenant_context
            .is_some_and(|verified| verified.org_units.iter().any(|u| u == &owner_org_unit_id));
        if requires_membership && !is_member {
            emit_blocked_memory_put_guardrail(
                state,
                tenant_context,
                &request,
                capability.subject.clone(),
                "owner_org_unit_membership_required",
            )
            .await?;
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let id = Uuid::new_v4().to_string();
    let partition_key = request.partition.key();
    let kind = memory_kind_for_request(request.kind.clone());
    let audit_id = Uuid::new_v4().to_string();
    let db = open_global_memory_db_for_state(state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact_refs = request.artifact_refs.clone();
    let artifact_ref_labels = artifact_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let source_type = match request.kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
    .to_string();
    let user_id = capability.subject.clone();
    let trust_label = memory_trust_label_for_put(&request);
    let metadata = memory_metadata_with_trust_fields(
        memory_metadata_with_storage_fields(
            request.metadata.clone(),
            &artifact_refs,
            request.classification,
        ),
        trust_label,
    );
    let provenance = memory_provenance_with_trust(
        memory_put_provenance(&request, &partition_key, &artifact_refs, tenant_context),
        trust_label,
    );
    let record = GlobalMemoryRecord {
        id: id.clone(),
        user_id,
        source_type,
        content: request.content.clone(),
        content_hash: String::new(),
        run_id: request.run_id.clone(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some(request.partition.project_id.clone()),
        channel_tag: None,
        host_tag: None,
        metadata,
        provenance: Some(provenance),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: None,
    };
    let memory_linkage_value = memory_linkage_from_parts(
        &request.run_id,
        Some(&request.partition.project_id),
        record.metadata.as_ref(),
        record.provenance.as_ref(),
    );
    let put_detail = format!(
        "kind={} classification={} artifact_refs={} visibility=private tier={} partition_key={}{}",
        kind,
        memory_classification_label(record.metadata.as_ref()),
        artifact_ref_labels,
        request.partition.tier,
        partition_key,
        memory_linkage_detail(&memory_linkage_value)
    );
    persist_global_memory_record(&state, &db, record).await;
    append_memory_audit(
        &state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_put".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(id.clone()),
            source_memory_id: None,
            to_tier: Some(request.partition.tier),
            partition_key: partition_key.clone(),
            actor: capability.subject,
            status: "ok".to_string(),
            detail: Some(put_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.put",
        json!({
            "runID": request.run_id,
            "memoryID": id,
            "kind": kind,
            "classification": request.classification,
            "artifactRefs": artifact_refs,
            "visibility": "private",
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_value.clone(),
            "auditID": audit_id,
        }),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "memory.updated",
        json!({
            "memoryID": id,
            "runID": request.run_id,
            "action": "put",
            "kind": kind,
            "classification": request.classification,
            "artifactRefs": artifact_refs,
            "visibility": "private",
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_value,
            "auditID": audit_id,
        }),
    );
    Ok(MemoryPutResponse {
        id,
        stored: true,
        tier: request.partition.tier,
        partition_key,
        audit_id,
    })
}
