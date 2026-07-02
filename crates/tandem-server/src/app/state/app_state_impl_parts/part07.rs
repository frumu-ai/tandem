// Top-level state helpers split from mod.rs for the file-size gate
// (same module via include!).

pub async fn run_session_part_persister(state: AppState) {
    crate::app::tasks::run_session_part_persister(state).await
}

pub async fn run_status_indexer(state: AppState) {
    crate::app::tasks::run_status_indexer(state).await
}

pub async fn run_agent_team_supervisor(state: AppState) {
    crate::app::tasks::run_agent_team_supervisor(state).await
}

pub async fn run_incident_monitor(state: AppState) {
    crate::app::tasks::run_incident_monitor(state).await
}

pub async fn run_incident_monitor_recovery_sweep(state: AppState) {
    crate::app::tasks::run_incident_monitor_recovery_sweep(state).await
}

pub async fn run_usage_aggregator(state: AppState) {
    crate::app::tasks::run_usage_aggregator(state).await
}

pub async fn run_optimization_scheduler(state: AppState) {
    crate::app::tasks::run_optimization_scheduler(state).await
}

pub async fn process_incident_monitor_event(
    state: &AppState,
    event: &EngineEvent,
    config: &IncidentMonitorConfig,
) -> anyhow::Result<IncidentMonitorIncidentRecord> {
    let submission =
        crate::incident_monitor::service::build_incident_monitor_submission_from_event(state, config, event)
            .await?;
    let duplicate_matches = crate::http::incident_monitor::incident_monitor_failure_pattern_matches(
        state,
        submission.repo.as_deref().unwrap_or_default(),
        submission.fingerprint.as_deref().unwrap_or_default(),
        submission.title.as_deref(),
        submission.detail.as_deref(),
        &submission.excerpt,
        3,
    )
    .await;
    let fingerprint = submission
        .fingerprint
        .clone()
        .ok_or_else(|| anyhow::anyhow!("incident monitor submission fingerprint missing"))?;
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = config
        .workspace_root
        .clone()
        .unwrap_or(default_workspace_root);
    let now = now_ms();
    let quality_gate =
        crate::incident_monitor::service::evaluate_incident_monitor_submission_quality(&submission);

    let existing = state
        .incident_monitor_incidents
        .read()
        .await
        .values()
        .find(|row| row.fingerprint == fingerprint)
        .cloned();

    let mut incident = if let Some(mut row) = existing {
        row.occurrence_count = row.occurrence_count.saturating_add(1);
        row.updated_at_ms = now;
        row.last_seen_at_ms = Some(now);
        if row.excerpt.is_empty() {
            row.excerpt = submission.excerpt.clone();
        }
        if row.confidence.is_none() {
            row.confidence = submission.confidence.clone();
        }
        if row.risk_level.is_none() {
            row.risk_level = submission.risk_level.clone();
        }
        if row.actor.is_none() {
            row.actor = submission.actor.clone();
        }
        if row.model.is_none() {
            row.model = submission.model.clone();
        }
        if row.tool_name.is_none() {
            row.tool_name = submission.tool_name.clone();
        }
        if row.action.is_none() {
            row.action = submission.action.clone();
        }
        if row.policy.is_none() {
            row.policy = submission.policy.clone();
        }
        if row.approval_state.is_none() {
            row.approval_state = submission.approval_state.clone();
        }
        if row.risk_category.is_none() {
            row.risk_category = submission.risk_category.clone();
        }
        if row.blast_radius.is_none() {
            row.blast_radius = submission.blast_radius.clone();
        }
        merge_incident_monitor_missing_submission_values(
            &mut row.external_correlation_ids,
            &submission.external_correlation_ids,
        );
        if row.expected_destination.is_none() {
            row.expected_destination = submission.expected_destination.clone();
        }
        row.quality_gate = Some(quality_gate.clone());
        for evidence_ref in &submission.evidence_refs {
            if !row
                .evidence_refs
                .iter()
                .any(|existing| existing == evidence_ref)
            {
                row.evidence_refs.push(evidence_ref.clone());
            }
        }
        row
    } else {
        IncidentMonitorIncidentRecord {
            incident_id: format!("failure-incident-{}", uuid::Uuid::new_v4().simple()),
            fingerprint: fingerprint.clone(),
            event_type: event.event_type.clone(),
            status: "queued".to_string(),
            repo: submission.repo.clone().unwrap_or_default(),
            workspace_root,
            title: submission
                .title
                .clone()
                .unwrap_or_else(|| format!("Failure detected in {}", event.event_type)),
            detail: submission.detail.clone(),
            excerpt: submission.excerpt.clone(),
            source: submission.source.clone(),
            run_id: submission.run_id.clone(),
            session_id: submission.session_id.clone(),
            correlation_id: submission.correlation_id.clone(),
            component: submission.component.clone(),
            level: submission.level.clone(),
            occurrence_count: 1,
            created_at_ms: now,
            updated_at_ms: now,
            last_seen_at_ms: Some(now),
            draft_id: None,
            triage_run_id: None,
            last_error: None,
            confidence: submission.confidence.clone(),
            risk_level: submission.risk_level.clone(),
            actor: submission.actor.clone(),
            model: submission.model.clone(),
            tool_name: submission.tool_name.clone(),
            action: submission.action.clone(),
            policy: submission.policy.clone(),
            approval_state: submission.approval_state.clone(),
            risk_category: submission.risk_category.clone(),
            blast_radius: submission.blast_radius.clone(),
            external_correlation_ids: submission.external_correlation_ids.clone(),
            expected_destination: submission.expected_destination.clone(),
            evidence_refs: submission.evidence_refs.clone(),
            quality_gate: Some(quality_gate.clone()),
            duplicate_summary: None,
            duplicate_matches: None,
            event_payload: Some(event.properties.clone()),
            ..IncidentMonitorIncidentRecord::default()
        }
    };
    state.put_incident_monitor_incident(incident.clone()).await?;

    if !duplicate_matches.is_empty() {
        incident.status = "duplicate_suppressed".to_string();
        let duplicate_summary =
            crate::http::incident_monitor::build_incident_monitor_duplicate_summary(&duplicate_matches);
        incident.duplicate_summary = Some(duplicate_summary.clone());
        incident.duplicate_matches = Some(duplicate_matches.clone());
        incident.updated_at_ms = now_ms();
        state.put_incident_monitor_incident(incident.clone()).await?;
        state.event_bus.publish(EngineEvent::new(
            "incident_monitor.incident.duplicate_suppressed",
            serde_json::json!({
                "incident_id": incident.incident_id,
                "fingerprint": incident.fingerprint,
                "eventType": incident.event_type,
                "status": incident.status,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
            }),
        ));
        return Ok(incident);
    }

    let draft = match state.submit_incident_monitor_draft(submission).await {
        Ok(draft) => draft,
        Err(error) => {
            let error_text = error.to_string();
            incident.status = if error_text.contains("signal quality gate") {
                "quality_gate_blocked".to_string()
            } else {
                "draft_failed".to_string()
            };
            incident.last_error = Some(truncate_text(&error_text, 500));
            incident.updated_at_ms = now_ms();
            state.put_incident_monitor_incident(incident.clone()).await?;
            state.event_bus.publish(EngineEvent::new(
                "incident_monitor.incident.detected",
                serde_json::json!({
                    "incident_id": incident.incident_id,
                    "fingerprint": incident.fingerprint,
                    "eventType": incident.event_type,
                    "draft_id": incident.draft_id,
                    "triage_run_id": incident.triage_run_id,
                    "status": incident.status,
                    "detail": incident.last_error,
                }),
            ));
            return Ok(incident);
        }
    };
    incident.draft_id = Some(draft.draft_id.clone());
    incident.status = "draft_created".to_string();
    state.put_incident_monitor_incident(incident.clone()).await?;

    match crate::http::incident_monitor::ensure_incident_monitor_triage_run(
        state.clone(),
        &draft.draft_id,
        true,
    )
    .await
    {
        Ok((updated_draft, _run_id, _deduped)) => {
            incident.triage_run_id = updated_draft.triage_run_id.clone();
            if incident.triage_run_id.is_some() {
                incident.status = "triage_queued".to_string();
            }
            incident.last_error = None;
        }
        Err(error) => {
            incident.status = "draft_created".to_string();
            incident.last_error = Some(truncate_text(&error.to_string(), 500));
        }
    }

    if let Some(draft_id) = incident.draft_id.clone() {
        let latest_draft = state
            .get_incident_monitor_draft(&draft_id)
            .await
            .unwrap_or(draft.clone());
        match crate::incident_monitor::router::publish_draft(
            state,
            crate::incident_monitor::router::IncidentMonitorPublishRequest {
                draft_id: draft_id.clone(),
                incident_id: Some(incident.incident_id.clone()),
                mode: crate::incident_monitor_github::PublishMode::Auto,
                destination_ids: Vec::new(),
            },
        )
        .await
        {
            Ok(outcome) => {
                incident.status = outcome.action;
                incident.last_error = None;
            }
            Err(error) => {
                let detail = truncate_text(&error.to_string(), 500);
                incident.last_error = Some(detail.clone());
                let mut failed_draft = latest_draft;
                failed_draft.status = "github_post_failed".to_string();
                failed_draft.github_status = Some("github_post_failed".to_string());
                failed_draft.last_post_error = Some(detail.clone());
                let evidence_digest = failed_draft.evidence_digest.clone();
                let _ = state.put_incident_monitor_draft(failed_draft.clone()).await;
                let _ = crate::incident_monitor::router::record_publish_failure(
                    state,
                    &failed_draft,
                    Some(&incident.incident_id),
                    "auto_post",
                    evidence_digest.as_deref(),
                    &detail,
                )
                .await;
            }
        }
    }

    incident.updated_at_ms = now_ms();
    state.put_incident_monitor_incident(incident.clone()).await?;
    state.event_bus.publish(EngineEvent::new(
        "incident_monitor.incident.detected",
        serde_json::json!({
            "incident_id": incident.incident_id,
            "fingerprint": incident.fingerprint,
            "eventType": incident.event_type,
            "draft_id": incident.draft_id,
            "triage_run_id": incident.triage_run_id,
            "status": incident.status,
        }),
    ));
    Ok(incident)
}

pub fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

/// Constant-time equality for secrets, tokens, and their hashes. Both inputs are
/// hashed and the fixed-length digests compared without early exit, so neither
/// the contents nor the lengths leak through timing.
pub fn constant_time_str_eq(left: &str, right: &str) -> bool {
    let left_digest = Sha256::digest(left.as_bytes());
    let right_digest = Sha256::digest(right.as_bytes());
    let mut diff = 0u8;
    for (a, b) in left_digest.iter().zip(right_digest.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn automation_status_uses_scheduler_capacity(status: &AutomationRunStatus) -> bool {
    matches!(status, AutomationRunStatus::Running)
}

fn automation_status_holds_workspace_lock(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Running | AutomationRunStatus::Pausing
    )
}

pub async fn run_routine_scheduler(state: AppState) {
    crate::app::tasks::run_routine_scheduler(state).await
}

pub async fn run_routine_executor(state: AppState) {
    crate::app::tasks::run_routine_executor(state).await
}

pub async fn build_routine_prompt(state: &AppState, run: &RoutineRunRecord) -> String {
    crate::app::routines::build_routine_prompt(state, run).await
}

pub fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...<truncated>");
    out
}

pub async fn append_configured_output_artifacts(state: &AppState, run: &RoutineRunRecord) {
    crate::app::routines::append_configured_output_artifacts(state, run).await
}

pub fn default_model_spec_from_effective_config(config: &Value) -> Option<ModelSpec> {
    let provider_id = config
        .get("default_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let model_id = config
        .get("providers")
        .and_then(|v| v.get(provider_id))
        .and_then(|v| v.get("default_model"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    Some(ModelSpec {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

pub async fn resolve_routine_model_spec_for_run(
    state: &AppState,
    run: &RoutineRunRecord,
) -> (Option<ModelSpec>, String) {
    crate::app::routines::resolve_routine_model_spec_for_run(state, run).await
}

fn normalize_non_empty_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw {
        let normalized = item.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(not(feature = "browser"))]
impl AppState {
    pub async fn close_browser_sessions_for_owner(&self, _owner_session_id: &str) -> usize {
        0
    }

    pub async fn close_all_browser_sessions(&self) -> usize {
        0
    }

    pub async fn browser_status(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false, "sidecar": { "found": false }, "browser": { "found": false } })
    }

    pub async fn browser_smoke_test(
        &self,
        _url: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("browser feature disabled")
    }

    pub async fn install_browser_sidecar(&self) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("browser feature disabled")
    }

    pub async fn browser_health_summary(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false })
    }
}
