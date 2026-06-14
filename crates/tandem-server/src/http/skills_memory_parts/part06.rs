fn validate_memory_capability_guardrail_context(
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    capability: Option<MemoryCapabilityToken>,
    subject_mode: MemoryCapabilitySubjectMode,
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
        subject_mode,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryCapabilitySubjectMode {
    ActorOnly,
    ActorOrChannel,
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
    subject_mode: MemoryCapabilitySubjectMode,
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
    Ok(subject == resolution.subject
        || (subject_mode == MemoryCapabilitySubjectMode::ActorOrChannel
            && subject.starts_with("channel:")))
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
        MemoryCapabilitySubjectMode::ActorOnly,
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
        MemoryCapabilitySubjectMode::ActorOnly,
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
        if request.retrieval_gateway.is_some() {
            MemoryCapabilitySubjectMode::ActorOrChannel
        } else {
            MemoryCapabilitySubjectMode::ActorOnly
        },
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
