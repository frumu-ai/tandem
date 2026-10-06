// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use tandem_memory::derived_lineage::{
    CanonicalInputReference, CanonicalMemoryRestriction, DerivedMemoryLineage,
    metadata_with_derived_lineage,
};
use tandem_types::{canonical_message_digest, Message, MessagePart, MessageRole};

/// Only this server resolver constructs extraction authority. Neither request
/// text nor model output can construct a batch or select a wider destination.
struct TrustedDistillationBatch {
    conversation: Vec<String>,
    lineage: DerivedMemoryLineage,
    source_message_ids: Vec<String>,
}

fn distillation_source_data_classes(lineage: &DerivedMemoryLineage) -> Vec<tandem_types::DataClass> {
    fn collect(lineage: &DerivedMemoryLineage, output: &mut Vec<tandem_types::DataClass>) {
        for source in &lineage.sources {
            if !output.contains(&source.target.data_class) { output.push(source.target.data_class); }
            if let Some(nested) = &source.nested_lineage { collect(nested, output); }
        }
    }
    let mut output = Vec::new();
    collect(lineage, &mut output);
    output
}

fn distillation_classification(lineage: &DerivedMemoryLineage) -> tandem_memory::MemoryClassification {
    if distillation_source_data_classes(lineage).iter().any(|class| {
        !matches!(class, tandem_types::DataClass::Public | tandem_types::DataClass::Internal)
    }) { tandem_memory::MemoryClassification::Restricted } else { tandem_memory::MemoryClassification::Internal }
}

fn distillation_egress_classes(lineage: &DerivedMemoryLineage) -> Vec<tandem_data_boundary::SensitiveDataClass> {
    use tandem_data_boundary::SensitiveDataClass as Sensitive;
    use tandem_types::DataClass;
    distillation_source_data_classes(lineage).into_iter().filter_map(|class| match class {
        DataClass::Public | DataClass::Internal => None,
        DataClass::Credential => Some(Sensitive::Credential),
        DataClass::SourceCode => Some(Sensitive::SourceCode),
        DataClass::FinancialRecord => Some(Sensitive::Financial),
        DataClass::CustomerData => Some(Sensitive::CustomerData),
        DataClass::Confidential | DataClass::Executive => Some(Sensitive::ProprietaryBusinessData),
        DataClass::Restricted | DataClass::Regulated => Some(Sensitive::UnknownSensitive),
    }).collect()
}

fn distillation_read_scope(
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    subject: &str,
) -> Result<tandem_memory::MemoryReadScope, StatusCode> {
    let mut scope = tandem_memory::MemoryReadScope::tenant(context_memory_tenant_scope(tenant));
    // A canonical subject-private session needs no department membership.
    // Department-shared sources still pass the full current source filter.
    scope.org_unit = crate::memory::subject::active_org_unit(verified);
    scope.subject = Some(subject.to_string());
    Ok(scope)
}

fn distillation_access_filter(
    verified: Option<&VerifiedTenantContext>, subject: &str,
) -> MemoryAccessFilter {
    crate::memory::read_policy::governed_memory_read_filter(
        crate::config::env::resolve_runtime_auth_mode(), verified, false, crate::now_ms(),
    ).unwrap_or_else(|| MemoryAccessFilter::local_noop(crate::now_ms()))
        .with_caller_subject(subject.to_string())
}

async fn distillation_canonical_memory(
    state: &AppState, tenant: &TenantContext, verified: Option<&VerifiedTenantContext>,
    subject: &str, id: &str,
) -> Result<(GlobalMemoryRecord, CanonicalMemoryRestriction), StatusCode> {
    let store = open_global_memory_store_for_state(state).await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut source_scope = distillation_read_scope(tenant, verified, subject)?;
    // A point lookup keeps tenant and private-owner predicates. Department
    // eligibility is evaluated by the current governed filter, so an explicit
    // source grant can authorize a caller outside the collector's department.
    source_scope.org_unit = None;
        let record = match with_verified_memory_decrypt_principal(verified,
            store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {
                scope: source_scope.clone(), id: id.to_string(),
            }),
        ).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            tandem_memory::MemoryStoreReadResult::GlobalRecord(record) => record,
            _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        };
        let record = record.ok_or(StatusCode::NOT_FOUND)?;
        if record.demoted || record.expires_at_ms.is_some_and(|expiry| expiry <= crate::now_ms())
            || !matches!(record.redaction_status.as_str(), "passed" | "redacted") {
            return Err(StatusCode::NOT_FOUND);
        }
        let filter = with_verified_memory_decrypt_principal(verified,
            crate::memory::derived_lineage::resolved_filter_for_record(
                state, tenant, store.as_ref(), &source_scope, &record,
                distillation_access_filter(verified, subject),
            ),
        ).await.ok_or(StatusCode::NOT_FOUND)?;
        if !filter.allows_global_record(&record) { return Err(StatusCode::NOT_FOUND); }
        let restriction = CanonicalMemoryRestriction::from_global_record(&record, &source_scope.tenant)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        Ok((record, restriction))
}

fn canonical_message_text(message: &Message) -> String {
    message.parts.iter().filter_map(|part| match part {
        MessagePart::Text {text} => Some(text.as_str()), _ => None,
    }).collect::<Vec<_>>().join("\n")
}

async fn resolve_distillation_batches(
    state: &AppState, tenant: &TenantContext, verified: Option<&VerifiedTenantContext>,
    subject: &str, input: &ContextDistillRequest,
) -> Result<Vec<TrustedDistillationBatch>, StatusCode> {
    use std::collections::BTreeSet;
    if input.message_ids.len() > 128 || input.source_memory_ids.len() > 32
        || input.message_ids.iter().collect::<BTreeSet<_>>().len() != input.message_ids.len()
        || input.source_memory_ids.iter().collect::<BTreeSet<_>>().len() != input.source_memory_ids.len() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let session = state.storage.get_session(&input.session_id).await.ok_or(StatusCode::NOT_FOUND)?;
    super::sessions_actor_scope::ensure_same_session_actor(tenant, &session.tenant_context)?;
    let selected = if input.message_ids.is_empty() {
        if input.source_memory_ids.is_empty() {
            session.messages.iter().filter(|message| matches!(&message.role, MessageRole::User | MessageRole::Assistant))
                .collect::<Vec<_>>()
        } else { Vec::new() }
    } else {
        let mut selected = Vec::new();
        // Canonical order is independent of request ordering.
        for message in &session.messages {
            if input.message_ids.contains(&message.id) { selected.push(message); }
        }
        if selected.len() != input.message_ids.len() { return Err(StatusCode::NOT_FOUND); }
        selected
    };
    if selected.len() > 128 { return Err(StatusCode::BAD_REQUEST); }
    let projection = selected.iter().map(|message| canonical_message_text(message)).collect::<Vec<_>>();
    if !input.conversation.is_empty() && input.conversation != projection {
        return Err(StatusCode::BAD_REQUEST);
    }
    let scope = distillation_read_scope(tenant, verified, subject)?;
    let mut batches = Vec::new();
    if !selected.is_empty() {
        let mut refs = Vec::new();
        let mut sources = Vec::new();
        let mut pending = selected.iter().map(|message| message.id.clone()).collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) { continue; }
            if seen.len() > 128 { return Err(StatusCode::BAD_REQUEST); }
            let message = session.messages.iter().find(|message| message.id == id).ok_or(StatusCode::NOT_FOUND)?;
            let digest = canonical_message_digest(message);
            refs.push(CanonicalInputReference::SessionMessage {
                session_id: session.id.clone(), message_id: id, body_digest: digest.clone(),
            });
            match &message.role {
                MessageRole::User => {}
                MessageRole::Assistant => {
                    let source = message.source_lineage.as_ref().ok_or(StatusCode::BAD_REQUEST)?;
                    if source.schema_version != 1 || !source.complete || source.run_id.is_empty()
                        || source.subject != subject || source.message_digest != digest
                        || !super::tenant_matches(tenant, &source.tenant_context)
                        || source.tenant_context.actor_id != session.tenant_context.actor_id {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    pending.extend(source.input_message_ids.iter().cloned());
                    let index = session.messages.iter().position(|item| item.id == message.id).ok_or(StatusCode::BAD_REQUEST)?;
                    if source.input_message_ids.iter().any(|id| {
                        session.messages.iter().position(|item| &item.id == id).is_none_or(|source_index| source_index >= index)
                    }) { return Err(StatusCode::BAD_REQUEST); }
                    for reference in &source.included_memory {
                        let (_, restriction) = distillation_canonical_memory(state, tenant, verified, subject, &reference.memory_id).await?;
                        if restriction.source_reference() != *reference { return Err(StatusCode::NOT_FOUND); }
                        if !sources.iter().any(|source: &CanonicalMemoryRestriction| source.memory_id == restriction.memory_id) {
                            refs.push(CanonicalInputReference::Memory {source: reference.clone()});
                            sources.push(restriction);
                        }
                    }
                }
                _ => return Err(StatusCode::BAD_REQUEST),
            }
        }
        refs.sort_by_key(|reference| serde_json::to_string(reference).unwrap_or_default());
        sources.sort_by(|a,b| a.memory_id.cmp(&b.memory_id));
        let lineage = DerivedMemoryLineage::new(Some(subject.to_string()), scope.org_unit.clone(), sources, refs)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        batches.push(TrustedDistillationBatch {
            conversation: projection, lineage,
            source_message_ids: selected.iter().map(|message| message.id.clone()).collect(),
        });
    }
    // Each canonical memory is extracted in isolation. Private session text
    // cannot influence a model call authorized to produce shared material.
    for id in &input.source_memory_ids {
        let (record, source) = distillation_canonical_memory(state, tenant, verified, subject, id).await?;
        let owner = source.target.owner_subject.clone();
        let department = if source.target.evidence == tandem_memory::types::GovernedReadEvidence::TenantLocalMemory {
            source.target.owner_org_unit_id.clone()
        } else { None };
        let lineage = DerivedMemoryLineage::new(owner, department, vec![source.clone()],
            vec![CanonicalInputReference::Memory {source: source.source_reference()}])
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        batches.push(TrustedDistillationBatch {
            conversation: vec![record.content], lineage, source_message_ids: Vec::new(),
        });
    }
    if batches.is_empty() { return Err(StatusCode::BAD_REQUEST); }
    Ok(batches)
}

async fn global_memory_record_visible_to_verified_request(
    state: &AppState, tenant: &TenantContext, verified: Option<&VerifiedTenantContext>,
    store: &dyn tandem_memory::MemoryStore, scope: &tandem_memory::MemoryReadScope,
    record: &GlobalMemoryRecord, access_filter: Option<&MemoryAccessFilter>,
) -> bool {
    match DerivedMemoryLineage::from_metadata(record.metadata.as_ref()) {
        Ok(None) => global_memory_record_visible_to_access_filter(record, access_filter),
        Err(_) => false,
        Ok(Some(_)) => {
            let filter = access_filter.cloned().unwrap_or_else(|| MemoryAccessFilter::local_noop(crate::now_ms()));
            let filter = with_verified_memory_decrypt_principal(verified,
                crate::memory::derived_lineage::resolved_filter_for_record(
                    state, tenant, store, scope, record, filter,
                ),
            ).await;
            filter.is_some_and(|filter| filter.allows_global_record(record))
        }
    }
}

fn workflow_learning_candidate_memory_metadata(
    candidate: &WorkflowLearningCandidate, metadata: Option<Value>,
) -> Result<Option<Value>, StatusCode> {
    match crate::memory::derived_lineage::candidate_lineage(candidate).map_err(|_| StatusCode::NOT_FOUND)? {
        Some(lineage) => {
            let mut metadata = metadata.unwrap_or_else(|| json!({}));
            let object = metadata.as_object_mut().ok_or(StatusCode::BAD_REQUEST)?;
            // This flag prevents the collector's active department from
            // inventing a narrower floor for already authorized shared sources.
            // A private owner's additional restriction still applies.
            object.insert("tenant_shared".to_owned(), json!(lineage.owner_org_unit_id.is_none()));
            let metadata = metadata_with_derived_lineage(Some(metadata), &lineage).map_err(|_| StatusCode::BAD_REQUEST)?;
            Ok(memory_metadata_with_owner_org_unit(metadata, lineage.owner_org_unit_id.as_deref()))
        }
        None => Ok(metadata),
    }
}
