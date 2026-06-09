fn validate_memory_capability_guardrail_context(
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, (String, &'static str, StatusCode)> {
    let cap = capability.unwrap_or_else(|| default_memory_capability_for(run_id, partition));
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
    Ok(cap)
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
    tandem_memory::validate_memory_authority_job_context(tandem_memory::MemoryAuthorityJobValidation {
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
    })
    .map_err(|error| error.as_str())
}

async fn validate_memory_put_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
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
    request: &MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
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
    request: &MemorySearchRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            let requested_scopes = if request.read_scopes.is_empty() {
                default_memory_capability_for(&request.run_id, &request.partition)
                    .memory
                    .read_tiers
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
