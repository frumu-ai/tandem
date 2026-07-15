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
    // TAN-556: actually persist the telemetry record to the configured sink
    // before reporting the publish as posted, so `record_telemetry` isn't a
    // receipt-only no-op. A write failure surfaces as a publish failure rather
    // than a false success.
    if destination.kind == IncidentMonitorDestinationKind::Telemetry {
        let sink = resolve_telemetry_sink_path(state, &destination.telemetry_path());
        persist_incident_monitor_telemetry(&sink, &receipt).await?;
    }
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
    let post = state.put_incident_monitor_post(post).await?;
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

/// Append a telemetry record as a JSON line to the resolved sink file, creating
/// parent directories as needed (TAN-556).
async fn persist_incident_monitor_telemetry(
    path: &std::path::Path,
    receipt: &Value,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use tokio::io::AsyncWriteExt;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create telemetry sink directory {}", parent.display()))?;
        }
    }
    let mut line = serde_json::to_string(receipt)?;
    line.push('\n');
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("open telemetry sink {}", path.display()))?;
    file.write_all(line.as_bytes())
        .await
        .with_context(|| format!("append telemetry record to {}", path.display()))?;
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
