// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn validate_memory_capability_guardrail_context(
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    retrieval_gateway: Option<&tandem_memory::MemoryRetrievalGatewayRequest>,
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
        retrieval_gateway,
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
    retrieval_gateway: Option<&tandem_memory::MemoryRetrievalGatewayRequest>,
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
    if subject == resolution.subject {
        return Ok(true);
    }
    Ok(verified_channel_gateway_subject_matches(
        verified_tenant_context,
        retrieval_gateway,
        subject,
    ))
}

fn verified_channel_gateway_subject_matches(
    verified_tenant_context: Option<&VerifiedTenantContext>,
    retrieval_gateway: Option<&tandem_memory::MemoryRetrievalGatewayRequest>,
    subject: &str,
) -> bool {
    if verified_tenant_context.is_none() {
        return false;
    }
    let Some(gateway) = retrieval_gateway else {
        return false;
    };
    if gateway.grant.subject.trim() != subject {
        return false;
    }
    let Some(channel) = gateway
        .channel
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Some(user_id) = gateway
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    subject == format!("channel:{channel}:{user_id}")
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
        None,
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
        None,
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
        request.retrieval_gateway.as_ref(),
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
    let store = open_global_memory_store_for_state(state)
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
    // Authoritatively stamp the collector's active department (TAN-646). A
    // client-supplied department already survived the membership check above and
    // is preserved; otherwise the verified context's active department is written
    // so attributable data is never persisted without a department.
    let active_org_unit = crate::memory::subject::active_org_unit(verified_tenant_context);
    // Per-user opt-in (TAN-648): a `private` write additionally restricts the
    // record to the collecting subject (`user_id`), stamped as `owner_subject`
    // so the governed read filter denies any other caller. Default (not private)
    // leaves the record department/tenant-governed.
    let owner_subject = request.private.then(|| user_id.clone());
    let metadata = memory_metadata_with_owner_subject(
        memory_metadata_with_owner_org_unit(
            memory_metadata_with_trust_fields(
                memory_metadata_with_storage_fields(
                    request.metadata.clone(),
                    &artifact_refs,
                    request.classification,
                ),
                trust_label,
            ),
            active_org_unit.as_deref(),
        ),
        owner_subject.as_deref(),
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
    persist_global_memory_record(&state, store.as_ref(), record).await;
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

#[cfg(test)]
mod retrieval_gateway_subject_tests {
    use super::*;

    fn partition() -> tandem_memory::MemoryPartition {
        tandem_memory::MemoryPartition {
            org_id: "acme".to_string(),
            workspace_id: "north".to_string(),
            project_id: "proj-a".to_string(),
            tier: tandem_memory::GovernedMemoryTier::Session,
        }
    }

    fn channel_capability(subject: &str) -> tandem_memory::MemoryCapabilityToken {
        tandem_memory::MemoryCapabilityToken {
            run_id: "channel-gateway-run".to_string(),
            subject: subject.to_string(),
            org_id: "acme".to_string(),
            workspace_id: "north".to_string(),
            project_id: "proj-a".to_string(),
            memory: tandem_memory::MemoryCapabilities::default(),
            expires_at: 9_999_999_999_999,
        }
    }

    fn channel_gateway(subject: &str) -> tandem_memory::MemoryRetrievalGatewayRequest {
        tandem_memory::MemoryRetrievalGatewayRequest {
            grant: tandem_memory::MemoryRetrievalGrant {
                grant_id: "channel-gateway-grant".to_string(),
                subject: subject.to_string(),
                org_id: "acme".to_string(),
                workspace_id: "north".to_string(),
                project_ids: vec!["proj-a".to_string()],
                source_binding_ids: Vec::new(),
                source_object_ids: Vec::new(),
                data_classes: Vec::new(),
                budgets: tandem_memory::MemoryRetrievalBudgets::default(),
                revoked: false,
                expires_at: None,
            },
            session_id: Some("channel-session".to_string()),
            channel: Some("slack".to_string()),
            user_id: Some("U999".to_string()),
        }
    }

    fn verified_channel_service_context(
        tenant_context: tandem_types::TenantContext,
    ) -> tandem_types::VerifiedTenantContext {
        let request_principal =
            tandem_types::RequestPrincipal::authenticated_user("channel-service", "tandem-channel");
        let authority_chain = tandem_types::AuthorityChain::from_request(request_principal);
        tandem_types::VerifiedTenantContext {
            tenant_context,
            human_actor: tandem_types::HumanActor::tandem_user("channel-service"),
            authority_chain,
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-channel".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_999_999_999_999,
            assertion_id: "channel-service-assertion".to_string(),
            assertion_key_id: None,
        }
    }

    #[test]
    fn owner_subject_metadata_is_server_controlled() {
        use tandem_memory::types::owner_subject_from_metadata;

        // Private write → collector subject stamped.
        let private = memory_metadata_with_owner_subject(None, Some("user-a"));
        assert_eq!(
            owner_subject_from_metadata(private.as_ref()).as_deref(),
            Some("user-a")
        );

        // Non-private write must STRIP any client-supplied owner_subject, so a
        // forged metadata key can't lock the record to someone else (P2 fix).
        let stripped = memory_metadata_with_owner_subject(
            Some(serde_json::json!({ "owner_subject": "forged", "role": "user" })),
            None,
        );
        assert_eq!(owner_subject_from_metadata(stripped.as_ref()), None);
        // Other metadata keys survive the strip.
        assert_eq!(
            stripped
                .as_ref()
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str()),
            Some("user")
        );

        // Private write overrides a client-supplied value with the collector.
        let overridden = memory_metadata_with_owner_subject(
            Some(serde_json::json!({ "owner_subject": "forged" })),
            Some("user-a"),
        );
        assert_eq!(
            owner_subject_from_metadata(overridden.as_ref()).as_deref(),
            Some("user-a")
        );

        // Nothing to do → untouched.
        assert!(memory_metadata_with_owner_subject(None, None).is_none());
    }

    #[test]
    fn owner_org_unit_metadata_stamp_is_absent_preserving() {
        use tandem_memory::types::owner_org_unit_id_from_metadata;

        // No active department → metadata untouched.
        assert!(memory_metadata_with_owner_org_unit(None, None).is_none());
        let unchanged =
            memory_metadata_with_owner_org_unit(Some(serde_json::json!({"role": "user"})), None);
        assert_eq!(
            owner_org_unit_id_from_metadata(unchanged.as_ref()),
            None,
            "no department stamped when none is active"
        );

        // Absent department → stamped from the active department.
        let stamped = memory_metadata_with_owner_org_unit(
            Some(serde_json::json!({"role": "user"})),
            Some("department/finance"),
        );
        assert_eq!(
            owner_org_unit_id_from_metadata(stamped.as_ref()).as_deref(),
            Some("department/finance")
        );

        // Client-supplied department (already membership-validated) is preserved,
        // never overwritten by the active department.
        let preserved = memory_metadata_with_owner_org_unit(
            Some(serde_json::json!({"owner_org_unit_id": "department/sales"})),
            Some("department/finance"),
        );
        assert_eq!(
            owner_org_unit_id_from_metadata(preserved.as_ref()).as_deref(),
            Some("department/sales")
        );

        // Missing metadata is created as an object carrying the department.
        let created = memory_metadata_with_owner_org_unit(None, Some("department/eng"));
        assert_eq!(
            owner_org_unit_id_from_metadata(created.as_ref()).as_deref(),
            Some("department/eng")
        );
    }

    #[test]
    fn verified_channel_gateway_may_use_sender_subject() {
        let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "acme",
            "north",
            None,
            "channel-service",
        );
        let verified = verified_channel_service_context(tenant_context.clone());
        let subject = "channel:slack:U999";
        let gateway = channel_gateway(subject);
        let cap = validate_memory_capability_guardrail_context(
            &tenant_context,
            Some(&verified),
            "channel-gateway-run",
            &partition(),
            Some(&gateway),
            Some(channel_capability(subject)),
        )
        .expect("verified channel gateway subject should be accepted");

        assert_eq!(cap.subject, subject);
    }

    #[test]
    fn unverified_channel_gateway_subject_still_rejected() {
        let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "acme",
            "north",
            None,
            "user-a",
        );
        let subject = "channel:slack:U999";
        let gateway = channel_gateway(subject);
        let err = validate_memory_capability_guardrail_context(
            &tenant_context,
            None,
            "channel-gateway-run",
            &partition(),
            Some(&gateway),
            Some(channel_capability(subject)),
        )
        .expect_err("unverified forged channel subject should be rejected");

        assert_eq!(err.1, "capability subject actor mismatch");
    }
}
