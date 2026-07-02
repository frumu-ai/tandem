#![cfg_attr(
    not(test),
    deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use serde_json::{json, Value};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{
    AppState, IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord,
    IncidentMonitorLogCandidate, IncidentMonitorLogSource, IncidentMonitorLogSourceRuntimeStatus,
    IncidentMonitorLogSourceState, IncidentMonitorLogStartPosition,
    IncidentMonitorLogWatcherStatus, IncidentMonitorMonitoredProject, IncidentMonitorSubmission,
};

#[derive(Debug, Clone)]
pub struct IncidentMonitorLogReplayResult {
    pub incident: IncidentMonitorIncidentRecord,
    pub draft: Option<IncidentMonitorDraftRecord>,
}

pub async fn run_incident_monitor_log_watcher(state: AppState) {
    state
        .update_incident_monitor_log_watcher_status(|status| {
            status.running = true;
            status.last_error = None;
        })
        .await;
    loop {
        let now_ms = crate::now_ms();
        if let Err(error) = poll_enabled_sources(&state, now_ms).await {
            state
                .update_incident_monitor_log_watcher_status(|status| {
                    status.running = true;
                    status.last_error = Some(error.to_string());
                })
                .await;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn poll_enabled_sources(state: &AppState, now_ms: u64) -> anyhow::Result<()> {
    let config = state.incident_monitor_config().await;
    let enabled_projects = config
        .monitored_projects
        .iter()
        .filter(|project| project.enabled && !project.paused)
        .count();
    let enabled_sources = config
        .monitored_projects
        .iter()
        .filter(|project| project.enabled && !project.paused)
        .flat_map(|project| project.log_sources.iter())
        .filter(|source| source.enabled && !source.paused)
        .count();
    if !config.enabled || config.paused {
        state
            .update_incident_monitor_log_watcher_status(|status| {
                status.running = true;
                status.enabled_projects = enabled_projects;
                status.enabled_sources = enabled_sources;
                status.last_poll_at_ms = Some(now_ms);
            })
            .await;
        return Ok(());
    }

    let previous_status = state
        .incident_monitor_log_watcher_status
        .read()
        .await
        .clone();
    let mut source_statuses = previous_status.sources;
    for project in config
        .monitored_projects
        .iter()
        .filter(|project| project.enabled && !project.paused)
    {
        for source in project
            .log_sources
            .iter()
            .filter(|source| source.enabled && !source.paused)
        {
            let key = format!("{}/{}", project.project_id, source.source_id);
            let last_poll_at = source_statuses
                .iter()
                .find(|row| format!("{}/{}", row.project_id, row.source_id) == key)
                .and_then(|row| row.last_poll_at_ms)
                .unwrap_or(0);
            if now_ms < last_poll_at.saturating_add(source.watch_interval_seconds * 1000) {
                continue;
            }
            let status = match poll_log_source_once(state, project, source, now_ms).await {
                Ok(status) => status,
                Err(error) => {
                    // A hard error on one source (e.g. a file truncated mid-read)
                    // must not abort the whole tick and discard the statuses
                    // accumulated for the other sources.
                    tracing::warn!(
                        project_id = %project.project_id,
                        source_id = %source.source_id,
                        error = %error,
                        "incident monitor log source poll failed; skipping this source for the tick"
                    );
                    continue;
                }
            };
            source_statuses.retain(|row| {
                !(row.project_id == status.project_id && row.source_id == status.source_id)
            });
            source_statuses.push(status);
        }
    }
    state
        .update_incident_monitor_log_watcher_status(|status| {
            status.running = true;
            status.enabled_projects = enabled_projects;
            status.enabled_sources = enabled_sources;
            status.last_poll_at_ms = Some(now_ms);
            status.last_error = None;
            status.sources = source_statuses;
        })
        .await;
    Ok(())
}

pub async fn poll_log_source_once(
    state: &AppState,
    project: &IncidentMonitorMonitoredProject,
    source: &IncidentMonitorLogSource,
    now_ms: u64,
) -> anyhow::Result<IncidentMonitorLogSourceRuntimeStatus> {
    let absolute_path = resolve_log_source_path(project, source)?;
    let mut source_state = state
        .get_incident_monitor_log_source_state(&project.project_id, &source.source_id)
        .await
        .unwrap_or_else(|| IncidentMonitorLogSourceState {
            project_id: project.project_id.clone(),
            source_id: source.source_id.clone(),
            path: absolute_path.display().to_string(),
            updated_at_ms: 0,
            ..IncidentMonitorLogSourceState::default()
        });
    source_state.path = absolute_path.display().to_string();

    let metadata = match fs::metadata(&absolute_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            source_state.last_error = Some("log file not found".to_string());
            source_state.consecutive_errors = source_state.consecutive_errors.saturating_add(1);
            source_state.updated_at_ms = now_ms;
            state
                .put_incident_monitor_log_source_state(source_state.clone())
                .await?;
            return Ok(status_from_state(
                &source_state,
                false,
                None,
                Some(now_ms),
                None,
                None,
            ));
        }
        Err(error) => return Err(error).context("failed to stat log file"),
    };
    let file_size = metadata.len();
    let inode = inode_for_metadata(&metadata);
    if source_state.offset == 0
        && source_state.inode.is_none()
        && !source_was_successfully_positioned(&source_state)
        && matches!(source.start_position, IncidentMonitorLogStartPosition::End)
    {
        source_state.offset = file_size;
        source_state.inode = inode.clone();
        source_state.positioned_at_ms = Some(now_ms);
        source_state.updated_at_ms = now_ms;
        source_state.last_error = None;
        source_state.consecutive_errors = 0;
        state
            .put_incident_monitor_log_source_state(source_state.clone())
            .await?;
        return Ok(status_from_state(
            &source_state,
            true,
            Some(file_size),
            Some(now_ms),
            None,
            None,
        ));
    }
    if source_state
        .inode
        .as_ref()
        .zip(inode.as_ref())
        .is_some_and(|(a, b)| a != b)
        || file_size < source_state.offset
    {
        source_state.offset = 0;
        source_state.partial_line = None;
        source_state.partial_line_offset_start = None;
    }
    source_state.inode = inode.clone();

    let read_len = file_size
        .saturating_sub(source_state.offset)
        .min(source.max_bytes_per_poll);
    if read_len == 0 {
        source_state.positioned_at_ms.get_or_insert(now_ms);
        source_state.updated_at_ms = now_ms;
        source_state.last_error = None;
        source_state.consecutive_errors = 0;
        state
            .put_incident_monitor_log_source_state(source_state.clone())
            .await?;
        return Ok(status_from_state(
            &source_state,
            true,
            Some(file_size),
            Some(now_ms),
            None,
            None,
        ));
    }

    let mut file = fs::File::open(&absolute_path).await?;
    file.seek(std::io::SeekFrom::Start(source_state.offset))
        .await?;
    let mut bytes = vec![0; read_len as usize];
    file.read_exact(&mut bytes).await?;
    let parse_result = crate::incident_monitor::log_parser::parse_log_candidates(
        project,
        source,
        &absolute_path,
        inode.clone(),
        source_state.offset,
        &bytes,
        source_state.partial_line.clone(),
        source_state.partial_line_offset_start,
    );
    let poll_start_offset = source_state.offset;
    let cap = source.max_candidates_per_poll;
    let parsed_count = parse_result.candidates.len();
    // A cap of 0 means "no per-poll cap"; otherwise we only process the first
    // `cap` candidates this poll.
    let truncated = cap > 0 && parsed_count > cap;
    let mut candidates = parse_result.candidates;
    if truncated {
        candidates.truncate(cap);
    }

    if truncated {
        // TAN-549: more candidates were parsed than we will process this poll.
        // Advancing the offset by the full read window would skip the remainder
        // permanently (silent incident loss). Instead resume from the end of the
        // last processed candidate and re-read the rest next poll; drop the
        // parser's partial-line state since that tail now lives beyond the new
        // offset and will be re-read.
        let resume_offset = candidates
            .last()
            .map(|candidate| candidate.offset_end)
            .unwrap_or_else(|| poll_start_offset.saturating_add(read_len))
            .max(poll_start_offset);
        let consumed = resume_offset.saturating_sub(poll_start_offset);
        source_state.offset = resume_offset;
        source_state.partial_line = None;
        source_state.partial_line_offset_start = None;
        source_state.total_bytes_read = source_state.total_bytes_read.saturating_add(consumed);
        tracing::warn!(
            project_id = %project.project_id,
            source_id = %source.source_id,
            processed = candidates.len(),
            deferred = parsed_count.saturating_sub(candidates.len()),
            "incident monitor log poll hit max_candidates_per_poll; deferring remaining candidates to the next poll instead of dropping them"
        );
    } else {
        source_state.offset = source_state.offset.saturating_add(read_len);
        source_state.partial_line = parse_result.next_partial_line;
        source_state.partial_line_offset_start = parse_result.next_partial_line_offset_start;
        source_state.total_bytes_read = source_state.total_bytes_read.saturating_add(read_len);
    }
    source_state.positioned_at_ms.get_or_insert(now_ms);
    source_state.total_candidates = source_state
        .total_candidates
        .saturating_add(candidates.len() as u64);
    source_state.updated_at_ms = now_ms;
    source_state.last_error = None;
    source_state.consecutive_errors = 0;

    let mut submitted = 0usize;
    let mut last_candidate_at_ms = None;
    let mut last_submitted_at_ms = None;
    for candidate in candidates {
        last_candidate_at_ms = Some(now_ms);
        if fingerprint_recent(
            &mut source_state,
            &candidate.fingerprint,
            source.fingerprint_cooldown_ms,
            now_ms,
        ) {
            continue;
        }
        if submit_log_candidate(state, project, source, candidate)
            .await?
            .is_some()
        {
            submitted += 1;
            last_submitted_at_ms = Some(now_ms);
        }
    }
    source_state.total_submitted = source_state
        .total_submitted
        .saturating_add(submitted as u64);
    prune_recent_fingerprints(&mut source_state, now_ms);
    state
        .put_incident_monitor_log_source_state(source_state.clone())
        .await?;
    Ok(status_from_state(
        &source_state,
        true,
        Some(file_size),
        Some(now_ms),
        last_candidate_at_ms,
        last_submitted_at_ms,
    ))
}

pub async fn submit_log_candidate(
    state: &AppState,
    project: &IncidentMonitorMonitoredProject,
    source: &IncidentMonitorLogSource,
    mut candidate: IncidentMonitorLogCandidate,
) -> anyhow::Result<Option<IncidentMonitorDraftRecord>> {
    let binding = project.source_binding(Some(source));
    let evidence_ref =
        crate::incident_monitor::log_artifacts::write_log_evidence_artifact(state, &candidate)
            .await?;
    if !candidate
        .evidence_refs
        .iter()
        .any(|row| row == &evidence_ref)
    {
        candidate.evidence_refs.push(evidence_ref.clone());
    }
    let now = crate::now_ms();
    let event_payload = json!({
        "project_id": project.project_id,
        "source_id": source.source_id,
        "source_kind": binding.source_kind.as_str(),
        "log_path": candidate.path,
        "offset_start": candidate.offset_start,
        "offset_end": candidate.offset_end,
        "inode": candidate.inode,
        "evidence_artifact": evidence_ref,
        "detected_at_ms": now,
        "mcp_server": project.mcp_server,
        "model_policy": project.model_policy,
        "allowed_destination_ids": binding.allowed_destination_ids.clone(),
        "default_destination_ids": binding.default_destination_ids.clone(),
        "default_route_tags": binding.default_route_tags.clone(),
        "tenant_id": binding.tenant_id.clone(),
        "workspace_id": binding.workspace_id.clone(),
        "event_schema_version": binding.event_schema_version.clone(),
        "approval_policy": binding.approval_policy.clone(),
        "redaction_profile": binding.redaction_profile.clone(),
        "retention_profile": binding.retention_profile.clone(),
    });
    let mut incident = latest_incident_monitor_incident_by_repo_fingerprint(
        state,
        &project.repo,
        &project.project_id,
        &source.source_id,
        &candidate.fingerprint,
    )
    .await
    .unwrap_or_else(|| IncidentMonitorIncidentRecord {
        incident_id: format!("failure-incident-{}", uuid::Uuid::new_v4().simple()),
        fingerprint: candidate.fingerprint.clone(),
        event_type: candidate.event.clone(),
        status: "queued".to_string(),
        repo: project.repo.clone(),
        workspace_root: project.workspace_root.clone(),
        title: candidate.title.clone(),
        project_id: Some(project.project_id.clone()),
        log_source_id: Some(source.source_id.clone()),
        source_kind: Some(binding.source_kind.clone()),
        detail: Some(candidate.detail.clone()),
        excerpt: candidate.excerpt.clone(),
        source: Some(candidate.source.clone()),
        component: candidate.component.clone(),
        level: Some(candidate.level.clone()),
        occurrence_count: 0,
        created_at_ms: now,
        updated_at_ms: now,
        last_seen_at_ms: Some(now),
        confidence: Some(candidate.confidence.clone()),
        risk_level: Some(candidate.risk_level.clone()),
        actor: candidate.actor.clone(),
        model: candidate.model.clone(),
        tool_name: candidate.tool_name.clone(),
        action: candidate.action.clone(),
        policy: candidate.policy.clone(),
        approval_state: candidate.approval_state.clone(),
        risk_category: candidate.risk_category.clone(),
        blast_radius: candidate.blast_radius.clone(),
        external_correlation_ids: candidate.external_correlation_ids.clone(),
        expected_destination: Some(candidate.expected_destination.clone()),
        route_tags: candidate.route_tags.clone(),
        allowed_destination_ids: binding.allowed_destination_ids.clone(),
        default_destination_ids: binding.default_destination_ids.clone(),
        tenant_id: binding.tenant_id.clone(),
        workspace_id: binding.workspace_id.clone(),
        event_schema_version: binding.event_schema_version.clone(),
        source_approval_policy: Some(binding.approval_policy.clone()),
        redaction_profile: binding.redaction_profile.clone(),
        retention_profile: binding.retention_profile.clone(),
        evidence_refs: candidate.evidence_refs.clone(),
        event_payload: Some(event_payload.clone()),
        ..IncidentMonitorIncidentRecord::default()
    });
    incident.occurrence_count = incident.occurrence_count.saturating_add(1).max(1);
    incident.updated_at_ms = now;
    incident.last_seen_at_ms = Some(now);
    if incident.workspace_root.trim().is_empty() {
        incident.workspace_root = project.workspace_root.clone();
    }
    incident
        .project_id
        .get_or_insert_with(|| project.project_id.clone());
    incident
        .log_source_id
        .get_or_insert_with(|| source.source_id.clone());
    incident
        .source_kind
        .get_or_insert_with(|| binding.source_kind.clone());
    if incident.actor.is_none() {
        incident.actor = candidate.actor.clone();
    }
    if incident.model.is_none() {
        incident.model = candidate.model.clone();
    }
    if incident.tool_name.is_none() {
        incident.tool_name = candidate.tool_name.clone();
    }
    if incident.action.is_none() {
        incident.action = candidate.action.clone();
    }
    if incident.policy.is_none() {
        incident.policy = candidate.policy.clone();
    }
    if incident.approval_state.is_none() {
        incident.approval_state = candidate.approval_state.clone();
    }
    if incident.risk_category.is_none() {
        incident.risk_category = candidate.risk_category.clone();
    }
    if incident.blast_radius.is_none() {
        incident.blast_radius = candidate.blast_radius.clone();
    }
    merge_string_values(
        &mut incident.external_correlation_ids,
        &candidate.external_correlation_ids,
    );
    merge_string_values(&mut incident.route_tags, &candidate.route_tags);
    if incident.allowed_destination_ids.is_empty() {
        incident.allowed_destination_ids = binding.allowed_destination_ids.clone();
    }
    if incident.default_destination_ids.is_empty() {
        incident.default_destination_ids = binding.default_destination_ids.clone();
    }
    incident.tenant_id = incident
        .tenant_id
        .clone()
        .or_else(|| binding.tenant_id.clone());
    incident.workspace_id = incident
        .workspace_id
        .clone()
        .or_else(|| binding.workspace_id.clone());
    incident.event_schema_version = incident
        .event_schema_version
        .clone()
        .or_else(|| binding.event_schema_version.clone());
    incident
        .source_approval_policy
        .get_or_insert_with(|| binding.approval_policy.clone());
    incident.redaction_profile = incident
        .redaction_profile
        .clone()
        .or_else(|| binding.redaction_profile.clone());
    incident.retention_profile = incident
        .retention_profile
        .clone()
        .or_else(|| binding.retention_profile.clone());
    merge_evidence_refs(&mut incident.evidence_refs, &candidate.evidence_refs);
    incident.event_payload = Some(merge_payload(incident.event_payload.clone(), event_payload));

    if let Some(draft_id) = incident.draft_id.as_deref() {
        if let Some(draft) = state.get_incident_monitor_draft(draft_id).await {
            state.put_incident_monitor_incident(incident).await?;
            return Ok(Some(draft));
        }
    }

    state
        .put_incident_monitor_incident(incident.clone())
        .await?;
    let submission = IncidentMonitorSubmission {
        project_id: Some(project.project_id.clone()),
        workspace_root: Some(project.workspace_root.clone()),
        log_source_id: Some(source.source_id.clone()),
        source_kind: Some(binding.source_kind.clone()),
        repo: Some(project.repo.clone()),
        title: Some(candidate.title.clone()),
        detail: Some(candidate.detail.clone()),
        source: Some(candidate.source.clone()),
        file_name: Some(candidate.path.clone()),
        process: candidate.process.clone(),
        component: candidate.component.clone(),
        event: Some(candidate.event.clone()),
        level: Some(candidate.level.clone()),
        excerpt: candidate.excerpt.clone(),
        fingerprint: Some(candidate.fingerprint.clone()),
        confidence: Some(candidate.confidence.clone()),
        risk_level: Some(candidate.risk_level.clone()),
        actor: candidate.actor.clone(),
        model: candidate.model.clone(),
        tool_name: candidate.tool_name.clone(),
        action: candidate.action.clone(),
        policy: candidate.policy.clone(),
        approval_state: candidate.approval_state.clone(),
        risk_category: candidate.risk_category.clone(),
        blast_radius: candidate.blast_radius.clone(),
        external_correlation_ids: candidate.external_correlation_ids.clone(),
        expected_destination: Some(candidate.expected_destination.clone()),
        route_tags: candidate.route_tags.clone(),
        allowed_destination_ids: binding.allowed_destination_ids.clone(),
        default_destination_ids: binding.default_destination_ids.clone(),
        tenant_id: binding.tenant_id.clone(),
        workspace_id: binding.workspace_id.clone(),
        event_schema_version: binding.event_schema_version.clone(),
        source_approval_policy: Some(binding.approval_policy.clone()),
        redaction_profile: binding.redaction_profile.clone(),
        retention_profile: binding.retention_profile.clone(),
        evidence_refs: candidate.evidence_refs.clone(),
        ..IncidentMonitorSubmission::default()
    };
    let mut draft = match state.submit_incident_monitor_draft(submission).await {
        Ok(draft) => draft,
        Err(error) => {
            incident.status = "draft_failed".to_string();
            incident.last_error = Some(error.to_string());
            state.put_incident_monitor_incident(incident).await?;
            return Ok(None);
        }
    };
    if project.require_approval_for_new_issues && draft.status != "approval_required" {
        draft.status = "approval_required".to_string();
        draft = state.put_incident_monitor_draft(draft).await?;
    }
    incident.draft_id = Some(draft.draft_id.clone());
    incident.status = "draft_created".to_string();
    state
        .put_incident_monitor_incident(incident.clone())
        .await?;
    if project.auto_create_new_issues {
        if let Ok((updated_draft, _run_id, _deduped)) =
            crate::http::incident_monitor::ensure_incident_monitor_triage_run(
                state.clone(),
                &draft.draft_id,
                true,
            )
            .await
        {
            draft = updated_draft;
        }
    }
    Ok(Some(draft))
}

pub async fn reset_log_source_offset(
    state: &AppState,
    project: &IncidentMonitorMonitoredProject,
    source: &IncidentMonitorLogSource,
    now_ms: u64,
) -> anyhow::Result<IncidentMonitorLogSourceState> {
    let absolute_path = resolve_log_source_path(project, source)?;
    let metadata = fs::metadata(&absolute_path).await.ok();
    let inode = metadata.as_ref().and_then(inode_for_metadata);
    let mut source_state = state
        .get_incident_monitor_log_source_state(&project.project_id, &source.source_id)
        .await
        .unwrap_or_else(|| IncidentMonitorLogSourceState {
            project_id: project.project_id.clone(),
            source_id: source.source_id.clone(),
            ..IncidentMonitorLogSourceState::default()
        });
    source_state.path = absolute_path.display().to_string();
    source_state.inode = inode;
    source_state.offset = 0;
    source_state.partial_line = None;
    source_state.partial_line_offset_start = None;
    source_state.last_line_hash = None;
    source_state.recent_fingerprints.clear();
    source_state.positioned_at_ms = Some(now_ms);
    source_state.updated_at_ms = now_ms;
    source_state.last_error = None;
    source_state.consecutive_errors = 0;
    let source_state = state
        .put_incident_monitor_log_source_state(source_state.clone())
        .await?;
    let file_size = metadata.as_ref().map(|metadata| metadata.len());
    state
        .update_incident_monitor_log_watcher_status(|status| {
            status.sources.retain(|row| {
                !(row.project_id == source_state.project_id
                    && row.source_id == source_state.source_id)
            });
            status.sources.push(status_from_state(
                &source_state,
                metadata.is_some(),
                file_size,
                None,
                None,
                None,
            ));
        })
        .await;
    Ok(source_state)
}

pub async fn replay_latest_log_source_candidate(
    state: &AppState,
    project: &IncidentMonitorMonitoredProject,
    source: &IncidentMonitorLogSource,
) -> anyhow::Result<Option<IncidentMonitorLogReplayResult>> {
    let Some(incident) = latest_incident_monitor_incident_by_log_source(
        state,
        &project.project_id,
        &source.source_id,
    )
    .await
    else {
        return Ok(None);
    };
    let payload = incident
        .event_payload
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("latest log-source incident has no event payload"))?;
    let offset_start = payload
        .get("offset_start")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("latest log-source incident has no offset_start"))?;
    let offset_end = payload
        .get("offset_end")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("latest log-source incident has no offset_end"))?;
    if offset_end <= offset_start {
        anyhow::bail!("latest log-source incident has invalid offsets");
    }

    let absolute_path = resolve_log_source_path(project, source)?;
    let metadata = fs::metadata(&absolute_path)
        .await
        .with_context(|| format!("failed to stat log file {}", absolute_path.display()))?;
    if metadata.len() < offset_end {
        anyhow::bail!("latest log-source incident offsets exceed current log file size");
    }
    let inode = inode_for_metadata(&metadata);
    let mut file = fs::File::open(&absolute_path).await?;
    file.seek(std::io::SeekFrom::Start(offset_start)).await?;
    let read_len = offset_end.saturating_sub(offset_start);
    let mut bytes = vec![0; read_len as usize];
    file.read_exact(&mut bytes).await?;
    let parse_result = crate::incident_monitor::log_parser::parse_log_candidates(
        project,
        source,
        &absolute_path,
        inode,
        offset_start,
        &bytes,
        None,
        None,
    );
    let Some(candidate) = parse_result.candidates.into_iter().find(|candidate| {
        candidate.offset_start == offset_start && candidate.offset_end == offset_end
    }) else {
        anyhow::bail!("latest log-source incident offsets no longer parse to a candidate");
    };
    let draft = submit_log_candidate(state, project, source, candidate).await?;
    Ok(Some(IncidentMonitorLogReplayResult { incident, draft }))
}

pub fn resolve_log_source_path(
    project: &IncidentMonitorMonitoredProject,
    source: &IncidentMonitorLogSource,
) -> anyhow::Result<PathBuf> {
    let workspace_root = PathBuf::from(&project.workspace_root);
    if !workspace_root.is_absolute() {
        anyhow::bail!("workspace_root must be absolute");
    }
    let workspace_canonical = nearest_existing_path(&workspace_root)
        .and_then(|path| path.canonicalize().ok())
        .unwrap_or(workspace_root.clone());
    let raw_path = PathBuf::from(&source.path);
    let candidate = if raw_path.is_absolute() {
        raw_path
    } else {
        workspace_root.join(raw_path)
    };
    let nearest = nearest_existing_path(&candidate)
        .ok_or_else(|| anyhow::anyhow!("log source path has no existing parent"))?;
    let nearest_canonical = nearest.canonicalize()?;
    if !nearest_canonical.starts_with(&workspace_canonical) {
        anyhow::bail!("log source path escapes workspace_root");
    }
    Ok(candidate)
}

async fn latest_incident_monitor_incident_by_repo_fingerprint(
    state: &AppState,
    repo: &str,
    project_id: &str,
    source_id: &str,
    fingerprint: &str,
) -> Option<IncidentMonitorIncidentRecord> {
    state
        .incident_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| {
            row.repo == repo
                && row.fingerprint == fingerprint
                && row.project_id.as_deref() == Some(project_id)
                && row.log_source_id.as_deref() == Some(source_id)
        })
        .max_by_key(|row| row.updated_at_ms)
        .cloned()
}

async fn latest_incident_monitor_incident_by_log_source(
    state: &AppState,
    project_id: &str,
    source_id: &str,
) -> Option<IncidentMonitorIncidentRecord> {
    state
        .incident_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| {
            row.event_payload.as_ref().is_some_and(|payload| {
                payload
                    .get("project_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == project_id)
                    && payload
                        .get("source_id")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == source_id)
                    && payload
                        .get("offset_start")
                        .and_then(Value::as_u64)
                        .is_some()
                    && payload.get("offset_end").and_then(Value::as_u64).is_some()
            })
        })
        .max_by_key(|row| row.updated_at_ms)
        .cloned()
}

fn status_from_state(
    state: &IncidentMonitorLogSourceState,
    healthy: bool,
    file_size: Option<u64>,
    last_poll_at_ms: Option<u64>,
    last_candidate_at_ms: Option<u64>,
    last_submitted_at_ms: Option<u64>,
) -> IncidentMonitorLogSourceRuntimeStatus {
    IncidentMonitorLogSourceRuntimeStatus {
        project_id: state.project_id.clone(),
        source_id: state.source_id.clone(),
        path: state.path.clone(),
        healthy,
        offset: state.offset,
        inode: state.inode.clone(),
        file_size,
        last_poll_at_ms,
        last_candidate_at_ms,
        last_submitted_at_ms,
        last_error: state.last_error.clone(),
        consecutive_errors: state.consecutive_errors,
        total_bytes_read: state.total_bytes_read,
        total_candidates: state.total_candidates,
        total_submitted: state.total_submitted,
    }
}

fn source_was_successfully_positioned(state: &IncidentMonitorLogSourceState) -> bool {
    state.positioned_at_ms.is_some()
        || state.offset > 0
        || state.inode.is_some()
        || state.total_bytes_read > 0
        || state.total_candidates > 0
        || state.total_submitted > 0
        || (state.updated_at_ms > 0 && state.last_error.is_none())
}

fn fingerprint_recent(
    state: &mut IncidentMonitorLogSourceState,
    fingerprint: &str,
    cooldown_ms: u64,
    now_ms: u64,
) -> bool {
    let recent = state
        .recent_fingerprints
        .get(fingerprint)
        .map(|seen| now_ms.saturating_sub(*seen) < cooldown_ms)
        .unwrap_or(false);
    state
        .recent_fingerprints
        .insert(fingerprint.to_string(), now_ms);
    recent
}

fn prune_recent_fingerprints(state: &mut IncidentMonitorLogSourceState, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(24 * 60 * 60 * 1000);
    state.recent_fingerprints.retain(|_, seen| *seen >= cutoff);
    while state.recent_fingerprints.len() > 500 {
        let Some(first) = state.recent_fingerprints.keys().next().cloned() else {
            break;
        };
        state.recent_fingerprints.remove(&first);
    }
}

fn merge_evidence_refs(existing: &mut Vec<String>, incoming: &[String]) {
    merge_string_values(existing, incoming);
    if existing.len() > 50 {
        let keep_from = existing.len() - 50;
        existing.drain(0..keep_from);
    }
}

fn merge_string_values(existing: &mut Vec<String>, incoming: &[String]) {
    for value in incoming {
        if !existing.iter().any(|row| row == value) {
            existing.push(value.clone());
        }
    }
}

fn merge_payload(existing: Option<Value>, incoming: Value) -> Value {
    let mut base = existing.unwrap_or_else(|| json!({}));
    if let (Some(base), Some(incoming)) = (base.as_object_mut(), incoming.as_object()) {
        for (key, value) in incoming {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

fn nearest_existing_path(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(unix)]
fn inode_for_metadata(metadata: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino().to_string())
}

#[cfg(not(unix))]
fn inode_for_metadata(_metadata: &std::fs::Metadata) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IncidentMonitorConfig;
    use tempfile::tempdir;
    use tokio::fs;

    fn project(root: &Path) -> IncidentMonitorMonitoredProject {
        IncidentMonitorMonitoredProject {
            project_id: "customer-api".to_string(),
            name: "Customer API".to_string(),
            enabled: true,
            repo: "owner/customer-api".to_string(),
            workspace_root: root.display().to_string(),
            source_kind: crate::IncidentMonitorSourceKind::ExternalApp,
            allowed_destination_ids: vec![
                crate::INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            default_route_tags: vec!["customer-api".to_string()],
            tenant_id: Some("tenant-a".to_string()),
            auto_create_new_issues: false,
            log_sources: vec![source()],
            ..IncidentMonitorMonitoredProject::default()
        }
    }

    fn source() -> IncidentMonitorLogSource {
        IncidentMonitorLogSource {
            source_id: "api-log".to_string(),
            path: "logs/app.log".to_string(),
            source_kind: Some(crate::IncidentMonitorSourceKind::Ci),
            default_destination_ids: vec![
                crate::INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            default_route_tags: vec!["api-log".to_string()],
            workspace_id: Some("workspace-a".to_string()),
            start_position: IncidentMonitorLogStartPosition::End,
            ..IncidentMonitorLogSource::default()
        }
    }

    fn log_candidate(
        root: &Path,
        source_id: &str,
        fingerprint: &str,
    ) -> IncidentMonitorLogCandidate {
        IncidentMonitorLogCandidate {
            project_id: "customer-api".to_string(),
            source_id: source_id.to_string(),
            source_kind: crate::IncidentMonitorSourceKind::Ci,
            repo: "owner/customer-api".to_string(),
            workspace_root: root.display().to_string(),
            path: format!("logs/{source_id}.log"),
            offset_start: 0,
            offset_end: 64,
            title: "Shared source failure".to_string(),
            detail: format!("{source_id} reported the shared failure"),
            source: format!("log.{source_id}"),
            event: "log.error".to_string(),
            level: "error".to_string(),
            excerpt: vec![format!("ERROR {source_id} failed")],
            raw_excerpt_redacted: vec![format!("ERROR {source_id} failed")],
            fingerprint: fingerprint.to_string(),
            confidence: "high".to_string(),
            risk_level: "medium".to_string(),
            expected_destination: "incident_monitor_issue_draft".to_string(),
            ..IncidentMonitorLogCandidate::default()
        }
    }

    fn test_state(root: &Path) -> AppState {
        let mut state = AppState::new_starting("test".to_string(), true);
        state.incident_monitor_config_path = root.join("config.json");
        state.incident_monitor_drafts_path = root.join("drafts.json");
        state.incident_monitor_incidents_path = root.join("incidents.json");
        state.incident_monitor_posts_path = root.join("posts.json");
        state.incident_monitor_log_watcher_state_path = root.join("log-watcher-state.json");
        state.incident_monitor_log_evidence_dir = root.join("evidence");
        state
    }

    #[test]
    fn resolve_log_source_path_rejects_escape() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let mut source = source();
        source.path = outside.path().join("app.log").display().to_string();
        let err = resolve_log_source_path(&project(dir.path()), &source).unwrap_err();
        assert!(err.to_string().contains("escapes workspace_root"));
    }

    #[tokio::test]
    async fn poll_from_end_then_append_creates_draft_and_incident() {
        let dir = tempdir().unwrap();
        let state_dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir_all(&logs).await.unwrap();
        let log_path = logs.join("app.log");
        fs::write(&log_path, "INFO booted\n").await.unwrap();

        let state = test_state(state_dir.path());
        let project = project(dir.path());
        state
            .put_incident_monitor_config(IncidentMonitorConfig {
                enabled: true,
                monitored_projects: vec![project.clone()],
                ..IncidentMonitorConfig::default()
            })
            .await
            .unwrap();

        let first = poll_log_source_once(&state, &project, &source(), 1_000)
            .await
            .unwrap();
        assert_eq!(first.offset, "INFO booted\n".len() as u64);
        assert_eq!(state.list_incident_monitor_incidents(10).await.len(), 0);

        fs::write(
            &log_path,
            "INFO booted\nERROR upload failed\n    at normalize src/uploads.ts:42:1\n",
        )
        .await
        .unwrap();
        let second = poll_log_source_once(&state, &project, &source(), 2_000)
            .await
            .unwrap();
        assert_eq!(second.total_submitted, 1);
        let drafts = state.list_incident_monitor_drafts(10).await;
        let incidents = state.list_incident_monitor_incidents(10).await;
        assert_eq!(drafts.len(), 1);
        assert_eq!(incidents.len(), 1);
        assert_eq!(incidents[0].repo, "owner/customer-api");
        assert_eq!(
            incidents[0].workspace_root,
            dir.path().display().to_string()
        );
        assert_eq!(
            drafts[0].source_kind,
            Some(crate::IncidentMonitorSourceKind::Ci)
        );
        assert_eq!(
            drafts[0].allowed_destination_ids,
            vec![crate::INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()]
        );
        assert_eq!(
            drafts[0].default_destination_ids,
            vec![crate::INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()]
        );
        assert_eq!(
            drafts[0].route_tags,
            vec!["customer-api".to_string(), "api-log".to_string()]
        );
        assert_eq!(drafts[0].tenant_id.as_deref(), Some("tenant-a"));
        assert_eq!(drafts[0].workspace_id.as_deref(), Some("workspace-a"));
        assert_eq!(
            incidents[0].source_kind,
            Some(crate::IncidentMonitorSourceKind::Ci)
        );
    }

    #[tokio::test]
    async fn same_fingerprint_from_different_sources_creates_source_bound_incidents() {
        let dir = tempdir().unwrap();
        let state_dir = tempdir().unwrap();
        let state = test_state(state_dir.path());
        let api_source = source();
        let mut worker_source = source();
        worker_source.source_id = "worker-log".to_string();
        worker_source.path = "logs/worker.log".to_string();
        worker_source.default_route_tags = vec!["worker-log".to_string()];
        worker_source.workspace_id = Some("workspace-b".to_string());
        let project = IncidentMonitorMonitoredProject {
            log_sources: vec![api_source.clone(), worker_source.clone()],
            ..project(dir.path())
        };
        state
            .put_incident_monitor_config(IncidentMonitorConfig {
                enabled: true,
                monitored_projects: vec![project.clone()],
                ..IncidentMonitorConfig::default()
            })
            .await
            .unwrap();

        let first = submit_log_candidate(
            &state,
            &project,
            &api_source,
            log_candidate(dir.path(), "api-log", "shared-fingerprint"),
        )
        .await
        .unwrap()
        .expect("first source draft");
        let second = submit_log_candidate(
            &state,
            &project,
            &worker_source,
            log_candidate(dir.path(), "worker-log", "shared-fingerprint"),
        )
        .await
        .unwrap()
        .expect("second source draft");

        assert_ne!(first.draft_id, second.draft_id);
        let drafts = state.list_incident_monitor_drafts(10).await;
        let incidents = state.list_incident_monitor_incidents(10).await;
        assert_eq!(drafts.len(), 2);
        assert_eq!(incidents.len(), 2);
        assert!(drafts
            .iter()
            .any(|draft| draft.log_source_id.as_deref() == Some("api-log")));
        assert!(drafts
            .iter()
            .any(|draft| draft.log_source_id.as_deref() == Some("worker-log")));
        assert!(incidents
            .iter()
            .any(|incident| incident.log_source_id.as_deref() == Some("api-log")));
        assert!(incidents
            .iter()
            .any(|incident| incident.log_source_id.as_deref() == Some("worker-log")));
    }

    #[tokio::test]
    async fn missing_end_source_seeks_to_end_when_file_first_appears() {
        let dir = tempdir().unwrap();
        let state_dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir_all(&logs).await.unwrap();
        let log_path = logs.join("app.log");

        let state = test_state(state_dir.path());
        let project = project(dir.path());
        state
            .put_incident_monitor_config(IncidentMonitorConfig {
                enabled: true,
                monitored_projects: vec![project.clone()],
                ..IncidentMonitorConfig::default()
            })
            .await
            .unwrap();

        let missing = poll_log_source_once(&state, &project, &source(), 1_000)
            .await
            .unwrap();
        assert!(!missing.healthy);
        assert_eq!(missing.offset, 0);
        assert_eq!(missing.total_submitted, 0);

        let history = "ERROR boot failed\n    at boot src/main.ts:1:1\n";
        fs::write(&log_path, history).await.unwrap();
        let created = poll_log_source_once(&state, &project, &source(), 2_000)
            .await
            .unwrap();
        assert!(created.healthy);
        assert_eq!(created.offset, history.len() as u64);
        assert_eq!(created.total_submitted, 0);
        assert_eq!(state.list_incident_monitor_incidents(10).await.len(), 0);

        let appended = "ERROR upload failed\n    at normalize src/uploads.ts:42:1\n";
        fs::write(&log_path, format!("{}{}", history, appended))
            .await
            .unwrap();
        let fresh = poll_log_source_once(&state, &project, &source(), 3_000)
            .await
            .unwrap();
        assert_eq!(fresh.total_submitted, 1);
        assert_eq!(state.list_incident_monitor_drafts(10).await.len(), 1);
        assert_eq!(state.list_incident_monitor_incidents(10).await.len(), 1);
    }

    #[tokio::test]
    async fn reset_offset_replays_from_beginning_for_end_source() {
        let dir = tempdir().unwrap();
        let state_dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir_all(&logs).await.unwrap();
        let log_path = logs.join("app.log");
        fs::write(
            &log_path,
            "ERROR boot failed\n    at boot src/main.ts:1:1\n",
        )
        .await
        .unwrap();

        let state = test_state(state_dir.path());
        let project = project(dir.path());
        state
            .put_incident_monitor_config(IncidentMonitorConfig {
                enabled: true,
                monitored_projects: vec![project.clone()],
                ..IncidentMonitorConfig::default()
            })
            .await
            .unwrap();

        let first = poll_log_source_once(&state, &project, &source(), 1_000)
            .await
            .unwrap();
        assert_eq!(
            first.offset,
            "ERROR boot failed\n    at boot src/main.ts:1:1\n".len() as u64
        );
        assert_eq!(state.list_incident_monitor_incidents(10).await.len(), 0);

        let reset = reset_log_source_offset(&state, &project, &source(), 2_000)
            .await
            .unwrap();
        assert_eq!(reset.offset, 0);
        #[cfg(unix)]
        assert!(reset.inode.is_some());

        let second = poll_log_source_once(&state, &project, &source(), 3_000)
            .await
            .unwrap();
        assert_eq!(second.total_submitted, 1);
        assert_eq!(state.list_incident_monitor_drafts(10).await.len(), 1);
        assert_eq!(state.list_incident_monitor_incidents(10).await.len(), 1);
    }

    #[tokio::test]
    async fn replay_latest_candidate_uses_stored_log_offsets() {
        let dir = tempdir().unwrap();
        let state_dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir_all(&logs).await.unwrap();
        let log_path = logs.join("app.log");
        fs::write(
            &log_path,
            "ERROR upload failed\n    at normalize src/uploads.ts:42:1\n",
        )
        .await
        .unwrap();

        let state = test_state(state_dir.path());
        let project = project(dir.path());
        let mut replay_source = source();
        replay_source.start_position = IncidentMonitorLogStartPosition::Beginning;
        state
            .put_incident_monitor_config(IncidentMonitorConfig {
                enabled: true,
                monitored_projects: vec![IncidentMonitorMonitoredProject {
                    log_sources: vec![replay_source.clone()],
                    ..project.clone()
                }],
                ..IncidentMonitorConfig::default()
            })
            .await
            .unwrap();

        let first = poll_log_source_once(&state, &project, &replay_source, 1_000)
            .await
            .unwrap();
        assert_eq!(first.total_submitted, 1);
        let before = state.list_incident_monitor_incidents(10).await;
        assert_eq!(before.len(), 1);

        let replay = replay_latest_log_source_candidate(&state, &project, &replay_source)
            .await
            .unwrap()
            .expect("replayable latest candidate");
        assert_eq!(replay.incident.incident_id, before[0].incident_id);
        assert!(replay.draft.is_some());
        let after = state.list_incident_monitor_incidents(10).await;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].occurrence_count, 2);
    }
}
