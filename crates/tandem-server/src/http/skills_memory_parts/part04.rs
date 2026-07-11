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
    let updated = with_verified_memory_decrypt_principal(
        verified_tenant_context,
        store.mutate(tandem_memory::MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope,
            id: new_id.clone(),
            visibility: "shared".to_string(),
            demoted: false,
            metadata: next_metadata.clone(),
            provenance: Some(next_provenance.clone()),
        }),
    )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !matches!(updated, tandem_memory::MemoryStoreMutationResult::Changed(true)) {
        return Err(StatusCode::NOT_FOUND);
    }
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

fn memory_search_rendering_role(label: tandem_memory::MemoryTrustLabel) -> &'static str {
    if label.is_trusted_for_promotion() {
        "context"
    } else {
        "evidence"
    }
}

fn memory_trust_result_payload(label: tandem_memory::MemoryTrustLabel) -> Value {
    json!({
        "label": label.as_str(),
        "trusted_for_promotion": label.is_trusted_for_promotion(),
        "rendering_role": memory_search_rendering_role(label),
    })
}

fn memory_record_storage_tier(
    record: &GlobalMemoryRecord,
) -> Option<tandem_memory::GovernedMemoryTier> {
    let tier = record
        .metadata
        .as_ref()
        .and_then(|value| value.pointer("/promotion/to_tier"))
        .or_else(|| {
            record
                .provenance
                .as_ref()
                .and_then(|value| value.pointer("/promotion/to_tier"))
        })
        .or_else(|| {
            record
                .provenance
                .as_ref()
                .and_then(|value| value.pointer("/partition/tier"))
        })?
        .as_str()?;
    match tier {
        "session" => Some(tandem_memory::GovernedMemoryTier::Session),
        "project" => Some(tandem_memory::GovernedMemoryTier::Project),
        "team" => Some(tandem_memory::GovernedMemoryTier::Team),
        "curated" => Some(tandem_memory::GovernedMemoryTier::Curated),
        _ => None,
    }
}

pub(super) async fn memory_search(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<MemorySearchInput>,
) -> Result<Json<MemorySearchResponse>, StatusCode> {
    let request = input.request;
    let capability = validate_memory_search_capability_with_guardrail(
        &state,
        &tenant_context,
        verified_tenant_context.as_deref(),
        &request,
        input.capability,
    )
    .await?;
    let requested_scopes = if request.read_scopes.is_empty() {
        capability.memory.read_tiers.clone()
    } else {
        request.read_scopes.clone()
    };
    let mut scopes_used = Vec::new();
    let mut blocked_scopes = Vec::new();
    for scope in &requested_scopes {
        if capability.memory.read_tiers.contains(scope) {
            scopes_used.push(scope.clone());
        } else {
            blocked_scopes.push(scope.clone());
        }
    }
    let requested_limit = request.limit.unwrap_or(8).clamp(1, 100);
    let gateway_limit = match validate_memory_retrieval_gateway_for_search(
        &state,
        &tenant_context,
        &request,
        &capability,
    )
    .await
    {
        Ok(limit) => limit,
        Err((status, detail)) => match emit_blocked_memory_search_guardrail(
            status,
            detail,
            capability.subject.clone(),
            &state,
            &tenant_context,
            &request,
            &requested_scopes,
            &request.partition.key(),
        )
        .await
        {
            Err(status_code) => return Err(status_code),
            Ok(_) => return Err(status),
        },
    };
    let limit = requested_limit.min(gateway_limit);
    // Storage bounds search retrieval at 100 candidates. Fetch that complete
    // window so tier/source authorization is evaluated before the requested
    // result limit instead of allowing disallowed top-ranked rows to consume it.
    let candidate_limit = 100;
    let source_access_filter =
        crate::memory::read_policy::governed_memory_read_filter_with_workflow_phase(
            crate::config::env::resolve_runtime_auth_mode(),
            verified_tenant_context.as_deref(),
            request.retrieval_gateway.is_some(),
            crate::now_ms(),
            request.workflow_phase.as_deref(),
        )
        .map(|filter| filter.with_caller_subject(capability.subject.clone()));
    let strict_source_projection_active = source_access_filter.is_some();
    let (hits, gateway_budget_exhausted) = if scopes_used.is_empty() {
        (Vec::new(), false)
    } else {
        let store = open_global_memory_store_for_state(&state)
            .await
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let (owner_org_unit_id, caller_subject) = trusted_memory_database_scope(
            &tenant_context,
            verified_tenant_context.as_deref(),
            Some(&capability.subject),
        )?;
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        });
        scope.org_unit = owner_org_unit_id;
        scope.subject = caller_subject;
        let hits = match with_verified_memory_decrypt_principal(
            verified_tenant_context.as_deref(),
            store.query(tandem_memory::MemoryStoreQueryRequest::SearchGlobalRecords {
                scope,
                user_id: capability.subject.clone(),
                query: request.query.clone(),
                limit: candidate_limit,
                project_tag: Some(request.partition.project_id.clone()),
            }),
        )
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            tandem_memory::MemoryStoreQueryResult::GlobalSearchHits(hits) => hits,
            _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        };
        let filtered = hits
            .into_iter()
            .filter(|hit| {
                memory_record_storage_tier(&hit.record)
                    .map(|tier| scopes_used.contains(&tier))
                    // Legacy records without authoritative tier provenance are
                    // treated as session data, without consulting visibility.
                    .unwrap_or_else(|| {
                        scopes_used.contains(&tandem_memory::GovernedMemoryTier::Session)
                    })
            })
            .filter(|hit| {
                let source_projection_allows = global_memory_record_visible_to_access_filter(
                    &hit.record,
                    source_access_filter.as_ref(),
                );
                if strict_source_projection_active {
                    source_projection_allows
                } else {
                    source_projection_allows || request.retrieval_gateway.is_some()
                }
            })
            .filter(|hit| memory_retrieval_gateway_allows_record(&request, &hit.record))
            .collect::<Vec<_>>();
        let (filtered, response_budget_exhausted) =
            apply_memory_retrieval_gateway_result_budgets(&request, filtered, limit);
        let (filtered, window_budget_exhausted) =
            apply_memory_retrieval_gateway_window_budgets(&state, &request, filtered).await;
        (
            filtered,
            response_budget_exhausted || window_budget_exhausted,
        )
    };
    let results = hits
        .into_iter()
        .map(|hit| {
            let trust_label = memory_record_trust_label(hit.record.metadata.as_ref())
                .unwrap_or(tandem_memory::MemoryTrustLabel::SystemGenerated);
            json!({
                "id": hit.record.id,
                "tier": memory_tier_for_visibility(&hit.record.visibility),
                "classification": memory_classification_label(hit.record.metadata.as_ref()),
                "kind": memory_kind_label(&hit.record.source_type),
                "source_type": hit.record.source_type,
                "created_at_ms": hit.record.created_at_ms,
                "content": hit.record.content,
                "score": hit.score,
                "run_id": hit.record.run_id,
                "visibility": hit.record.visibility,
                "artifact_refs": memory_artifact_refs(hit.record.metadata.as_ref()),
                "linkage": memory_linkage(&hit.record),
                "governance": memory_promotion_governance_payload(
                    hit.record.metadata.as_ref(),
                    hit.record.provenance.as_ref(),
                ),
                "influence": memory_influence_payload(&hit.record, &request.run_id),
                "memory_trust": memory_trust_result_payload(trust_label),
                "rendering_role": memory_search_rendering_role(trust_label),
                "metadata": hit.record.metadata,
                "provenance": hit.record.provenance,
            })
        })
        .collect::<Vec<_>>();
    let result_ids = results
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let result_kinds = results
        .iter()
        .filter_map(|row| row.get("kind").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let linkage = json!({
        "run_id": request.run_id,
        "project_id": request.partition.project_id,
        "origin_event_type": "memory.search",
        "origin_run_id": request.run_id,
        "origin_session_id": Value::Null,
        "origin_message_id": Value::Null,
        "partition_key": request.partition.key(),
        "promote_run_id": Value::Null,
        "approval_id": Value::Null,
        "artifact_refs": [],
    });
    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
    let search_status = if scopes_used.is_empty() && !blocked_scopes.is_empty() {
        "blocked"
    } else if gateway_budget_exhausted {
        "budget_exhausted"
    } else {
        "ok"
    };
    let search_detail = format!(
        "query={} result_count={} result_ids={} result_kinds={} requested_scopes={} scopes_used={} blocked_scopes={} retrieval_gateway={} gateway_budget_exhausted={}{}",
        request.query,
        results.len(),
        result_ids.join(","),
        result_kinds.join(","),
        requested_scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        scopes_used
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        blocked_scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        request
            .retrieval_gateway
            .as_ref()
            .map(|gateway| gateway.grant.grant_id.as_str())
            .unwrap_or("none"),
        gateway_budget_exhausted,
        memory_linkage_detail(&linkage)
    );
    append_memory_audit(
        &state,
        &tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_search".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: None,
            source_memory_id: None,
            to_tier: None,
            partition_key: request.partition.key(),
            actor: capability.subject,
            status: search_status.to_string(),
            detail: Some(search_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.search",
        json!({
            "runID": request.run_id,
            "query": request.query,
            "partitionKey": request.partition.key(),
            "resultCount": results.len(),
            "resultIDs": result_ids,
            "resultKinds": result_kinds,
            "requestedScopes": requested_scopes,
            "scopesUsed": scopes_used.clone(),
            "blockedScopes": blocked_scopes.clone(),
            "retrievalGatewayGrantID": request
                .retrieval_gateway
                .as_ref()
                .map(|gateway| gateway.grant.grant_id.clone()),
            "gatewayBudgetExhausted": gateway_budget_exhausted,
            "linkage": linkage,
            "status": search_status,
            "auditID": audit_id,
        }),
    );
    Ok(Json(MemorySearchResponse {
        results,
        scopes_used,
        blocked_scopes,
        audit_id,
    }))
}

const DEFAULT_MEMORY_RETRIEVAL_MAX_RESULTS_PER_WINDOW: u32 = 20;
const DEFAULT_MEMORY_RETRIEVAL_MAX_TOKENS_PER_WINDOW: i64 = 800;
const DEFAULT_MEMORY_RETRIEVAL_MAX_CHARS_PER_WINDOW: usize = 4_000;

fn suspicious_memory_retrieval_query_reason(query: &str) -> Option<&'static str> {
    let lowered = query.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }
    let broad_terms = [
        "dump",
        "everything",
        "entire memory",
        "all memory",
        "all documents",
        "all records",
        "full database",
        "bulk",
    ];
    if broad_terms.iter().any(|term| lowered.contains(term)) {
        return Some("retrieval query pattern blocked: broad export");
    }
    let export_patterns = [
        "export all",
        "export everything",
        "export entire",
        "export the entire",
        "export full",
        "export the full",
        "bulk export",
    ];
    if export_patterns
        .iter()
        .any(|pattern| lowered.contains(pattern))
    {
        return Some("retrieval query pattern blocked: broad export");
    }
    let starts = [
        "list all",
        "show all",
        "give me all",
        "print all",
        "return all",
    ];
    if starts.iter().any(|term| lowered.starts_with(term)) {
        return Some("retrieval query pattern blocked: broad enumeration");
    }
    None
}

async fn validate_memory_retrieval_gateway_for_search(
    state: &AppState,
    _tenant_context: &TenantContext,
    request: &MemorySearchRequest,
    capability: &MemoryCapabilityToken,
) -> Result<i64, (StatusCode, &'static str)> {
    let Some(gateway) = request.retrieval_gateway.as_ref() else {
        if capability.subject.starts_with("channel:") {
            return Err((
                StatusCode::FORBIDDEN,
                "channel memory search requires retrieval gateway",
            ));
        }
        return Ok(100);
    };
    let grant = &gateway.grant;
    if grant.revoked {
        return Err((StatusCode::FORBIDDEN, "retrieval grant revoked"));
    }
    let now = crate::now_ms();
    if grant.expires_at.is_some_and(|expires_at| expires_at <= now) {
        return Err((StatusCode::UNAUTHORIZED, "retrieval grant expired"));
    }
    if grant.subject != capability.subject {
        return Err((StatusCode::FORBIDDEN, "retrieval grant subject mismatch"));
    }
    if grant.org_id != request.partition.org_id
        || grant.workspace_id != request.partition.workspace_id
    {
        return Err((StatusCode::FORBIDDEN, "retrieval grant tenant mismatch"));
    }
    if !grant.project_ids.is_empty()
        && !grant
            .project_ids
            .iter()
            .any(|project_id| project_id == &request.partition.project_id)
    {
        return Err((StatusCode::FORBIDDEN, "retrieval grant project mismatch"));
    }
    if let Some(reason) = suspicious_memory_retrieval_query_reason(&request.query) {
        return Err((StatusCode::FORBIDDEN, reason));
    }
    if let Some(max_queries) = grant.budgets.max_queries_per_window {
        let window_ms = grant.budgets.window_ms.unwrap_or(60_000).max(1);
        let budget_key = memory_retrieval_budget_key(gateway);
        let mut windows = state.memory_retrieval_budget_windows.write().await;
        let window = windows.entry(budget_key).or_insert_with(|| {
            tandem_memory::MemoryRetrievalBudgetWindow {
                started_at_ms: now,
                query_count: 0,
                result_count: 0,
                token_count: 0,
                char_count: 0,
            }
        });
        if now.saturating_sub(window.started_at_ms) >= window_ms {
            window.started_at_ms = now;
            window.query_count = 0;
            window.result_count = 0;
            window.token_count = 0;
            window.char_count = 0;
        }
        if window.query_count >= max_queries {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "retrieval grant query budget exhausted",
            ));
        }
        window.query_count = window.query_count.saturating_add(1);
    }
    Ok(grant.budgets.max_top_k.unwrap_or(100).clamp(1, 100) as i64)
}

fn memory_retrieval_budget_key(gateway: &tandem_memory::MemoryRetrievalGatewayRequest) -> String {
    format!(
        "{}:{}:{}:{}",
        gateway.grant.grant_id,
        gateway.session_id.as_deref().unwrap_or("*"),
        gateway.channel.as_deref().unwrap_or("*"),
        gateway.user_id.as_deref().unwrap_or("*")
    )
}

fn memory_retrieval_gateway_allows_record(
    request: &MemorySearchRequest,
    record: &GlobalMemoryRecord,
) -> bool {
    let Some(gateway) = request.retrieval_gateway.as_ref() else {
        return true;
    };
    let grant = &gateway.grant;
    if !grant.project_ids.is_empty()
        && !record.project_tag.as_ref().is_some_and(|project_id| {
            grant
                .project_ids
                .iter()
                .any(|allowed| allowed == project_id)
        })
    {
        return false;
    }
    let target = MemorySourceAccessTarget::from_metadata(record.metadata.as_ref());
    if !grant.source_binding_ids.is_empty()
        && !target
            .as_ref()
            .and_then(|target| target.source_binding_id.as_ref())
            .is_some_and(|binding_id| {
                grant
                    .source_binding_ids
                    .iter()
                    .any(|allowed| allowed == binding_id)
            })
    {
        return false;
    }
    if !grant.source_object_ids.is_empty()
        && !target
            .as_ref()
            .and_then(|target| target.source_object_id.as_ref())
            .is_some_and(|source_object_id| {
                grant
                    .source_object_ids
                    .iter()
                    .any(|allowed| allowed == source_object_id)
            })
    {
        return false;
    }
    if !grant.data_classes.is_empty() {
        let Some(data_class) = memory_record_data_class(record, target.as_ref()) else {
            return false;
        };
        if !grant.data_classes.contains(&data_class) {
            return false;
        }
    }
    true
}

fn memory_record_data_class(
    record: &GlobalMemoryRecord,
    target: Option<&MemorySourceAccessTarget>,
) -> Option<tandem_enterprise_contract::DataClass> {
    if let Some(target) = target {
        return Some(target.data_class);
    }
    match memory_classification_label(record.metadata.as_ref()) {
        "internal" => Some(tandem_enterprise_contract::DataClass::Internal),
        "restricted" => Some(tandem_enterprise_contract::DataClass::Restricted),
        "confidential" => Some(tandem_enterprise_contract::DataClass::Confidential),
        "public" => Some(tandem_enterprise_contract::DataClass::Public),
        _ => None,
    }
}

fn apply_memory_retrieval_gateway_result_budgets(
    request: &MemorySearchRequest,
    hits: Vec<tandem_memory::types::GlobalMemorySearchHit>,
    limit: i64,
) -> (Vec<tandem_memory::types::GlobalMemorySearchHit>, bool) {
    let Some(gateway) = request.retrieval_gateway.as_ref() else {
        return (hits.into_iter().take(limit as usize).collect(), false);
    };
    let max_chars = gateway.grant.budgets.max_chars.unwrap_or(usize::MAX);
    let max_tokens = gateway.grant.budgets.max_tokens.unwrap_or(i64::MAX);
    let mut chars_used = 0usize;
    let mut tokens_used = 0i64;
    let mut budget_exhausted = false;
    let mut allowed = Vec::new();
    for hit in hits {
        let char_count = hit.record.content.chars().count();
        let token_count = hit.record.content.split_whitespace().count() as i64;
        if chars_used.saturating_add(char_count) > max_chars
            || tokens_used.saturating_add(token_count) > max_tokens
        {
            budget_exhausted = true;
            continue;
        }
        chars_used = chars_used.saturating_add(char_count);
        tokens_used = tokens_used.saturating_add(token_count);
        allowed.push(hit);
        if allowed.len() >= limit as usize {
            break;
        }
    }
    (allowed, budget_exhausted)
}

async fn apply_memory_retrieval_gateway_window_budgets(
    state: &AppState,
    request: &MemorySearchRequest,
    hits: Vec<tandem_memory::types::GlobalMemorySearchHit>,
) -> (Vec<tandem_memory::types::GlobalMemorySearchHit>, bool) {
    let Some(gateway) = request.retrieval_gateway.as_ref() else {
        return (hits, false);
    };
    let max_results = gateway
        .grant
        .budgets
        .max_results_per_window
        .unwrap_or(DEFAULT_MEMORY_RETRIEVAL_MAX_RESULTS_PER_WINDOW)
        .max(1);
    let max_tokens = gateway
        .grant
        .budgets
        .max_tokens_per_window
        .unwrap_or(DEFAULT_MEMORY_RETRIEVAL_MAX_TOKENS_PER_WINDOW)
        .max(1);
    let max_chars = gateway
        .grant
        .budgets
        .max_chars_per_window
        .unwrap_or(DEFAULT_MEMORY_RETRIEVAL_MAX_CHARS_PER_WINDOW)
        .max(1);
    let now = crate::now_ms();
    let window_ms = gateway.grant.budgets.window_ms.unwrap_or(60_000).max(1);
    let budget_key = memory_retrieval_budget_key(gateway);
    let mut windows = state.memory_retrieval_budget_windows.write().await;
    let window =
        windows
            .entry(budget_key)
            .or_insert_with(|| tandem_memory::MemoryRetrievalBudgetWindow {
                started_at_ms: now,
                query_count: 0,
                result_count: 0,
                token_count: 0,
                char_count: 0,
            });
    if now.saturating_sub(window.started_at_ms) >= window_ms {
        window.started_at_ms = now;
        window.query_count = 0;
        window.result_count = 0;
        window.token_count = 0;
        window.char_count = 0;
    }

    let mut allowed = Vec::new();
    let mut budget_exhausted = false;
    for hit in hits {
        let token_count = hit.record.content.split_whitespace().count() as i64;
        let char_count = hit.record.content.chars().count();
        if window.result_count >= max_results
            || window.token_count.saturating_add(token_count) > max_tokens
            || window.char_count.saturating_add(char_count) > max_chars
        {
            budget_exhausted = true;
            continue;
        }
        window.result_count = window.result_count.saturating_add(1);
        window.token_count = window.token_count.saturating_add(token_count);
        window.char_count = window.char_count.saturating_add(char_count);
        allowed.push(hit);
    }
    (allowed, budget_exhausted)
}

fn global_memory_record_visible_to_access_filter(
    record: &GlobalMemoryRecord,
    access_filter: Option<&MemoryAccessFilter>,
) -> bool {
    access_filter
        .map(|filter| filter.allows_global_record(record))
        .unwrap_or_else(|| {
            MemorySourceAccessTarget::from_metadata(record.metadata.as_ref()).is_none()
        })
}

pub(super) async fn memory_demote(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<MemoryDemoteInput>,
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
    // Preserve the legacy tenant-authenticated 403 ownership response without
    // weakening verified enterprise callers' scoped point-read behavior.
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
            id: input.id.clone(),
        }),
    )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        tandem_memory::MemoryStoreReadResult::GlobalRecord(record) => record,
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let Some(record) = record else {
        emit_missing_memory_demote_audit(
            &state,
            &tenant_context,
            &input.run_id,
            &input.id,
            "memory not found",
        )
        .await?;
        return Err(StatusCode::NOT_FOUND);
    };
    enforce_memory_record_ownership_for_mutation(
        &tenant_context,
        verified_tenant_context.as_deref(),
        &record.user_id,
    )?;
    let partition_key = memory_linkage(&record)
        .get("partition_key")
        .and_then(Value::as_str)
        .unwrap_or("demoted")
        .to_string();
    let demote_detail = format!(
        "kind={} classification={} artifact_refs={} visibility=private tier={} partition_key={} demoted=true{}",
        memory_kind_label(&record.source_type),
        memory_classification_label(record.metadata.as_ref()),
        memory_artifact_refs(record.metadata.as_ref())
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(","),
        tandem_memory::GovernedMemoryTier::Session,
        partition_key,
        memory_linkage_detail(&memory_linkage(&record))
    );
    let audit_id = Uuid::new_v4().to_string();
    append_memory_audit(
        &state,
        &tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_demote".to_string(),
            run_id: input.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(input.id.clone()),
            source_memory_id: None,
            to_tier: None,
            partition_key: partition_key.clone(),
            actor: "system".to_string(),
            status: "ok".to_string(),
            detail: Some(demote_detail),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    let changed = match with_verified_memory_decrypt_principal(
        verified_tenant_context.as_deref(),
        store.mutate(tandem_memory::MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope,
            id: input.id.clone(),
            visibility: "private".to_string(),
            demoted: true,
            metadata: memory_metadata_with_owner_subject(
                record.metadata.clone(),
                Some(record.user_id.as_str()),
            ),
            provenance: record.provenance.clone(),
        }),
    )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        tandem_memory::MemoryStoreMutationResult::Changed(changed) => changed,
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    if !changed {
        return Err(StatusCode::NOT_FOUND);
    }
    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.updated",
        json!({
            "memoryID": input.id,
            "runID": input.run_id,
            "action": "demote",
            "kind": memory_kind_label(&record.source_type),
            "classification": memory_classification_label(record.metadata.as_ref()),
            "artifactRefs": memory_artifact_refs(record.metadata.as_ref()),
            "visibility": "private",
            "tier": tandem_memory::GovernedMemoryTier::Session,
            "partitionKey": partition_key,
            "demoted": true,
            "linkage": memory_linkage(&record),
            "auditID": audit_id,
        }),
    );
    Ok(Json(json!({
        "ok": true,
        "audit_id": audit_id,
    })))
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextResolveUriRequest {
    uri: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextTreeQuery {
    uri: String,
    max_depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextGenerateLayersRequest {
    node_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextDistillRequest {
    session_id: String,
    conversation: Vec<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    artifact_refs: Vec<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    importance_threshold: Option<f64>,
}

/// Context-tree operations resolve tenancy from the request's TenantContext,
/// never from the client-supplied URI/node id: a foreign tenant's node behaves
/// exactly like a nonexistent one.
fn context_memory_tenant_scope(
    tenant_context: &TenantContext,
) -> tandem_memory::types::MemoryTenantScope {
    tandem_memory::types::MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    }
}

/// Run a context memory read under the caller's decrypt principal so hosted-KMS
/// sealed rows (TAN-668) decrypt only for the scope the verified request is
/// authorized for (TAN-672).
///
/// A principal is scoped only when the caller's strict resource scope covers the
/// workspace memory space these reads operate on — context layers seal under the
/// tenant `Internal` key scope, which cannot distinguish two same-tenant nodes,
/// so a caller whose projection is narrower than the workspace (e.g. one project)
/// must NOT be able to decrypt arbitrary same-tenant summaries. When no principal
/// is scoped, sealed rows fail closed and NULL-envelope (local) rows read
/// normally — so single-tenant behavior is unchanged.
async fn read_under_decrypt_principal<F, T>(
    verified_tenant_context: Option<&VerifiedTenantContext>,
    tenant_context: &TenantContext,
    future: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    use crate::memory::decrypt_principal::{
        memory_decrypt_principal_from_verified_context, verified_resource_scope_covers,
        workspace_memory_space_resource,
    };
    let workspace_memory = workspace_memory_space_resource(tenant_context);
    let principal = verified_tenant_context
        .filter(|verified| verified_resource_scope_covers(verified, &workspace_memory))
        .and_then(|verified| {
            memory_decrypt_principal_from_verified_context(verified, crate::now_ms())
        });
    match principal {
        Some(principal) => {
            tandem_memory::decrypt_context::with_decrypt_principal(principal, future).await
        }
        None => future.await,
    }
}

pub(super) async fn context_resolve_uri(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<ContextResolveUriRequest>,
) -> Result<Json<Value>, StatusCode> {
    let manager = open_memory_manager_for_state(&state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let scope = context_memory_tenant_scope(&tenant_context);
    let node = read_under_decrypt_principal(
        verified_tenant_context.as_deref(),
        &tenant_context,
        manager.resolve_uri(&input.uri, &scope),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "node": node })))
}

pub(super) async fn context_tree(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<ContextTreeQuery>,
) -> Result<Json<Value>, StatusCode> {
    let manager = open_memory_manager_for_state(&state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let max_depth = query.max_depth.unwrap_or(3);
    let scope = context_memory_tenant_scope(&tenant_context);
    // The tree walk decrypts each node's layer summaries (get_layer), so it runs
    // under the caller's decrypt principal in hosted mode.
    let tree = read_under_decrypt_principal(
        verified_tenant_context.as_deref(),
        &tenant_context,
        manager.tree(&query.uri, max_depth, &scope),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "tree": tree })))
}

pub(super) async fn context_generate_layers(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<ContextGenerateLayersRequest>,
) -> Result<Json<Value>, StatusCode> {
    let runtime_state = state.runtime.wait();
    let providers = runtime_state.providers.clone();

    let manager = open_memory_manager_for_state(&state)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let scope = context_memory_tenant_scope(&tenant_context);
    let layer_run_id = format!("context-layer-run-{}", uuid::Uuid::new_v4());
    let layer_session_id = format!("context-layer-session-{}", input.node_id);
    let provider_egress = crate::provider_egress::memory_egress_context(
        &state,
        Some(&tenant_context),
        verified_tenant_context.as_deref(),
        Some(&layer_run_id),
        Some(&layer_session_id),
    );
    // Layer generation reads the node's existing L0/L1/L2 (get_layer) before
    // (re)writing, so it must decrypt under the caller's principal in hosted mode.
    let layer_future = read_under_decrypt_principal(
        verified_tenant_context.as_deref(),
        &tenant_context,
        manager.generate_layers_for_node_with_egress(
            &input.node_id,
            &providers,
            &scope,
            Some(&provider_egress),
        ),
    );
    crate::http::session_run_retry::scope_provider_auth_for_tenant(
        &state,
        &tenant_context,
        crate::http::session_run_retry::PromptExecutionSurface::KnowledgeBase,
        Some(&layer_session_id),
        Some(&layer_run_id),
        None,
        layer_future,
    )
    .await
    .map_err(|e| {
        tracing::warn!("Failed to generate layers: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn context_distill(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<ContextDistillRequest>,
) -> Result<Json<Value>, StatusCode> {
    let runtime_state = state.runtime.wait();
    let providers = runtime_state.providers.clone();
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
    let partition = tandem_memory::MemoryPartition {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        project_id,
        tier: tandem_memory::GovernedMemoryTier::Session,
    };
    let capability = issue_run_memory_capability(
        &run_id,
        Some(subject.as_str()),
        &partition,
        RunMemoryCapabilityPolicy::CoderWorkflow,
    );
    let provider_egress = crate::provider_egress::memory_egress_context(
        &state,
        Some(&tenant_context),
        verified_tenant_context.as_deref(),
        Some(&run_id),
        Some(&input.session_id),
    );
    let writer = GovernedDistillationWriter {
        state: state.clone(),
        tenant_context: tenant_context.clone(),
        verified_tenant_context: verified_tenant_context.as_deref().cloned(),
        partition,
        capability,
        run_id,
        workflow_id: input.workflow_id.clone(),
        artifact_refs: input.artifact_refs.clone(),
        subject,
    };
    let threshold = input.importance_threshold.unwrap_or(0.5).clamp(0.0, 1.0);
    let distiller = tandem_memory::SessionDistiller::with_threshold(Arc::new(providers), threshold)
        .with_provider_egress(provider_egress);
    let distillation_future =
        distiller.distill_with_writer(&input.session_id, &input.conversation, &writer);
    let report = crate::http::session_run_retry::scope_provider_auth_for_tenant(
        &state,
        &tenant_context,
        crate::http::session_run_retry::PromptExecutionSurface::KnowledgeBase,
        Some(&input.session_id),
        Some(&provider_auth_run_id),
        None,
        distillation_future,
    )
    .await
    .map_err(|e| {
        tracing::warn!("Failed to distill session: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
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
    })))
}

pub(super) async fn workflow_learning_candidates_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowLearningCandidateListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let status = match query.status.as_deref() {
        Some(value) => {
            Some(workflow_learning_status_from_str(value).ok_or(StatusCode::BAD_REQUEST)?)
        }
        None => None,
    };
    let kind = match query.kind.as_deref() {
        Some(value) => Some(workflow_learning_kind_from_str(value).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let mut candidates = state
        .list_workflow_learning_candidates(query.workflow_id.as_deref(), status, kind)
        .await;
    if let Some(project_id) = query.project_id.as_deref() {
        candidates.retain(|candidate| candidate.project_id == project_id);
    }
    let count = candidates.len();
    Ok(Json(json!({
        "candidates": candidates,
        "count": count,
    })))
}

pub(super) async fn workflow_learning_candidate_review(
    State(state): State<AppState>,
    Path(candidate_id): Path<String>,
    Json(input): Json<WorkflowLearningCandidateReviewRequest>,
) -> Result<Json<Value>, StatusCode> {
    let Some(candidate) = state.get_workflow_learning_candidate(&candidate_id).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    let action = input
        .action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("approve")
        .to_ascii_lowercase();
    let next_status = match action.as_str() {
        "approve" | "approved" => WorkflowLearningCandidateStatus::Approved,
        "reject" | "rejected" => WorkflowLearningCandidateStatus::Rejected,
        "applied" => WorkflowLearningCandidateStatus::Applied,
        "supersede" | "superseded" => WorkflowLearningCandidateStatus::Superseded,
        "regress" | "regressed" => WorkflowLearningCandidateStatus::Regressed,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let baseline = if matches!(
        next_status,
        WorkflowLearningCandidateStatus::Approved | WorkflowLearningCandidateStatus::Applied
    ) {
        Some(
            state
                .workflow_learning_metrics_for_workflow(&candidate.workflow_id)
                .await,
        )
    } else {
        None
    };
    let reviewed_at_ms = crate::now_ms();
    let updated = state
        .update_workflow_learning_candidate(&candidate_id, |candidate| {
            candidate.status = next_status;
            if candidate.baseline_before.is_none() {
                candidate.baseline_before = baseline.clone();
            }
            if let Some(note) = input
                .note
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidate.evidence_refs.push(json!({
                    "review_note": note,
                    "reviewer_id": input.reviewer_id,
                    "reviewed_at_ms": reviewed_at_ms,
                    "action": action,
                }));
            }
        })
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({
        "ok": true,
        "candidate": updated,
    })))
}
