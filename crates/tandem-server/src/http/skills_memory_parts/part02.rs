// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Debug, Clone)]
struct MemoryPromotionGovernanceEvidence {
    audit_id: String,
    policy_decision_id: Option<String>,
    scrub_report: ScrubReport,
    source_outcome: Value,
}

fn memory_promote_metadata(
    metadata: Option<&Value>,
    request: &MemoryPromoteRequest,
    promoted_at_ms: u64,
    governance: &MemoryPromotionGovernanceEvidence,
) -> Option<Value> {
    let mut obj = metadata
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    obj.insert(
        "promotion".to_string(),
        json!({
            "promoted_at_ms": promoted_at_ms,
            "promote_run_id": request.run_id,
            "source_memory_id": request.source_memory_id,
            "from_tier": request.from_tier,
            "to_tier": request.to_tier,
            "reason": request.reason,
            "review": {
                "required": request.review.required,
                "reviewer_id": request.review.reviewer_id,
                "approval_id": request.review.approval_id,
            },
            "governance": {
                "audit_id": governance.audit_id,
                "policy_decision_id": governance.policy_decision_id,
                "scrub_status": governance.scrub_report.status,
                "scrub_redactions": governance.scrub_report.redactions,
                "source_outcome": governance.source_outcome,
            },
        }),
    );
    let next_trust_label = if memory_review_has_evidence(&request.review) {
        tandem_memory::MemoryTrustLabel::HumanApproved
    } else {
        memory_record_trust_label(metadata)
            .unwrap_or(tandem_memory::MemoryTrustLabel::SystemGenerated)
    };
    apply_memory_trust_metadata(&mut obj, next_trust_label, "promotion");
    Some(Value::Object(obj))
}

fn memory_promote_provenance(
    provenance: Option<&Value>,
    request: &MemoryPromoteRequest,
    partition_key: &str,
    promoted_at_ms: u64,
    tenant_context: &TenantContext,
    governance: &MemoryPromotionGovernanceEvidence,
) -> Value {
    let mut obj = provenance
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    obj.insert(
        "promotion".to_string(),
        json!({
            "promoted_at_ms": promoted_at_ms,
            "promote_run_id": request.run_id,
            "source_memory_id": request.source_memory_id,
            "partition_key": partition_key,
            "to_tier": request.to_tier,
            "reviewer_id": request.review.reviewer_id,
            "approval_id": request.review.approval_id,
            "tenant_context": tenant_context,
            "governance": {
                "audit_id": governance.audit_id,
                "policy_decision_id": governance.policy_decision_id,
                "scrub_status": governance.scrub_report.status,
                "scrub_redactions": governance.scrub_report.redactions,
                "source_outcome": governance.source_outcome,
            },
        }),
    );
    obj.insert(
        "memory_trust".to_string(),
        json!({
            "label": if memory_review_has_evidence(&request.review) {
                tandem_memory::MemoryTrustLabel::HumanApproved.as_str()
            } else {
                memory_record_trust_label(provenance)
                    .unwrap_or(tandem_memory::MemoryTrustLabel::SystemGenerated)
                    .as_str()
            },
            "reviewer_id": request.review.reviewer_id,
            "approval_id": request.review.approval_id,
        }),
    );
    obj.insert(
        "governance".to_string(),
        json!({
            "audit_id": governance.audit_id,
            "policy_decision_id": governance.policy_decision_id,
            "scrub_status": governance.scrub_report.status,
            "source_outcome": governance.source_outcome,
        }),
    );
    Value::Object(obj)
}

fn memory_promotion_governance_payload(
    metadata: Option<&Value>,
    provenance: Option<&Value>,
) -> Value {
    let promotion_metadata = metadata
        .and_then(|row| row.get("promotion"))
        .and_then(|row| row.get("governance"));
    let promotion_provenance = provenance
        .and_then(|row| row.get("promotion"))
        .and_then(|row| row.get("governance"));
    let record_governance = provenance.and_then(|row| row.get("governance"));
    let lookup = |key: &str| {
        promotion_metadata
            .and_then(|row| row.get(key))
            .or_else(|| promotion_provenance.and_then(|row| row.get(key)))
            .or_else(|| record_governance.and_then(|row| row.get(key)))
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "audit_id": lookup("audit_id"),
        "policy_decision_id": lookup("policy_decision_id"),
        "scrub_status": lookup("scrub_status"),
        "source_outcome": lookup("source_outcome"),
    })
}

fn memory_promotion_policy_decision_id(
    metadata: Option<&Value>,
    provenance: Option<&Value>,
) -> Option<String> {
    memory_promotion_governance_payload(metadata, provenance)
        .get("policy_decision_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn memory_target_partition_key(
    partition: &tandem_memory::MemoryPartition,
    target_tier: tandem_memory::GovernedMemoryTier,
) -> String {
    format!(
        "{}/{}/{}/{}",
        partition.org_id, partition.workspace_id, partition.project_id, target_tier
    )
}

fn memory_influence_payload(record: &GlobalMemoryRecord, retrieval_run_id: &str) -> Value {
    let linkage = memory_linkage(record);
    json!({
        "retrieval_run_id": retrieval_run_id,
        "memory_id": record.id,
        "source_run_id": record.run_id,
        "origin_run_id": linkage.get("origin_run_id").cloned().unwrap_or(Value::Null),
        "promote_run_id": linkage.get("promote_run_id").cloned().unwrap_or(Value::Null),
        "approval_id": linkage.get("approval_id").cloned().unwrap_or(Value::Null),
        "policy_decision_id": memory_promotion_policy_decision_id(
            record.metadata.as_ref(),
            record.provenance.as_ref()
        ),
        "scrub_status": memory_promotion_governance_payload(
            record.metadata.as_ref(),
            record.provenance.as_ref()
        )
        .get("scrub_status")
        .cloned()
        .unwrap_or(Value::Null),
    })
}

fn memory_linkage(record: &GlobalMemoryRecord) -> Value {
    memory_linkage_from_parts(
        &record.run_id,
        record.project_tag.as_deref(),
        record.metadata.as_ref(),
        record.provenance.as_ref(),
    )
}

fn memory_linkage_from_parts(
    run_id: &str,
    project_id: Option<&str>,
    metadata: Option<&Value>,
    provenance: Option<&Value>,
) -> Value {
    let artifact_refs = memory_artifact_refs(metadata);
    json!({
        "run_id": run_id,
        "project_id": project_id,
        "origin_event_type": provenance
            .and_then(|row| row.get("origin_event_type"))
            .and_then(Value::as_str),
        "origin_run_id": provenance
            .and_then(|row| row.get("origin_run_id"))
            .and_then(Value::as_str)
            .or(Some(run_id)),
        "origin_session_id": provenance
            .and_then(|row| row.get("origin_session_id"))
            .and_then(Value::as_str),
        "origin_message_id": provenance
            .and_then(|row| row.get("origin_message_id"))
            .and_then(Value::as_str),
        "partition_key": provenance
            .and_then(|row| row.get("partition_key"))
            .and_then(Value::as_str),
        "promote_run_id": provenance
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("promote_run_id"))
            .and_then(Value::as_str),
        "approval_id": provenance
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("approval_id"))
            .and_then(Value::as_str),
        "artifact_refs": artifact_refs,
    })
}

fn memory_kind_label(source_type: &str) -> &str {
    source_type
}

fn memory_linkage_detail(linkage: &Value) -> String {
    let origin_run_id = linkage
        .get("origin_run_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let project_id = linkage
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let promote_run_id = linkage
        .get("promote_run_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!(
        " origin_run_id={} project_id={} promote_run_id={}",
        origin_run_id, project_id, promote_run_id
    )
}

fn memory_kind_for_request(kind: tandem_memory::MemoryContentKind) -> &'static str {
    match kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
}

fn memory_tier_for_visibility(visibility: &str) -> tandem_memory::GovernedMemoryTier {
    if visibility.eq_ignore_ascii_case("shared") {
        tandem_memory::GovernedMemoryTier::Project
    } else {
        tandem_memory::GovernedMemoryTier::Session
    }
}

fn memory_classification_label(metadata: Option<&Value>) -> &str {
    metadata
        .and_then(|row| row.get("classification"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("internal")
}

fn memory_review_has_evidence(review: &tandem_memory::PromotionReview) -> bool {
    review
        .reviewer_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && review
            .approval_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

fn normalized_outcome_status(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

fn outcome_status_from_value(value: &Value) -> Option<String> {
    if let Some(status) = value
        .get("status")
        .or_else(|| value.get("outcome"))
        .or_else(|| value.get("decision"))
        .or_else(|| value.get("disposition"))
        .and_then(Value::as_str)
    {
        return Some(normalized_outcome_status(status));
    }
    if let Some(approved) = value.get("approved").and_then(Value::as_bool) {
        return Some(if approved { "approved" } else { "rejected" }.to_string());
    }
    None
}

fn outcome_status_from_metadata(metadata: Option<&Value>) -> Option<String> {
    let metadata = metadata?;
    for key in [
        "source_outcome",
        "approved_outcome",
        "outcome",
        "human_review",
        "approval",
    ] {
        if let Some(status) = metadata.get(key).and_then(outcome_status_from_value) {
            return Some(status);
        }
    }
    for key in ["outcome_status", "human_disposition", "review_status"] {
        if let Some(status) = metadata.get(key).and_then(Value::as_str) {
            return Some(normalized_outcome_status(status));
        }
    }
    if metadata
        .get("artifact_validation")
        .and_then(|value| value.get("rejected_artifact_reason"))
        .is_some()
        || metadata.get("rejected_artifact_reason").is_some()
    {
        return Some("artifact_rejected".to_string());
    }
    if let Some(approved) = metadata.get("approved").and_then(Value::as_bool) {
        return Some(if approved { "approved" } else { "rejected" }.to_string());
    }
    None
}

fn outcome_status_from_request(request: &MemoryPromoteRequest) -> Option<String> {
    let outcome = request.source_outcome.as_ref()?;
    outcome
        .status
        .as_deref()
        .map(normalized_outcome_status)
        .or_else(|| {
            outcome
                .approved
                .map(|approved| if approved { "approved" } else { "rejected" }.to_string())
        })
}

fn promotion_outcome_block_reason(
    request: &MemoryPromoteRequest,
    source: &GlobalMemoryRecord,
) -> Option<String> {
    for status in [
        outcome_status_from_request(request),
        outcome_status_from_metadata(source.metadata.as_ref()),
        outcome_status_from_metadata(source.provenance.as_ref()),
    ]
    .into_iter()
    .flatten()
    {
        if matches!(
            status.as_str(),
            "denied"
                | "deny"
                | "rejected"
                | "reject"
                | "rework"
                | "reworked"
                | "regressed"
                | "superseded"
                | "failed"
                | "failure"
                | "blocked"
                | "artifact_rejected"
        ) {
            return Some(format!("source outcome not approved: {status}"));
        }
    }
    None
}

fn promotion_source_outcome_value(
    request: &MemoryPromoteRequest,
    source: &GlobalMemoryRecord,
) -> Value {
    let requested = request.source_outcome.as_ref();
    let status = outcome_status_from_request(request)
        .or_else(|| outcome_status_from_metadata(source.metadata.as_ref()))
        .or_else(|| outcome_status_from_metadata(source.provenance.as_ref()));
    json!({
        "status": status,
        "approved": requested.and_then(|outcome| outcome.approved),
        "source_run_id": requested
            .and_then(|outcome| outcome.source_run_id.clone())
            .unwrap_or_else(|| source.run_id.clone()),
        "approval_id": requested
            .and_then(|outcome| outcome.approval_id.clone())
            .or_else(|| request.review.approval_id.clone()),
        "policy_decision_id": requested.and_then(|outcome| outcome.policy_decision_id.clone()),
        "audit_id": requested.and_then(|outcome| outcome.audit_id.clone()),
    })
}

fn memory_trust_label_for_put(request: &MemoryPutRequest) -> tandem_memory::MemoryTrustLabel {
    if let Some(label) = memory_record_trust_label(request.metadata.as_ref()) {
        return label;
    }
    if request
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("enterprise_source_binding"))
        .is_some()
    {
        tandem_memory::MemoryTrustLabel::ConnectorSourced
    } else {
        tandem_memory::MemoryTrustLabel::SystemGenerated
    }
}

fn memory_record_trust_label(metadata: Option<&Value>) -> Option<tandem_memory::MemoryTrustLabel> {
    match metadata
        .and_then(|row| row.get("memory_trust"))
        .and_then(|row| row.get("label"))
        .and_then(Value::as_str)
    {
        Some("external_user_supplied") => {
            Some(tandem_memory::MemoryTrustLabel::ExternalUserSupplied)
        }
        Some("connector_sourced") => Some(tandem_memory::MemoryTrustLabel::ConnectorSourced),
        Some("verified") => Some(tandem_memory::MemoryTrustLabel::Verified),
        Some("human_approved") => Some(tandem_memory::MemoryTrustLabel::HumanApproved),
        Some("system_generated") => Some(tandem_memory::MemoryTrustLabel::SystemGenerated),
        _ => None,
    }
}

fn apply_memory_trust_metadata(
    obj: &mut serde_json::Map<String, Value>,
    label: tandem_memory::MemoryTrustLabel,
    source: &'static str,
) {
    obj.insert(
        "memory_trust".to_string(),
        json!({
            "label": label.as_str(),
            "trusted_for_promotion": label.is_trusted_for_promotion(),
            "source": source,
        }),
    );
}

fn memory_metadata_with_trust_fields(
    metadata: Option<Value>,
    label: tandem_memory::MemoryTrustLabel,
) -> Option<Value> {
    let mut obj = metadata
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    apply_memory_trust_metadata(&mut obj, label, "memory_put");
    Some(Value::Object(obj))
}

fn memory_provenance_with_trust(
    mut provenance: Value,
    label: tandem_memory::MemoryTrustLabel,
) -> Value {
    if let Some(obj) = provenance.as_object_mut() {
        obj.insert(
            "memory_trust".to_string(),
            json!({
                "label": label.as_str(),
                "trusted_for_promotion": label.is_trusted_for_promotion(),
            }),
        );
    }
    provenance
}

pub(super) fn scrub_content(input: &str) -> ScrubReport {
    let mut redactions = 0u32;
    let mut blocked = false;
    let lower = input.to_lowercase();
    let redact_markers = [
        "api_key",
        "secret=",
        "authorization: bearer",
        "x-api-key",
        "token=",
    ];
    for marker in redact_markers {
        if lower.contains(marker) {
            redactions = redactions.saturating_add(1);
        }
    }
    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "sk-ant-",
        "ghp_",
    ];
    for marker in block_markers {
        if lower.contains(marker) {
            blocked = true;
            break;
        }
    }
    if blocked {
        ScrubReport {
            status: ScrubStatus::Blocked,
            redactions,
            block_reason: Some("sensitive secret marker detected".to_string()),
        }
    } else if redactions > 0 {
        ScrubReport {
            status: ScrubStatus::Redacted,
            redactions,
            block_reason: None,
        }
    } else {
        ScrubReport {
            status: ScrubStatus::Passed,
            redactions: 0,
            block_reason: None,
        }
    }
}

pub(super) fn scrub_content_for_memory(input: &str) -> (String, ScrubReport) {
    let mut scrubbed = input.to_string();
    let mut redactions = 0u32;
    let mut blocked = false;
    let redact_patterns = [
        r"(?i)authorization:\s*bearer\s+[a-z0-9\.\-_]+",
        r"(?i)(api[_-]?key|token|secret)\s*[:=]\s*[a-z0-9\-_]{8,}",
        r"(?i)x-api-key\s*:\s*[a-z0-9\-_]{8,}",
        r"(?i)sk-[a-z0-9]{12,}",
        r"(?i)ghp_[a-z0-9]{12,}",
    ];
    for pattern in redact_patterns {
        if let Ok(re) = Regex::new(pattern) {
            let matches = re.find_iter(&scrubbed).count() as u32;
            if matches > 0 {
                redactions = redactions.saturating_add(matches);
                scrubbed = re.replace_all(&scrubbed, "[REDACTED]").to_string();
            }
        }
    }
    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "-----begin rsa private key-----",
    ];
    let lowered = input.to_lowercase();
    for marker in block_markers {
        if lowered.contains(marker) {
            blocked = true;
            break;
        }
    }
    if blocked {
        (
            String::new(),
            ScrubReport {
                status: ScrubStatus::Blocked,
                redactions,
                block_reason: Some("sensitive secret marker detected".to_string()),
            },
        )
    } else if redactions > 0 {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Redacted,
                redactions,
                block_reason: None,
            },
        )
    } else {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Passed,
                redactions: 0,
                block_reason: None,
            },
        )
    }
}

pub(super) fn hash_text(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone)]
pub(super) struct RunMemoryContext {
    run_id: String,
    user_id: String,
    started_at_ms: u64,
    host_tag: Option<String>,
    tenant_context: TenantContext,
    /// Active department to stamp on ingested run memory (`owner_org_unit_id`),
    /// resolved from the run's verified context (TAN-646). `None` = unattributable
    /// / local mode (tenant-wide).
    owner_org_unit_id: Option<String>,
}

#[cfg(test)]
pub(super) async fn open_global_memory_db() -> Option<MemoryDatabase> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    if let Some(parent) = paths.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    MemoryDatabase::new(&paths.memory_db_path).await.ok()
}

#[cfg(test)]
pub(super) async fn open_global_memory_db_for_state(state: &AppState) -> Option<MemoryDatabase> {
    if let Some(parent) = state.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    MemoryDatabase::new(&state.memory_db_path).await.ok()
}

/// Assemble the bundled SQLite backend without exposing it to HTTP business
/// workflows. Other deployments can replace this construction boundary with a
/// different `MemoryStore` implementation.
pub(super) async fn open_global_memory_store_for_state(
    state: &AppState,
) -> Option<std::sync::Arc<dyn tandem_memory::MemoryStore>> {
    state.memory_store().await.ok()
}

pub(super) async fn with_verified_memory_decrypt_principal<F, T>(
    verified_tenant_context: Option<&VerifiedTenantContext>,
    future: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    let principal = verified_tenant_context.and_then(|verified| {
        crate::memory::decrypt_principal::memory_decrypt_principal_from_verified_context(
            verified,
            crate::now_ms(),
        )
    });
    match principal {
        Some(principal) => {
            tandem_memory::decrypt_context::with_decrypt_principal(principal, future).await
        }
        None => future.await,
    }
}

pub(super) async fn open_memory_manager() -> Option<tandem_memory::MemoryManager> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    if let Some(parent) = paths.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tandem_memory::MemoryManager::new_runtime(&paths.memory_db_path)
        .await
        .ok()
}

pub(super) async fn open_memory_manager_for_state(
    state: &AppState,
) -> Option<tandem_memory::MemoryManager> {
    let store = state.memory_store().await.ok()?;
    #[cfg(not(test))]
    let embedding_service = tandem_memory::embeddings::EmbeddingService::new();
    #[cfg(test)]
    let embedding_service = tandem_memory::embeddings::EmbeddingService::deterministic_for_tests(
        tandem_memory::types::DEFAULT_EMBEDDING_DIMENSION,
    );
    tandem_memory::MemoryManager::new_with_store(store, embedding_service).ok()
}

async fn open_memory_manager_for_request(
    state: &AppState, tenant: &TenantContext, verified: Option<&VerifiedTenantContext>,
) -> Option<tandem_memory::MemoryManager> {
    open_memory_manager_for_state(state).await.map(|manager| {
        manager.with_derived_memory_access_resolver(crate::memory::derived_lineage::lineage_resolver(
            state.clone(), tenant.clone(), verified.cloned(),
        ))
    })
}

pub(super) fn event_run_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("runID")
        .or_else(|| event.properties.get("run_id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub(super) fn event_session_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub(super) fn summarize_value(value: &Value, limit: usize) -> String {
    let text = if value.is_string() {
        value.as_str().unwrap_or_default().to_string()
    } else {
        value.to_string()
    };
    truncate_text(&text, limit)
}

/// How much of each tool invocation the run-finish ingestor persists into
/// `memory_records` (TAN-637). Tool args and output are the noisiest, least
/// re-surfaced content in the store, so the default keeps only the tool name
/// and outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolEventCaptureMode {
    /// Persist no tool invocation records.
    Off,
    /// One `tool_event` record per invocation: tool name + outcome, without
    /// verbatim args or output (a short error snippet is kept — the outcome
    /// is the signal these records exist for).
    Summary,
    /// Prior behavior: `tool_input`/`tool_output` records with truncated
    /// verbatim args and output.
    Full,
}

pub(super) fn tool_event_capture_mode() -> ToolEventCaptureMode {
    match std::env::var("TANDEM_MEMORY_TOOL_EVENT_CAPTURE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => ToolEventCaptureMode::Off,
        "full" => ToolEventCaptureMode::Full,
        _ => ToolEventCaptureMode::Summary,
    }
}

/// Hard ceiling on persisted memory record content, applied after scrubbing
/// (so redaction still sees the full text). Keeps a single pasted blob or
/// giant tool payload from bloating memory.sqlite; FTS relevance for prompt
/// context does not need more than this.
pub(super) const MAX_MEMORY_RECORD_CONTENT_CHARS: usize = 8_000;

pub(super) async fn persist_global_memory_record(
    state: &AppState,
    store: &dyn tandem_memory::MemoryStore,
    record: GlobalMemoryRecord,
) -> Option<tandem_memory::types::GlobalMemoryWriteResult> {
    persist_global_memory_record_with_commit_authority(state, store, record, None).await
}

async fn persist_global_memory_record_with_commit_authority(
    state: &AppState,
    store: &dyn tandem_memory::MemoryStore,
    mut record: GlobalMemoryRecord,
    authority: Option<tandem_memory::MemoryCommitAuthority>,
) -> Option<tandem_memory::types::GlobalMemoryWriteResult> {
    let tenant_context = record_tenant_context(&record);
    publish_tenant_event(
        state,
        &tenant_context,
        "memory.write.attempted",
        json!({
            "runID": record.run_id,
            "sourceType": record.source_type,
            "sessionID": record.session_id,
            "messageID": record.message_id,
        }),
    );
    let (scrubbed, scrub) = scrub_content_for_memory(&record.content);
    if scrub.status == ScrubStatus::Blocked || scrubbed.trim().is_empty() {
        publish_tenant_event(
            state,
            &tenant_context,
            "memory.write.skipped",
            json!({
                "runID": record.run_id,
                "sourceType": record.source_type,
                "reason": scrub.block_reason.unwrap_or_else(|| "scrub_blocked".to_string()),
                "sessionID": record.session_id,
                "messageID": record.message_id,
            }),
        );
        return None;
    }
    record.content = truncate_text(&scrubbed, MAX_MEMORY_RECORD_CONTENT_CHARS);
    record.redaction_count = scrub.redactions;
    record.redaction_status = match scrub.status {
        ScrubStatus::Passed => "passed".to_string(),
        ScrubStatus::Redacted => "redacted".to_string(),
        ScrubStatus::Blocked => "blocked".to_string(),
    };
    record.content_hash = hash_text(&record.content);
    let owner_subject = tandem_memory::types::owner_subject_from_metadata(record.metadata.as_ref())
        .or_else(|| {
            matches!(
                record.source_type.as_str(),
                "user_message"
                    | "assistant_final"
                    | "tool_event"
                    | "tool_input"
                    | "tool_output"
                    | "question_prompt"
                    | "plan_todos"
            )
            .then(|| record.user_id.clone())
        });
    record.metadata =
        memory_metadata_with_owner_subject(record.metadata.take(), owner_subject.as_deref());
    let scope = tandem_memory::MemoryWriteScope {
        tenant: MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        },
        org_unit: tandem_memory::types::owner_org_unit_id_from_metadata(record.metadata.as_ref()),
        subject: tandem_memory::types::owner_subject_from_metadata(record.metadata.as_ref()),
    };
    let request = tandem_memory::MemoryStoreWriteRequest::GlobalRecord {
        scope,
        record: record.clone(),
    };
    let result = match authority {
        Some(authority) => store.write_with_commit_authority(request, authority).await,
        None => store.write(request).await,
    };
    match result {
        Ok(tandem_memory::MemoryStoreWriteResult::GlobalRecord(write)) => {
            let event_name = if write.deduped {
                "memory.write.skipped"
            } else {
                "memory.write.succeeded"
            };
            publish_tenant_event(
                state,
                &tenant_context,
                event_name,
                json!({
                    "runID": record.run_id,
                    "memoryID": write.id,
                    "sourceType": record.source_type,
                    "deduped": write.deduped,
                    "redactionStatus": record.redaction_status,
                    "redactionCount": record.redaction_count,
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            );
            Some(write)
        }
        Ok(_) => {
            tracing::warn!("unexpected global memory store write result");
            publish_tenant_event(
                state,
                &tenant_context,
                "memory.write.skipped",
                json!({
                    "runID": record.run_id,
                    "sourceType": record.source_type,
                    "reason": "unexpected_memory_store_write_result",
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            );
            None
        }
        Err(err) => {
            tracing::warn!("global memory store write failed: {err}");
            publish_tenant_event(
                state,
                &tenant_context,
                "memory.write.skipped",
                json!({
                    "runID": record.run_id,
                    "sourceType": record.source_type,
                    "reason": format!("store_error:{err}"),
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            );
            None
        }
    }
}

pub(super) async fn ingest_run_messages(
    state: &AppState,
    store: &dyn tandem_memory::MemoryStore,
    session_id: &str,
    ctx: &RunMemoryContext,
) {
    let Some(session) = state.storage.get_session(session_id).await else {
        return;
    };
    let tool_capture = tool_event_capture_mode();
    for message in session.messages {
        let created_ms = message.created_at.timestamp_millis() as u64;
        if created_ms + 1_000 < ctx.started_at_ms {
            continue;
        }
        for part in message.parts {
            match (message.role.clone(), part) {
                (MessageRole::User, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    let _ = persist_global_memory_record(
                        state,
                        store,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "user_message".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: memory_metadata_with_owner_org_unit(
                                Some(json!({"role": "user"})),
                                ctx.owner_org_unit_id.as_deref(),
                            ),
                            provenance: Some(json!({"origin_event_type": "session.run.finished", "origin_message_id": message.id, "origin_session_id": session_id, "tenant_context": ctx.tenant_context})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (MessageRole::Assistant, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    let _ = persist_global_memory_record(
                        state,
                        store,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "assistant_final".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: memory_metadata_with_owner_org_unit(
                                Some(json!({"role": "assistant"})),
                                ctx.owner_org_unit_id.as_deref(),
                            ),
                            provenance: Some(json!({"origin_event_type": "session.run.finished", "origin_message_id": message.id, "origin_session_id": session_id, "tenant_context": ctx.tenant_context})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (
                    MessageRole::Assistant | MessageRole::Tool,
                    MessagePart::ToolInvocation {
                        tool,
                        args,
                        result,
                        error,
                    },
                ) => {
                    match tool_capture {
                        ToolEventCaptureMode::Off => continue,
                        ToolEventCaptureMode::Summary => {
                            let now = crate::now_ms();
                            let outcome = match error.as_deref() {
                                Some(err) => {
                                    format!("error {}", truncate_text(err.trim(), 200))
                                }
                                None => "ok".to_string(),
                            };
                            let _ = persist_global_memory_record(
                                state,
                                store,
                                GlobalMemoryRecord {
                                    id: Uuid::new_v4().to_string(),
                                    user_id: ctx.user_id.clone(),
                                    source_type: "tool_event".to_string(),
                                    content: format!("tool={} outcome={}", tool, outcome),
                                    content_hash: String::new(),
                                    run_id: ctx.run_id.clone(),
                                    session_id: Some(session_id.to_string()),
                                    message_id: Some(message.id.clone()),
                                    tool_name: Some(tool),
                                    project_tag: session.project_id.clone(),
                                    channel_tag: None,
                                    host_tag: ctx.host_tag.clone(),
                                    metadata: memory_metadata_with_owner_org_unit(
                                        None,
                                        ctx.owner_org_unit_id.as_deref(),
                                    ),
                                    provenance: Some(json!({
                                        "origin_event_type": "session.run.finished",
                                        "tenant_context": ctx.tenant_context,
                                    })),
                                    redaction_status: "passed".to_string(),
                                    redaction_count: 0,
                                    visibility: "private".to_string(),
                                    demoted: false,
                                    score_boost: 0.0,
                                    created_at_ms: now,
                                    updated_at_ms: now,
                                    expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                                },
                            )
                            .await;
                            continue;
                        }
                        ToolEventCaptureMode::Full => {}
                    }
                    let now = crate::now_ms();
                    let tool_input = summarize_value(&args, 1200);
                    let _ = persist_global_memory_record(
                        state,
                        store,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "tool_input".to_string(),
                            content: format!("tool={} args={}", tool, tool_input),
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: Some(tool.clone()),
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: memory_metadata_with_owner_org_unit(
                                None,
                                ctx.owner_org_unit_id.as_deref(),
                            ),
                            provenance: Some(json!({
                                "origin_event_type": "session.run.finished",
                                "tenant_context": ctx.tenant_context,
                            })),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                        },
                    )
                    .await;
                    let tool_output = result
                        .as_ref()
                        .map(|v| summarize_value(v, 1500))
                        .or(error)
                        .unwrap_or_default();
                    if !tool_output.trim().is_empty() {
                        let now = crate::now_ms();
                        let _ = persist_global_memory_record(
                            state,
                            store,
                            GlobalMemoryRecord {
                                id: Uuid::new_v4().to_string(),
                                user_id: ctx.user_id.clone(),
                                source_type: "tool_output".to_string(),
                                content: format!("tool={} output={}", tool, tool_output),
                                content_hash: String::new(),
                                run_id: ctx.run_id.clone(),
                                session_id: Some(session_id.to_string()),
                                message_id: Some(message.id.clone()),
                                tool_name: Some(tool),
                                project_tag: session.project_id.clone(),
                                channel_tag: None,
                                host_tag: ctx.host_tag.clone(),
                                metadata: memory_metadata_with_owner_org_unit(
                                    None,
                                    ctx.owner_org_unit_id.as_deref(),
                                ),
                                provenance: Some(json!({
                                    "origin_event_type": "session.run.finished",
                                    "tenant_context": ctx.tenant_context,
                                })),
                                redaction_status: "passed".to_string(),
                                redaction_count: 0,
                                visibility: "private".to_string(),
                                demoted: false,
                                score_boost: 0.0,
                                created_at_ms: now,
                                updated_at_ms: now,
                                expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                            },
                        )
                        .await;
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) async fn ingest_event_memory_records(
    state: &AppState,
    store: &dyn tandem_memory::MemoryStore,
    event: &EngineEvent,
    ctx_by_session: &HashMap<String, RunMemoryContext>,
) {
    let session_id = event_session_id(event);
    // Fail closed on attribution: without the session's run context there is no
    // resolved owner subject or tenant scope, and fabricating them (previously
    // user_id="default" + TenantContext::default()) files "private" memory
    // under a catch-all identity that unrelated readers can retrieve (TAN-633).
    // An event we cannot attribute is not worth storing as memory.
    let Some(session_ctx) = session_id
        .as_ref()
        .and_then(|sid| ctx_by_session.get(sid))
        .cloned()
    else {
        tracing::debug!(
            event_type = %event.event_type,
            session_id = session_id.as_deref().unwrap_or(""),
            "skipping event memory ingestion without an attributable run context"
        );
        return;
    };
    let run_id = event_run_id(event).unwrap_or_else(|| session_ctx.run_id.clone());
    let user_id = session_ctx.user_id.clone();
    let host_tag = session_ctx.host_tag.clone();
    let tenant_context =
        event_tenant_context(event).unwrap_or_else(|| session_ctx.tenant_context.clone());
    let (source_type, content, ttl_ms): (&str, String, Option<u64>) =
        match event.event_type.as_str() {
            "permission.asked" => (
                "approval_request",
                format!(
                    "permission requested tool={} query={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    event
                        .properties
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "permission.replied" => (
                "approval_decision",
                format!(
                    "permission reply requestID={} reply={}",
                    event
                        .properties
                        .get("requestID")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("reply")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "mcp.auth.required" | "mcp.auth.pending" => (
                "auth_challenge",
                format!(
                    "mcp auth tool={} server={} status={} message={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("server")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event.event_type,
                    event
                        .properties
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(7 * 24 * 60 * 60 * 1000),
            ),
            "todo.updated" => (
                "plan_todos",
                format!(
                    "todo updated: {}",
                    summarize_value(event.properties.get("todos").unwrap_or(&Value::Null), 1200)
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            "question.asked" => (
                "question_prompt",
                format!(
                    "question asked: {}",
                    summarize_value(
                        event.properties.get("questions").unwrap_or(&Value::Null),
                        1200
                    )
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            _ => return,
        };
    let now = crate::now_ms();
    let _ = persist_global_memory_record(
        state,
        store,
        GlobalMemoryRecord {
            id: Uuid::new_v4().to_string(),
            user_id,
            source_type: source_type.to_string(),
            content,
            content_hash: String::new(),
            run_id,
            session_id,
            message_id: event
                .properties
                .get("messageID")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            tool_name: event
                .properties
                .get("tool")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            project_tag: None,
            channel_tag: event
                .properties
                .get("channel")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            host_tag,
            metadata: memory_metadata_with_owner_org_unit(
                None,
                session_ctx.owner_org_unit_id.as_deref(),
            ),
            provenance: Some(json!({
                "origin_event_type": event.event_type,
                "tenant_context": tenant_context,
            })),
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: ttl_ms.map(|ttl| now + ttl),
        },
    )
    .await;
}

/// Resolve the owner subject for records ingested from a run, using the same
/// subject resolution the mutation guard and `memory_list` apply. This keeps an
/// authenticated actor's ingested run memory owned by their resolved subject
/// rather than the raw run client id, so they can list, demote, and delete their
/// own memory in governed mode (the ownership guard in `memory_delete` /
/// `memory_demote` compares against exactly this value). In local-unrestricted
/// mode the run client id remains the subject, matching the prompt-injection
/// reader.
pub(super) fn ingested_memory_owner_subject(
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    run_client_id: &str,
) -> String {
    if crate::memory::subject::local_memory_subjects_are_unrestricted(tenant_context, verified) {
        return crate::memory::subject::normalize_memory_subject(Some(run_client_id));
    }
    crate::memory::subject::request_memory_subject(tenant_context, verified, None)
        .map(|resolution| resolution.subject)
        .unwrap_or_else(|_| crate::memory::subject::normalize_memory_subject(Some(run_client_id)))
}

pub(super) async fn run_global_memory_ingestor(state: AppState) {
    if !state.wait_for_worker_ready_or_failed(120, 250).await {
        tracing::warn!("global memory ingestor: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    let Some(store) = open_global_memory_store_for_state(&state).await else {
        tracing::warn!("global memory ingestor disabled: could not open memory database");
        return;
    };
    let mut by_session: HashMap<String, RunMemoryContext> = HashMap::new();
    loop {
        match rx.recv().await {
            Ok(event) => match event.event_type.as_str() {
                "session.run.started" => {
                    let session_id = event_session_id(&event);
                    let run_id = event_run_id(&event);
                    if let (Some(session_id), Some(run_id)) = (session_id, run_id) {
                        let started_at_ms = event
                            .properties
                            .get("startedAtMs")
                            .and_then(|v| v.as_u64())
                            .unwrap_or_else(crate::now_ms);
                        let run_client_id = event
                            .properties
                            .get("clientID")
                            .and_then(|v| v.as_str())
                            .filter(|v| !v.trim().is_empty())
                            .unwrap_or("default")
                            .to_string();
                        let host_tag = event
                            .properties
                            .get("environment")
                            .and_then(|v| v.get("os"))
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                        let tenant_context = event_tenant_context(&event).unwrap_or_default();
                        // Own ingested run memory under the same subject the read
                        // and mutation paths resolve, so governed actors can manage
                        // their own memory (never their raw run client id).
                        let verified = state
                            .storage
                            .get_session(&session_id)
                            .await
                            .and_then(|session| session.verified_tenant_context);
                        let user_id = ingested_memory_owner_subject(
                            &tenant_context,
                            verified.as_ref(),
                            &run_client_id,
                        );
                        // Active department stamped on every record ingested for
                        // this run (TAN-646), from the same verified context that
                        // resolves the subject.
                        let owner_org_unit_id =
                            crate::memory::subject::active_org_unit(verified.as_ref());
                        by_session.insert(
                            session_id,
                            RunMemoryContext {
                                run_id,
                                user_id,
                                started_at_ms,
                                host_tag,
                                tenant_context,
                                owner_org_unit_id,
                            },
                        );
                    }
                }
                "session.run.finished" => {
                    if let Some(session_id) = event_session_id(&event) {
                        if let Some(ctx) = by_session.remove(&session_id) {
                            ingest_run_messages(&state, store.as_ref(), &session_id, &ctx).await;
                        }
                    }
                }
                "permission.asked" | "permission.replied" | "mcp.auth.required"
                | "mcp.auth.pending" | "todo.updated" | "question.asked" => {
                    ingest_event_memory_records(&state, store.as_ref(), &event, &by_session).await;
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

include!("memory_import.rs");
