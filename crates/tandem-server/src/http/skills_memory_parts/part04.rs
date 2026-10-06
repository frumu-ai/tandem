// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn memory_promote(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<MemoryPromoteInput>,
) -> Result<Json<MemoryPromoteResponse>, StatusCode> {
    let response = memory_promote_impl_with_verified(
        &state,
        &tenant_context,
        verified_tenant_context.as_deref(),
        input.request,
        input.capability,
    )
    .await?;
    Ok(Json(response))
}

pub(crate) async fn memory_promote_impl(
    state: &AppState,
    tenant_context: &TenantContext,
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPromoteResponse, StatusCode> {
    memory_promote_impl_with_verified(state, tenant_context, None, request, capability).await
}

async fn memory_promote_impl_with_verified(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPromoteResponse, StatusCode> {
    let source_memory_id = request.source_memory_id.clone();
    let capability = validate_memory_promote_capability_with_guardrail(
        state,
        tenant_context,
        verified_tenant_context,
        &request,
        capability,
    )
    .await?;
    if !capability.memory.promote_targets.contains(&request.to_tier) {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "promotion target not allowed by capability",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    // Same fail-closed gate as memory_put: Team/Curated have no backing store,
    // so promotions cannot mint records labeled with an unbacked tier either.
    if matches!(
        request.to_tier,
        tandem_memory::GovernedMemoryTier::Team | tandem_memory::GovernedMemoryTier::Curated
    ) {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "tier_not_storage_backed",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    if capability.memory.require_review_for_promote
        && (request.review.approval_id.is_none() || request.review.reviewer_id.is_none())
    {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "review approval required for promote",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    let store = open_global_memory_store_for_state(state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let local_unrestricted = crate::memory::subject::local_memory_subjects_are_unrestricted(
        tenant_context,
        verified_tenant_context,
    );
    let scope = if local_unrestricted {
        tandem_memory::MemoryReadScope::trusted_unrestricted(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        })
    } else {
        let (owner_org_unit_id, caller_subject) = trusted_memory_database_scope(
            tenant_context,
            verified_tenant_context,
            Some(&capability.subject),
        )?;
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        });
        scope.org_unit = owner_org_unit_id;
        scope.subject = caller_subject;
        scope
    };
    let source = match with_verified_memory_decrypt_principal(
        verified_tenant_context,
        store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {
            scope: scope.clone(),
            id: request.source_memory_id.clone(),
        }),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        tandem_memory::MemoryStoreReadResult::GlobalRecord(record) => record,
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let Some(source) = source else {
        let scrub_report = ScrubReport {
            status: ScrubStatus::Blocked,
            redactions: 0,
            block_reason: Some("source memory missing or previously blocked".to_string()),
        };
        let audit_id = Uuid::new_v4().to_string();
        let partition_key = format!(
            "{}/{}/{}/{}",
            request.partition.org_id,
            request.partition.workspace_id,
            request.partition.project_id,
            request.to_tier
        );
        let linkage = json!({
            "run_id": request.run_id,
            "project_id": request.partition.project_id,
            "origin_event_type": Value::Null,
            "origin_run_id": request.run_id,
            "origin_session_id": Value::Null,
            "origin_message_id": Value::Null,
            "partition_key": partition_key,
            "promote_run_id": Value::Null,
            "approval_id": request.review.approval_id,
            "artifact_refs": [],
        });
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report
                    .block_reason
                    .as_ref()
                    .map(|detail| format!("{detail}{}", memory_linkage_detail(&linkage))),
                created_at_ms: crate::now_ms(),
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": Value::Null,
                "classification": Value::Null,
                "artifactRefs": [],
                "visibility": Value::Null,
                "scrubStatus": scrub_report.status,
                "linkage": linkage,
                "detail": scrub_report.block_reason.clone(),
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
            policy_decision_id: None,
        });
    };
    let derived = source.metadata.as_ref().is_some_and(|metadata|
        metadata.get(tandem_memory::DERIVED_MEMORY_LINEAGE_METADATA_KEY).is_some());
    let target_reference = if derived {
        Some(tandem_memory::CanonicalMemoryRestriction::from_global_record(&source, &scope.tenant)
            .map_err(|_| StatusCode::FORBIDDEN)?.source_reference())
    } else { None };
    if derived
        && !global_memory_record_visible_to_verified_request(
            state, tenant_context, verified_tenant_context, store.as_ref(), &scope, &source,
            Some(&distillation_access_filter(verified_tenant_context, &capability.subject)),
        ).await {
        return Err(StatusCode::NOT_FOUND);
    }
    let scrub_report = scrub_content(&source.content);
    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
    let partition_key = format!(
        "{}/{}/{}/{}",
        request.partition.org_id,
        request.partition.workspace_id,
        request.partition.project_id,
        request.to_tier
    );
    let source_outcome = promotion_source_outcome_value(&request, &source);
    let require_scope_metadata =
        crate::memory::policy_status::current_memory_context_policy_status().strict_required;
    let scope_decision =
        tandem_memory::memory_promotion_scope_decision_for_context_with_enterprise_mode(
            &request.partition,
            request.to_tier,
            &request.review,
            source.metadata.as_ref(),
            request.authority_job_context.as_ref(),
            require_scope_metadata,
            now,
        )
        .map_err(|error| {
            tracing::warn!("invalid knowledge scope metadata on memory promotion: {error}");
            StatusCode::FORBIDDEN
        })?;
    if !scope_decision.allowed {
        let policy_decision = record_memory_promotion_policy_decision(
            state,
            tenant_context,
            &request,
            &capability.subject,
            Some(&source),
            &scrub_report,
            &audit_id,
            tandem_types::PolicyDecisionEffect::Deny,
            &scope_decision.reason_code,
            "knowledge scope promotion denied",
            Some(&source_outcome),
        )
        .await;
        let policy_decision_id = policy_decision
            .as_ref()
            .map(|record| record.decision_id.clone());
        let linkage = memory_linkage(&source);
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: Some(format!(
                    "{} policy_decision_id={}{}",
                    scope_decision.reason_code,
                    policy_decision_id.clone().unwrap_or_default(),
                    memory_linkage_detail(&linkage)
                )),
                created_at_ms: now,
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id.clone(),
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": memory_kind_label(&source.source_type),
                "classification": memory_classification_label(source.metadata.as_ref()),
                "artifactRefs": memory_artifact_refs(source.metadata.as_ref()),
                "visibility": source.visibility,
                "scrubStatus": scrub_report.status,
                "sourceOutcome": source_outcome,
                "policyDecisionID": policy_decision_id,
                "linkage": linkage,
                "detail": scope_decision.reason_code,
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
            policy_decision_id,
        });
    }
    if let Some(reason) = promotion_outcome_block_reason(&request, &source) {
        let policy_decision = record_memory_promotion_policy_decision(
            state,
            tenant_context,
            &request,
            &capability.subject,
            Some(&source),
            &scrub_report,
            &audit_id,
            tandem_types::PolicyDecisionEffect::Deny,
            "source_outcome_not_approved",
            &reason,
            Some(&source_outcome),
        )
        .await;
        let policy_decision_id = policy_decision
            .as_ref()
            .map(|record| record.decision_id.clone());
        let linkage = memory_linkage(&source);
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: Some(format!(
                    "{reason} scrub_status={} policy_decision_id={}{}",
                    serde_json::to_string(&scrub_report.status).unwrap_or_default(),
                    policy_decision_id.clone().unwrap_or_default(),
                    memory_linkage_detail(&linkage)
                )),
                created_at_ms: now,
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": memory_kind_label(&source.source_type),
                "classification": memory_classification_label(source.metadata.as_ref()),
                "artifactRefs": memory_artifact_refs(source.metadata.as_ref()),
                "visibility": source.visibility,
                "scrubStatus": scrub_report.status,
                "sourceOutcome": source_outcome,
                "policyDecisionID": policy_decision_id,
                "linkage": linkage,
                "detail": reason,
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
            policy_decision_id,
        });
    }
    let source_trust_label = memory_record_trust_label(source.metadata.as_ref())
        .unwrap_or(tandem_memory::MemoryTrustLabel::SystemGenerated);
    if !source_trust_label.is_trusted_for_promotion()
        && !memory_review_has_evidence(&request.review)
    {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "untrusted memory promotion requires review evidence",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    let linkage = memory_linkage(&source);
    if scrub_report.status == ScrubStatus::Blocked {
        let policy_decision = record_memory_promotion_policy_decision(
            state,
            tenant_context,
            &request,
            &capability.subject,
            Some(&source),
            &scrub_report,
            &audit_id,
            tandem_types::PolicyDecisionEffect::Deny,
            "scrub_blocked",
            scrub_report
                .block_reason
                .as_deref()
                .unwrap_or("memory promotion scrub blocked"),
            Some(&source_outcome),
        )
        .await;
        let policy_decision_id = policy_decision
            .as_ref()
            .map(|record| record.decision_id.clone());
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report.block_reason.as_ref().map(|detail| {
                    format!(
                        "{detail} policy_decision_id={}{}",
                        policy_decision_id.clone().unwrap_or_default(),
                        memory_linkage_detail(&linkage)
                    )
                }),
                created_at_ms: now,
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": memory_kind_label(&source.source_type),
                "classification": memory_classification_label(source.metadata.as_ref()),
                "artifactRefs": memory_artifact_refs(source.metadata.as_ref()),
                "visibility": source.visibility,
                "scrubStatus": scrub_report.status,
                "sourceOutcome": source_outcome,
                "policyDecisionID": policy_decision_id,
                "linkage": linkage,
                "detail": scrub_report.block_reason.clone(),
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
            policy_decision_id,
        });
    }
    let new_id = source.id.clone();
    let policy_decision = record_memory_promotion_policy_decision(
        state,
        tenant_context,
        &request,
        &capability.subject,
        Some(&source),
        &scrub_report,
        &audit_id,
        tandem_types::PolicyDecisionEffect::Allow,
        "memory_promotion_allowed",
        "approved memory promotion allowed",
        Some(&source_outcome),
    )
    .await;
    let policy_decision_id = policy_decision
        .as_ref()
        .map(|record| record.decision_id.clone());
    if let Some(record) = policy_decision
        .as_ref()
        .filter(|record| !matches!(record.decision, tandem_types::PolicyDecisionEffect::Allow))
    {
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: Some(format!(
                    "{} policy_decision_id={}{}",
                    record.reason,
                    policy_decision_id.clone().unwrap_or_default(),
                    memory_linkage_detail(&linkage)
                )),
                created_at_ms: now,
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": memory_kind_label(&source.source_type),
                "classification": memory_classification_label(source.metadata.as_ref()),
                "artifactRefs": memory_artifact_refs(source.metadata.as_ref()),
                "visibility": source.visibility,
                "scrubStatus": scrub_report.status,
                "sourceOutcome": source_outcome,
                "policyDecisionID": policy_decision_id,
                "linkage": linkage,
                "detail": record.reason,
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
            policy_decision_id,
        });
    }
    let governance = MemoryPromotionGovernanceEvidence {
        audit_id: audit_id.clone(),
        policy_decision_id: policy_decision_id.clone(),
        scrub_report: scrub_report.clone(),
        source_outcome: source_outcome.clone(),
    };
    let next_metadata =
        memory_promote_metadata(source.metadata.as_ref(), &request, now, &governance);
    let next_provenance = memory_promote_provenance(
        source.provenance.as_ref(),
        &request,
        &partition_key,
        now,
        tenant_context,
        &governance,
    );
    let classification = memory_classification_label(next_metadata.as_ref());
    let artifact_refs = memory_artifact_refs(next_metadata.as_ref());
    let artifact_ref_labels = artifact_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let kind = memory_kind_label(&source.source_type);
    let promote_detail = format!(
        "kind={} classification={} artifact_refs={} visibility=shared tier={} partition_key={} source_memory_id={} approval_id={} scrub_status={} policy_decision_id={}{}",
        kind,
        classification,
        artifact_ref_labels,
        request.to_tier,
        partition_key,
        source_memory_id,
        request.review.approval_id.clone().unwrap_or_default(),
        serde_json::to_string(&scrub_report.status).unwrap_or_default(),
        policy_decision_id.clone().unwrap_or_default(),
        memory_linkage_detail(&memory_linkage_from_parts(
            &source.run_id,
            source.project_tag.as_deref(),
            next_metadata.as_ref(),
            Some(&next_provenance),
        ))
    );
    let authority = if derived {
        // Canonical source reads must stay outside publication/target writer
        // locks. Repeat after the awaited policy/audit preparation above.
        let lineage = DerivedMemoryLineage::from_metadata(source.metadata.as_ref())
            .map_err(|_| StatusCode::FORBIDDEN)?.ok_or(StatusCode::FORBIDDEN)?;
        let filter = with_verified_memory_decrypt_principal(verified_tenant_context,
            crate::memory::derived_lineage::resolved_filter_for_lineage(
                state, tenant_context, store.as_ref(), &scope, &lineage,
                distillation_access_filter(verified_tenant_context, &capability.subject),
            ),
        ).await.ok_or(StatusCode::NOT_FOUND)?;
        let decision = tandem_memory::memory_promotion_scope_decision_for_context_with_enterprise_mode(
            &request.partition, request.to_tier, &request.review, source.metadata.as_ref(),
            request.authority_job_context.as_ref(), require_scope_metadata, crate::now_ms(),
        ).map_err(|_| StatusCode::FORBIDDEN)?;
        if !decision.allowed { return Err(StatusCode::FORBIDDEN); }
        let operation_request = request.clone();
        let operation_metadata = source.metadata.clone();
        let capability_expires_at_ms = capability.expires_at;
        let authority = derived_memory_commit_authority_with_lineage(
            state, tenant_context, verified_tenant_context, lineage, filter, Some(source.clone()),
            move |now| now < capability_expires_at_ms
                && tandem_memory::memory_promotion_scope_decision_for_context_with_enterprise_mode(
                &operation_request.partition, operation_request.to_tier, &operation_request.review,
                operation_metadata.as_ref(), operation_request.authority_job_context.as_ref(),
                require_scope_metadata, now,
            ).is_ok_and(|decision| decision.allowed),
        );
        Some((authority, target_reference.ok_or(StatusCode::FORBIDDEN)?))
    } else { None };
    let commit_state = state.clone();
    let commit_tenant = tenant_context.clone();
    let commit_verified = verified_tenant_context.cloned();
    let classification = classification.to_owned();
    let kind = kind.to_owned();
    let commit = async move {
        let state = &commit_state;
        let tenant_context = &commit_tenant;
        let mutation = tandem_memory::MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope, id: new_id.clone(), visibility: "shared".to_string(), demoted: false,
            metadata: next_metadata.clone(), provenance: Some(next_provenance.clone()),
        };
        let updated = if let Some((authority, expected)) = authority {
            with_verified_memory_decrypt_principal(commit_verified.as_ref(),
                store.mutate_with_commit_authority_if_unchanged(mutation, expected, authority)).await
        } else {
            with_verified_memory_decrypt_principal(commit_verified.as_ref(), store.mutate(mutation)).await
        }.map_err(derived_memory_commit_error_status)?;
        if !matches!(updated, tandem_memory::MemoryStoreMutationResult::Changed(true)) {
            return Err(StatusCode::NOT_FOUND);
        }
    append_memory_audit(
        &state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_promote".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(new_id.clone()),
            source_memory_id: Some(source_memory_id.clone()),
            to_tier: Some(request.to_tier),
            partition_key: format!(
                "{}/{}/{}/{}",
                request.partition.org_id,
                request.partition.workspace_id,
                request.partition.project_id,
                request.to_tier
            ),
            actor: capability.subject,
            status: "ok".to_string(),
            detail: Some(promote_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.promote",
        json!({
            "runID": request.run_id,
            "sourceMemoryID": source_memory_id,
            "memoryID": new_id,
            "kind": kind,
            "classification": classification,
            "artifactRefs": artifact_refs,
            "visibility": "shared",
            "toTier": request.to_tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_from_parts(
                &source.run_id,
                source.project_tag.as_deref(),
                next_metadata.as_ref(),
                Some(&next_provenance),
            ),
            "approvalID": request.review.approval_id,
            "auditID": audit_id,
            "policyDecisionID": policy_decision_id,
            "scrubStatus": scrub_report.status,
            "sourceOutcome": source_outcome,
            "governance": memory_promotion_governance_payload(
                next_metadata.as_ref(),
                Some(&next_provenance),
            ),
        }),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "memory.updated",
        json!({
            "memoryID": new_id,
            "runID": request.run_id,
            "action": "promote",
            "kind": kind,
            "classification": classification,
            "artifactRefs": artifact_refs,
            "visibility": "shared",
            "tier": request.to_tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_from_parts(
                &source.run_id,
                source.project_tag.as_deref(),
                next_metadata.as_ref(),
                Some(&next_provenance),
            ),
            "sourceMemoryID": source_memory_id,
            "approvalID": request.review.approval_id,
            "auditID": audit_id,
            "policyDecisionID": policy_decision_id,
            "scrubStatus": scrub_report.status,
            "sourceOutcome": source_outcome,
            "governance": memory_promotion_governance_payload(
                next_metadata.as_ref(),
                Some(&next_provenance),
            ),
        }),
    );
    Ok(MemoryPromoteResponse {
        promoted: true,
        new_memory_id: Some(new_id),
        to_tier: request.to_tier,
        scrub_report,
        audit_id,
        policy_decision_id,
    })
    };
    if derived {
        commit_derived_memory_with_current_policy(state, tenant_context, verified_tenant_context, commit).await?
    } else {
        commit.await
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_memory_promotion_policy_decision(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemoryPromoteRequest,
    actor: &str,
    source: Option<&GlobalMemoryRecord>,
    scrub_report: &ScrubReport,
    audit_id: &str,
    decision: tandem_types::PolicyDecisionEffect,
    reason_code: &str,
    reason: &str,
    source_outcome: Option<&Value>,
) -> Option<tandem_types::PolicyDecisionRecord> {
    let decision_id = format!("policy_decision_{}", Uuid::new_v4().simple());
    let data_class = source.and_then(|record| {
        let target = MemorySourceAccessTarget::from_metadata(record.metadata.as_ref());
        memory_record_data_class(record, target.as_ref())
    });
    let metadata = json!({
        "memory_promotion": {
            "source_memory_id": request.source_memory_id,
            "from_tier": request.from_tier,
            "to_tier": request.to_tier,
            "partition_key": memory_target_partition_key(&request.partition, request.to_tier),
            "reason": request.reason,
            "scrub_report": scrub_report,
            "source_outcome": source_outcome.cloned().unwrap_or(Value::Null),
            "source_run_id": source.map(|record| record.run_id.clone()),
            "source_visibility": source.map(|record| record.visibility.clone()),
            "source_kind": source.map(|record| memory_kind_label(&record.source_type).to_string()),
            "classification": source.map(|record| memory_classification_label(record.metadata.as_ref()).to_string()),
        }
    });
    let record = tandem_types::PolicyDecisionRecord {
        decision_id: decision_id.clone(),
        tenant_context: tenant_context.clone(),
        requester_context: None,
        actor_id: Some(actor.to_string()),
        session_id: source.and_then(|record| record.session_id.clone()),
        message_id: source.and_then(|record| record.message_id.clone()),
        run_id: Some(request.run_id.clone()),
        automation_id: None,
        node_id: None,
        tool: Some("memory.promote".to_string()),
        resource: None,
        data_classes: data_class.into_iter().collect(),
        risk_tier: Some("memory_promotion".to_string()),
        decision,
        reason_code: reason_code.to_string(),
        reason: reason.to_string(),
        policy_id: Some("memory_promotion_governance".to_string()),
        grant_id: None,
        approval_id: request.review.approval_id.clone(),
        audit_event_id: Some(audit_id.to_string()),
        created_at_ms: crate::now_ms(),
        metadata,
    };
    match state.record_policy_decision(record).await {
        Ok(record) => Some(record),
        Err(error) => {
            tracing::warn!("failed to record memory promotion policy decision: {error:?}");
            None
        }
    }
}
