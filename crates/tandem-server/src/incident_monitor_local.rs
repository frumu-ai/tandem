// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Value};
use tandem_types::EngineEvent;

use crate::{
    now_ms, sha256_hex, truncate_text, AppState, IncidentMonitorConfig,
    IncidentMonitorDestinationKind, IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord,
};

pub use crate::incident_monitor_github::{PublishMode, PublishOutcome};

const DEFAULT_TELEMETRY_PATH: &str = "incident-monitor/telemetry";
const DEFAULT_MEMORY_CATEGORY: &str = "failure_pattern";
const MEMORY_CATEGORY_FAILURE_PATTERN: &str = "failure_pattern";
const MEMORY_CATEGORY_RECURRENCE: &str = "recurrence";
const MEMORY_CATEGORY_POLICY_GAP: &str = "policy_gap";
const MEMORY_CATEGORY_SAFETY_RISK: &str = "safety_risk";

#[derive(Debug, Clone)]
pub struct LocalDestinationContext {
    pub destination_id: String,
    pub route_id: Option<String>,
    pub route_match_reason: Option<String>,
    pub kind: IncidentMonitorDestinationKind,
    pub telemetry_path: Option<String>,
    pub memory_category: Option<String>,
    pub config: Option<Value>,
}

impl LocalDestinationContext {
    fn route_match_reason(&self) -> Option<String> {
        self.route_match_reason
            .clone()
            .or_else(|| Some("destination_router".to_string()))
    }

    fn kind_label(&self) -> anyhow::Result<&'static str> {
        match self.kind {
            IncidentMonitorDestinationKind::Telemetry => Ok("telemetry"),
            IncidentMonitorDestinationKind::InternalMemory => Ok("internal_memory"),
            _ => anyhow::bail!(
                "Destination `{}` uses {:?}, which is not a local Incident Monitor destination",
                self.destination_id,
                self.kind
            ),
        }
    }

    fn operation(&self) -> anyhow::Result<&'static str> {
        match self.kind {
            IncidentMonitorDestinationKind::Telemetry => Ok("record_telemetry"),
            IncidentMonitorDestinationKind::InternalMemory => Ok("store_memory_summary"),
            _ => self.kind_label().map(|_| "record_local_destination"),
        }
    }

    fn target_ref(&self) -> anyhow::Result<String> {
        match self.kind {
            IncidentMonitorDestinationKind::Telemetry => {
                Ok(format!("telemetry:{}", self.telemetry_path()))
            }
            IncidentMonitorDestinationKind::InternalMemory => {
                Ok(format!("memory:{}", self.memory_category()))
            }
            _ => anyhow::bail!(
                "Destination `{}` uses {:?}, which is not a local Incident Monitor destination",
                self.destination_id,
                self.kind
            ),
        }
    }

    fn telemetry_path(&self) -> String {
        self.telemetry_path
            .as_deref()
            .and_then(normalize_config_string)
            .or_else(|| config_string(&self.config, &["telemetry_path", "path"]))
            .unwrap_or_else(|| DEFAULT_TELEMETRY_PATH.to_string())
    }

    fn memory_category(&self) -> String {
        let raw = self
            .configured_memory_category()
            .unwrap_or_else(|| DEFAULT_MEMORY_CATEGORY.to_string());
        normalize_memory_category(&raw).unwrap_or_else(|| DEFAULT_MEMORY_CATEGORY.to_string())
    }

    fn configured_memory_category(&self) -> Option<String> {
        self.memory_category
            .as_deref()
            .and_then(normalize_config_string)
            .or_else(|| config_string(&self.config, &["memory_category", "category"]))
    }
}

pub fn is_supported_memory_category(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    normalize_memory_category(value).as_deref() == Some(normalized.as_str())
}

pub async fn publish_draft(
    state: &AppState,
    draft_id: &str,
    incident_id: Option<&str>,
    mode: PublishMode,
    destination: LocalDestinationContext,
) -> anyhow::Result<PublishOutcome> {
    crate::incident_monitor::require_current_policy(state)?;
    let status = state.incident_monitor_status_snapshot().await;
    let config = status.config.clone();
    validate_local_publish_config(&config, mode, &destination)?;

    let mut draft = state
        .get_incident_monitor_draft(draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor draft not found"))?;
    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Incident Monitor draft has been denied");
    }
    if mode == PublishMode::Auto
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        return Ok(PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }

    let incident = match incident_id {
        Some(id) => state.get_incident_monitor_incident(id).await,
        None => None,
    };
    let evidence_digest = compute_evidence_digest(&draft);
    draft.evidence_digest = Some(evidence_digest.clone());

    let target_ref = destination.target_ref()?;
    if mode == PublishMode::RecheckOnly {
        if let Some(existing) = successful_post_for_draft(
            state,
            &draft.draft_id,
            &destination.destination_id,
            &target_ref,
            Some(&evidence_digest),
        )
        .await
        {
            apply_existing_local_post_to_draft(&mut draft, &existing);
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "local_record_found".to_string(),
                draft,
                post: None,
            });
        }
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "no_match".to_string(),
            draft,
            post: None,
        });
    }

    publish_local_record(
        state,
        draft,
        incident.as_ref(),
        &destination,
        &target_ref,
        &evidence_digest,
    )
    .await
}

fn validate_local_publish_config(
    config: &IncidentMonitorConfig,
    mode: PublishMode,
    destination: &LocalDestinationContext,
) -> anyhow::Result<()> {
    if !config.enabled {
        anyhow::bail!("Incident Monitor is disabled");
    }
    if config.paused && matches!(mode, PublishMode::Auto | PublishMode::Recovery) {
        anyhow::bail!("Incident Monitor is paused");
    }
    destination.kind_label()?;
    if destination.kind == IncidentMonitorDestinationKind::InternalMemory {
        if let Some(category) = destination.configured_memory_category() {
            if !is_supported_memory_category(&category) {
                anyhow::bail!(
                    "Internal memory destination category must be one of failure_pattern, recurrence, policy_gap, or safety_risk"
                );
            }
        }
    }
    Ok(())
}

async fn publish_local_record(
    state: &AppState,
    mut draft: IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    destination: &LocalDestinationContext,
    target_ref: &str,
    evidence_digest: &str,
) -> anyhow::Result<PublishOutcome> {
    let operation = destination.operation()?;
    let idempotency_key = build_idempotency_key(
        &destination.destination_id,
        destination.kind_label()?,
        target_ref,
        &draft.fingerprint,
        operation,
        evidence_digest,
    );
    if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
        apply_existing_local_post_to_draft(&mut draft, &existing);
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }
    if let Some(existing) = successful_post_for_draft(
        state,
        &draft.draft_id,
        &destination.destination_id,
        target_ref,
        Some(evidence_digest),
    )
    .await
    {
        apply_existing_local_post_to_draft(&mut draft, &existing);
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    let now = now_ms();
    let claim = IncidentMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        tenant_id: draft.tenant_id.clone(),
        workspace_id: draft.workspace_id.clone(),
        incident_id: incident.map(|row| row.incident_id.clone()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: operation.to_string(),
        status: "pending".to_string(),
        issue_number: None,
        issue_url: None,
        comment_id: None,
        comment_url: None,
        destination_id: Some(destination.destination_id.clone()),
        destination_kind: Some(destination.kind.clone()),
        route_id: destination.route_id.clone(),
        route_match_reason: destination.route_match_reason(),
        external_id: None,
        external_url: None,
        external_title: None,
        target_ref: Some(target_ref.to_string()),
        receipt: Some(json!({
            "provider": receipt_provider(destination),
            "destination_id": destination.destination_id,
            "operation": operation,
            "status": "pending",
            "target_ref": target_ref,
        })),
        evidence_digest: Some(evidence_digest.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: safe_evidence_refs(&draft.evidence_refs),
        quality_gate: None,
        idempotency_key: idempotency_key.clone(),
        response_excerpt: None,
        error: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let (claimed, existing_claim) = state
        .try_claim_incident_monitor_post_idempotency(claim)
        .await?;
    if !claimed {
        if existing_claim.status == "posted" {
            apply_existing_local_post_to_draft(&mut draft, &existing_claim);
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "skip_duplicate".to_string(),
                draft,
                post: Some(existing_claim),
            });
        }
        let posting_status = posting_status(destination);
        draft.github_status = Some(posting_status.to_string());
        draft.last_post_error = Some(format!(
            "another Incident Monitor publisher already claimed this {operation} idempotency key"
        ));
        return Ok(PublishOutcome {
            action: "publish_in_progress".to_string(),
            draft,
            post: Some(existing_claim),
        });
    }

    let record_id = deterministic_record_id(destination, target_ref, &draft, evidence_digest)?;
    let prepared = async {
        crate::incident_monitor::require_current_policy(state)?;
        let receipt = build_receipt(
            state,
            &draft,
            incident,
            destination,
            target_ref,
            &record_id,
            &idempotency_key,
            evidence_digest,
        )
        .await?;
        crate::incident_monitor::require_current_policy(state)?;
        // TAN-556: actually persist the telemetry record to the configured sink
        // before reporting the publish as posted, so `record_telemetry` isn't a
        // receipt-only no-op. A write failure surfaces as a publish failure rather
        // than a false success.
        if destination.kind == IncidentMonitorDestinationKind::Telemetry {
            let sink = resolve_telemetry_sink_path(state, &destination.telemetry_path());
            return persist_incident_monitor_telemetry(state, &sink, &receipt).await;
        }
        Ok::<_, anyhow::Error>(receipt)
    }
    .await;
    let receipt = match prepared {
        Ok(receipt) => receipt,
        Err(error) => {
            if error.is::<crate::incident_monitor::HostedPolicyUnavailable>() {
                pause_local_claim(state, &existing_claim).await?;
            }
            return Err(error);
        }
    };
    let response_excerpt = receipt
        .get("summary")
        .and_then(Value::as_str)
        .map(|value| truncate_text(value, 400))
        .or_else(|| {
            Some(truncate_text(
                &format!("{} {}", operation, draft.fingerprint),
                400,
            ))
        });
    let external_title = draft
        .title
        .as_deref()
        .map(safe_summary_text)
        .or_else(|| Some(draft.fingerprint.clone()));

    let post = IncidentMonitorPostRecord {
        status: "posted".to_string(),
        external_id: Some(record_id),
        external_title,
        receipt: Some(receipt),
        response_excerpt,
        error: None,
        updated_at_ms: now_ms(),
        ..existing_claim
    };
    let post = if destination.kind == IncidentMonitorDestinationKind::InternalMemory {
        let mut guard = state.incident_monitor_posts.write().await;
        if let Err(error) = crate::incident_monitor::require_current_policy(state) {
            drop(guard);
            pause_local_claim(state, &post).await?;
            return Err(error);
        }
        guard.insert(post.post_id.clone(), post.clone());
        drop(guard);
        state.persist_incident_monitor_posts().await?;
        post
    } else {
        // The telemetry write already happened; its receipt must remain durable
        // even if policy expires after delivery.
        state.put_incident_monitor_post(post).await?
    };
    apply_existing_local_post_to_draft(&mut draft, &post);
    let draft = state.put_incident_monitor_draft(draft).await?;
    state
        .update_incident_monitor_runtime_status(|runtime| {
            runtime.last_post_result = Some(format!(
                "{} {}",
                operation,
                post.external_id.as_deref().unwrap_or("unknown")
            ));
        })
        .await;
    publish_local_event(state, destination, &draft, &post, target_ref);
    Ok(PublishOutcome {
        action: operation.to_string(),
        draft,
        post: Some(post),
    })
}

async fn successful_post_by_idempotency(
    state: &AppState,
    idempotency_key: &str,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| post.idempotency_key == idempotency_key && post.status == "posted")
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().next()
}

async fn successful_post_for_draft(
    state: &AppState,
    draft_id: &str,
    destination_id: &str,
    target_ref: &str,
    evidence_digest: Option<&str>,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| post.draft_id == draft_id && post.status == "posted")
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().find(|row| {
        row.destination_id.as_deref() == Some(destination_id)
            && row.target_ref.as_deref() == Some(target_ref)
            && match evidence_digest {
                Some(expected) => row.evidence_digest.as_deref() == Some(expected),
                None => true,
            }
    })
}

fn apply_existing_local_post_to_draft(
    draft: &mut IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    let status = match post.destination_kind {
        Some(IncidentMonitorDestinationKind::InternalMemory) => "memory_summary_stored",
        _ => "telemetry_recorded",
    };
    draft.status = status.to_string();
    draft.github_status = Some(status.to_string());
    draft.github_issue_url = post.external_url.clone();
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.last_post_error = None;
}

/// Resolve the telemetry sink file. Absolute operator paths are honored as-is;
/// a relative path is anchored under the incident-monitor data directory rather
/// than the process working directory, keeping writes inside the state tree.
/// A redundant leading `incident-monitor/` (e.g. the default
/// `incident-monitor/telemetry`) is stripped first so the base isn't nested
/// twice into `<state>/incident-monitor/incident-monitor/telemetry`.
fn resolve_telemetry_sink_path(state: &AppState, configured: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(configured);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let Some(base) = state.incident_monitor_log_evidence_dir.parent() else {
        return path.to_path_buf();
    };
    // `base` is the incident-monitor data dir, so a path already prefixed with
    // `incident-monitor/` (the default `incident-monitor/telemetry`) would nest
    // it twice; drop that leading component.
    let relative = path.strip_prefix("incident-monitor").unwrap_or(path);
    base.join(relative)
}

mod telemetry_sink;
use telemetry_sink::persist_incident_monitor_telemetry;
async fn pause_local_claim(
    state: &AppState,
    claim: &IncidentMonitorPostRecord,
) -> anyhow::Result<()> {
    let changed = {
        let mut guard = state.incident_monitor_posts.write().await;
        if let Some(row) = guard
            .get_mut(&claim.post_id)
            .filter(|row| row.status == "pending" && row.idempotency_key == claim.idempotency_key)
        {
            row.status = "policy_paused".into();
            row.updated_at_ms = now_ms();
            true
        } else {
            false
        }
    };
    if changed {
        state.persist_incident_monitor_posts().await?;
    }
    Ok(())
}

async fn build_receipt(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    destination: &LocalDestinationContext,
    target_ref: &str,
    record_id: &str,
    idempotency_key: &str,
    evidence_digest: &str,
) -> anyhow::Result<Value> {
    match destination.kind {
        IncidentMonitorDestinationKind::Telemetry => Ok(json!({
            "provider": "incident_monitor_telemetry",
            "destination_id": destination.destination_id,
            "operation": "record_telemetry",
            "status": "posted",
            "record_id": record_id,
            "telemetry_path": destination.telemetry_path(),
            "target_ref": target_ref,
            "repo": draft.repo,
            "fingerprint": draft.fingerprint,
            "title": draft.title.as_deref().map(safe_summary_text),
            "incident_id": incident.map(|row| row.incident_id.clone()),
            "evidence_digest": evidence_digest,
            "confidence": draft.confidence,
            "risk_level": draft.risk_level,
            "risk_category": draft.risk_category,
            "actor": draft.actor,
            "model": draft.model,
            "tool_name": draft.tool_name,
            "action": draft.action,
            "policy": draft.policy,
            "approval_state": draft.approval_state,
            "blast_radius": draft.blast_radius,
            "external_correlation_ids": draft.external_correlation_ids,
            "expected_destination": draft.expected_destination,
            "route_id": destination.route_id,
            "route_match_reason": destination.route_match_reason(),
            "project_id": draft.project_id,
            "log_source_id": draft.log_source_id,
            "tenant_id": draft.tenant_id,
            "workspace_id": draft.workspace_id,
            "event_schema_version": draft.event_schema_version,
            "redaction_profile": draft.redaction_profile.as_deref().unwrap_or("incident_monitor_local_default"),
            "retention_profile": draft.retention_profile.as_deref().unwrap_or("incident_monitor_destination_receipt"),
            "idempotency_key": idempotency_key,
        })),
        IncidentMonitorDestinationKind::InternalMemory => {
            let category = destination.memory_category();
            let recurrence_count =
                memory_recurrence_count(state, draft, &destination.destination_id, target_ref)
                    .await;
            let summary = build_memory_summary(draft, incident, &category, recurrence_count);
            Ok(json!({
                "provider": "incident_monitor_internal_memory",
                "destination_id": destination.destination_id,
                "operation": "store_memory_summary",
                "status": "posted",
                "stored": true,
                // TAN-556: the durable record is the incident-monitor post/receipt
                // store — name it explicitly so `stored` isn't read as a claim
                // about a separate memory subsystem that isn't written.
                "storage_backend": "incident_monitor_posts",
                "record_id": record_id,
                "memory_ref": record_id,
                "category": category,
                "target_ref": target_ref,
                "summary": summary,
                "repo": draft.repo,
                "fingerprint": draft.fingerprint,
                "incident_id": incident.map(|row| row.incident_id.clone()),
                "recurrence_count": recurrence_count,
                "evidence_digest": evidence_digest,
                "confidence": draft.confidence,
                "risk_level": draft.risk_level,
                "risk_category": draft.risk_category,
                "actor": draft.actor,
                "model": draft.model,
                "tool_name": draft.tool_name,
                "action": draft.action,
                "policy": draft.policy,
                "approval_state": draft.approval_state,
                "blast_radius": draft.blast_radius,
                "external_correlation_ids": draft.external_correlation_ids,
                "expected_destination": draft.expected_destination,
                "route_id": destination.route_id,
                "route_match_reason": destination.route_match_reason(),
                "project_id": draft.project_id,
                "log_source_id": draft.log_source_id,
                "tenant_id": draft.tenant_id,
                "workspace_id": draft.workspace_id,
                "event_schema_version": draft.event_schema_version,
                "redaction_profile": draft.redaction_profile.as_deref().unwrap_or("incident_monitor_local_default"),
                "retention_profile": draft.retention_profile.as_deref().unwrap_or("incident_monitor_memory_signal"),
                "idempotency_key": idempotency_key,
            }))
        }
        _ => anyhow::bail!(
            "Destination `{}` uses {:?}, which is not a local Incident Monitor destination",
            destination.destination_id,
            destination.kind
        ),
    }
}

async fn memory_recurrence_count(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    destination_id: &str,
    target_ref: &str,
) -> u64 {
    let existing = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| {
            post.repo == draft.repo
                && post.fingerprint == draft.fingerprint
                && post.status == "posted"
                && post.destination_id.as_deref() == Some(destination_id)
                && post.target_ref.as_deref() == Some(target_ref)
        })
        .count() as u64;
    existing.saturating_add(1)
}

fn build_memory_summary(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    category: &str,
    recurrence_count: u64,
) -> String {
    let title = draft
        .title
        .as_deref()
        .or_else(|| incident.map(|row| row.title.as_str()))
        .map(safe_summary_text)
        .unwrap_or_else(|| "Incident Monitor failure".to_string());
    let risk = draft.risk_level.as_deref().unwrap_or("unknown");
    let risk_category = draft.risk_category.as_deref().unwrap_or("uncategorized");
    let confidence = draft.confidence.as_deref().unwrap_or("unknown");
    truncate_text(
        &format!(
            "{category}: {title}. fingerprint={} repo={} risk={} risk_category={} confidence={} recurrence_count={}",
            draft.fingerprint, draft.repo, risk, risk_category, confidence, recurrence_count
        ),
        800,
    )
}

fn publish_local_event(
    state: &AppState,
    destination: &LocalDestinationContext,
    draft: &IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
    target_ref: &str,
) {
    let event_name = match destination.kind {
        IncidentMonitorDestinationKind::InternalMemory => "incident_monitor.internal_memory.stored",
        _ => "incident_monitor.telemetry.recorded",
    };
    state.event_bus.publish(EngineEvent::new(
        event_name,
        json!({
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "target_ref": target_ref,
            "destination_id": destination.destination_id,
            "external_id": post.external_id,
            "evidence_digest": post.evidence_digest,
        }),
    ));
}

fn receipt_provider(destination: &LocalDestinationContext) -> &'static str {
    match destination.kind {
        IncidentMonitorDestinationKind::InternalMemory => "incident_monitor_internal_memory",
        _ => "incident_monitor_telemetry",
    }
}

fn posting_status(destination: &LocalDestinationContext) -> &'static str {
    match destination.kind {
        IncidentMonitorDestinationKind::InternalMemory => "memory_summary_storing",
        _ => "telemetry_recording",
    }
}

fn deterministic_record_id(
    destination: &LocalDestinationContext,
    target_ref: &str,
    draft: &IncidentMonitorDraftRecord,
    evidence_digest: &str,
) -> anyhow::Result<String> {
    let prefix = match destination.kind {
        IncidentMonitorDestinationKind::Telemetry => "bmtel",
        IncidentMonitorDestinationKind::InternalMemory => "bmmem",
        _ => anyhow::bail!(
            "Destination `{}` uses {:?}, which is not a local Incident Monitor destination",
            destination.destination_id,
            destination.kind
        ),
    };
    let digest = sha256_hex(&[
        &destination.destination_id,
        destination.kind_label()?,
        target_ref,
        &draft.repo,
        &draft.fingerprint,
        evidence_digest,
    ]);
    Ok(format!("{prefix}_{}", &digest[..24]))
}

fn compute_evidence_digest(draft: &IncidentMonitorDraftRecord) -> String {
    sha256_hex(&[
        draft.repo.as_str(),
        draft.fingerprint.as_str(),
        draft.title.as_deref().unwrap_or(""),
        draft.detail.as_deref().unwrap_or(""),
    ])
}

fn build_idempotency_key(
    destination_id: &str,
    kind: &str,
    target_ref: &str,
    fingerprint: &str,
    operation: &str,
    digest: &str,
) -> String {
    sha256_hex(&[
        destination_id,
        kind,
        target_ref,
        fingerprint,
        operation,
        digest,
    ])
}

fn config_string(config: &Option<Value>, keys: &[&str]) -> Option<String> {
    let config = config.as_ref()?;
    keys.iter()
        .find_map(|key| config.get(*key).and_then(Value::as_str))
        .and_then(normalize_config_string)
}

fn normalize_config_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_memory_category(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        MEMORY_CATEGORY_FAILURE_PATTERN
        | MEMORY_CATEGORY_RECURRENCE
        | MEMORY_CATEGORY_POLICY_GAP
        | MEMORY_CATEGORY_SAFETY_RISK => Some(normalized),
        _ => None,
    }
}

fn safe_summary_text(value: &str) -> String {
    truncate_text(&redact_sensitive_text(value), 240)
}

fn safe_evidence_refs(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| truncate_text(&redact_sensitive_text(value), 500))
        .collect()
}

fn redact_sensitive_text(value: &str) -> String {
    value
        .lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_sensitive_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for marker in [
        "authorization:",
        "authorization=",
        "password:",
        "password=",
        "secret:",
        "secret=",
        "token:",
        "token=",
        "api_key:",
        "api_key=",
        "apikey:",
        "apikey=",
    ] {
        if let Some(index) = lower.find(marker) {
            let keep = &line[..index + marker.len()];
            return format!("{keep}[redacted]");
        }
    }
    line.to_string()
}

#[cfg(test)]
mod hosted_policy_tests {
    use super::*;

    #[tokio::test]
    async fn telemetry_retry_recovers_flushed_record_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let posts_path = temp.path().join("posts.json");
        let drafts_path = temp.path().join("drafts.json");
        let sink = temp.path().join("telemetry/events.jsonl");
        let destination = LocalDestinationContext {
            destination_id: "telemetry-recovery".into(),
            route_id: None,
            route_match_reason: None,
            kind: IncidentMonitorDestinationKind::Telemetry,
            telemetry_path: Some(sink.to_string_lossy().into_owned()),
            memory_category: None,
            config: None,
        };
        let draft = IncidentMonitorDraftRecord {
            draft_id: "draft-recovery".into(),
            fingerprint: "fingerprint-recovery".into(),
            repo: "acme/platform".into(),
            title: Some("Recover telemetry delivery".into()),
            status: "ready".into(),
            created_at_ms: now_ms(),
            ..Default::default()
        };
        let target_ref = destination.target_ref().unwrap();
        let digest = compute_evidence_digest(&draft);
        let key = build_idempotency_key(
            &destination.destination_id,
            destination.kind_label().unwrap(),
            &target_ref,
            &draft.fingerprint,
            destination.operation().unwrap(),
            &digest,
        );
        let record_id =
            deterministic_record_id(&destination, &target_ref, &draft, &digest).unwrap();

        let mut before_crash = crate::app::state::tests::ready_test_state().await;
        before_crash.incident_monitor_posts_path = posts_path.clone();
        before_crash.incident_monitor_drafts_path = drafts_path.clone();
        let receipt = build_receipt(
            &before_crash,
            &draft,
            None,
            &destination,
            &target_ref,
            &record_id,
            &key,
            &digest,
        )
        .await
        .unwrap();
        let old = now_ms() - 11 * 60 * 1000;
        before_crash
            .put_incident_monitor_post(IncidentMonitorPostRecord {
                post_id: "pending-before-crash".into(),
                draft_id: draft.draft_id.clone(),
                fingerprint: draft.fingerprint.clone(),
                repo: draft.repo.clone(),
                operation: "record_telemetry".into(),
                status: "pending".into(),
                destination_id: Some(destination.destination_id.clone()),
                destination_kind: Some(IncidentMonitorDestinationKind::Telemetry),
                target_ref: Some(target_ref.clone()),
                evidence_digest: Some(digest.clone()),
                idempotency_key: key.clone(),
                created_at_ms: old,
                updated_at_ms: old,
                ..Default::default()
            })
            .await
            .unwrap();
        tokio::fs::create_dir_all(sink.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&sink, format!("{receipt}\n"))
            .await
            .unwrap();
        drop(before_crash);

        let mut restarted = crate::app::state::tests::ready_test_state().await;
        restarted.incident_monitor_posts_path = posts_path.clone();
        restarted.incident_monitor_drafts_path = drafts_path;
        restarted.load_incident_monitor_posts().await.unwrap();
        assert_eq!(restarted.list_incident_monitor_posts(10).await.len(), 1);
        let outcome =
            publish_local_record(&restarted, draft, None, &destination, &target_ref, &digest)
                .await
                .unwrap();
        let recovered_draft = outcome.draft.clone();
        let post = outcome.post.expect("recovered post");
        assert_eq!(post.status, "posted");
        assert_eq!(post.external_id.as_deref(), Some(record_id.as_str()));
        assert_eq!(post.receipt.as_ref(), Some(&receipt));
        let contents = tokio::fs::read_to_string(&sink).await.unwrap();
        assert_eq!(
            contents.lines().count(),
            1,
            "duplicate telemetry: {contents}"
        );
        let duplicate = publish_local_record(
            &restarted,
            recovered_draft,
            None,
            &destination,
            &target_ref,
            &digest,
        )
        .await
        .unwrap();
        assert_eq!(duplicate.action, "skip_duplicate");
        assert_eq!(tokio::fs::read_to_string(&sink).await.unwrap(), contents);

        let mut verified = crate::app::state::tests::ready_test_state().await;
        verified.incident_monitor_posts_path = posts_path;
        verified.load_incident_monitor_posts().await.unwrap();
        assert_eq!(
            verified
                .get_incident_monitor_post(&post.post_id)
                .await
                .unwrap()
                .status,
            "posted"
        );
    }

    #[tokio::test]
    async fn telemetry_sink_reconciliation_separates_partial_tail_and_serializes_writers() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
        let sink = temp.path().join("telemetry/events.jsonl");
        tokio::fs::create_dir_all(sink.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&sink, b"{incomplete").await.unwrap();
        let receipt = json!({
            "provider": "incident_monitor_telemetry",
            "operation": "record_telemetry",
            "status": "posted",
            "record_id": "bmtel_concurrent",
            "idempotency_key": "key-concurrent",
            "destination_id": "telemetry-primary",
            "target_ref": "telemetry:events",
        });
        let (first, second) = tokio::join!(
            persist_incident_monitor_telemetry(&state, &sink, &receipt),
            persist_incident_monitor_telemetry(&state, &sink, &receipt),
        );
        assert_eq!(first.unwrap(), receipt);
        assert_eq!(second.unwrap(), receipt);
        let contents = tokio::fs::read_to_string(&sink).await.unwrap();
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2, "unexpected sink: {contents}");
        assert_eq!(lines[0], "{incomplete");
        assert_eq!(serde_json::from_str::<Value>(lines[1]).unwrap(), receipt);

        let mut different_key = receipt.clone();
        different_key["idempotency_key"] = json!("different-key");
        assert_eq!(
            persist_incident_monitor_telemetry(&state, &sink, &different_key)
                .await
                .unwrap(),
            different_key
        );
        assert_eq!(
            tokio::fs::read_to_string(&sink)
                .await
                .unwrap()
                .lines()
                .count(),
            3
        );

        // Simulate a separate process appending while this process retains an
        // index. The next locked publisher must scan that new suffix.
        let mut external = receipt.clone();
        external["idempotency_key"] = json!("external-key");
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&sink)
                .unwrap();
            writeln!(file, "{external}").unwrap();
            file.sync_data().unwrap();
        }
        assert_eq!(
            persist_incident_monitor_telemetry(&state, &sink, &external)
                .await
                .unwrap(),
            external
        );
        assert_eq!(
            tokio::fs::read_to_string(&sink)
                .await
                .unwrap()
                .lines()
                .count(),
            4
        );

        #[cfg(unix)]
        {
            // A replacement at the same pathname must discard the old
            // offsets, even when the replacement already has a valid receipt.
            let mut replacement_receipt = receipt.clone();
            replacement_receipt["idempotency_key"] = json!("replacement-key");
            let replacement = temp.path().join("replacement.jsonl");
            std::fs::write(&replacement, format!("{replacement_receipt}\n")).unwrap();
            std::fs::rename(&replacement, &sink).unwrap();
            assert_eq!(
                persist_incident_monitor_telemetry(&state, &sink, &replacement_receipt)
                    .await
                    .unwrap(),
                replacement_receipt
            );
            assert_eq!(
                tokio::fs::read_to_string(&sink)
                    .await
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        }
    }

    #[tokio::test]
    async fn telemetry_append_is_visible_before_success_returns() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
        let sink = temp.path().join("telemetry/events.jsonl");
        for sequence in 0..32 {
            let receipt = json!({"event": "write-completion", "sequence": sequence});
            persist_incident_monitor_telemetry(&state, &sink, &receipt)
                .await
                .unwrap();
            // Read synchronously: an async read would give a pending background
            // write another scheduling opportunity and could hide the race.
            let contents = std::fs::read_to_string(&sink).unwrap();
            let lines = contents.lines().collect::<Vec<_>>();
            assert_eq!(lines.len(), sequence + 1);
            assert_eq!(
                serde_json::from_str::<Value>(lines.last().unwrap()).unwrap(),
                receipt
            );
        }
    }

    #[tokio::test]
    async fn hosted_incident_paused_local_claim_is_retryable_and_preserves_completed_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let mut state =
            crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
        state.incident_monitor_posts_path = temp.path().join("posts.json");
        let claim = IncidentMonitorPostRecord {
            post_id: "original".into(),
            idempotency_key: "local-policy-retry".into(),
            status: "pending".into(),
            updated_at_ms: now_ms(),
            ..Default::default()
        };
        assert!(
            state
                .try_claim_incident_monitor_post_idempotency(claim.clone())
                .await
                .unwrap()
                .0
        );
        pause_local_claim(&state, &claim).await.unwrap();
        assert_eq!(
            state
                .get_incident_monitor_post(&claim.post_id)
                .await
                .unwrap()
                .status,
            "policy_paused"
        );
        let retry = IncidentMonitorPostRecord {
            post_id: "retry".into(),
            ..claim.clone()
        };
        assert!(
            state
                .try_claim_incident_monitor_post_idempotency(retry.clone())
                .await
                .unwrap()
                .0
        );
        let completed = IncidentMonitorPostRecord {
            status: "posted".into(),
            ..retry.clone()
        };
        state.put_incident_monitor_post(completed).await.unwrap();
        pause_local_claim(&state, &retry).await.unwrap();
        assert_eq!(
            state
                .get_incident_monitor_post(&retry.post_id)
                .await
                .unwrap()
                .status,
            "posted"
        );
        assert!(
            !state
                .try_claim_incident_monitor_post_idempotency(claim)
                .await
                .unwrap()
                .0
        );
    }

    #[tokio::test]
    async fn hosted_incident_telemetry_policy_outage_does_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
        let sink = temp.path().join("telemetry/events.jsonl");
        let receipt = json!({"event": "incident", "id": "policy-control"});

        // The same sink remains usable in an unconfigured standalone deployment.
        persist_incident_monitor_telemetry(&state, &sink, &receipt)
            .await
            .unwrap();
        let before = tokio::fs::read(&sink).await.unwrap();
        assert_eq!(
            String::from_utf8(before.clone()).unwrap().lines().count(),
            1
        );

        state.enterprise.hosted_policy.configure_test_source(
            "org-a",
            "dep-a",
            temp.path().join("missing-policy.json"),
        );
        let error = persist_incident_monitor_telemetry(&state, &sink, &receipt)
            .await
            .unwrap_err();
        assert!(error.is::<crate::incident_monitor::HostedPolicyUnavailable>());
        assert_eq!(tokio::fs::read(&sink).await.unwrap(), before);
        let new_sink = temp.path().join("blocked/events.jsonl");
        assert!(
            persist_incident_monitor_telemetry(&state, &new_sink, &receipt)
                .await
                .is_err()
        );
        assert!(!new_sink.parent().unwrap().exists());
    }
}
