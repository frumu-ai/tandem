// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::{collections::HashMap, time::Duration};

use serde_json::Value;
use tandem_types::{EngineEvent, MessagePartInput, SendMessageRequest, Session, TenantContext};

use crate::app::state::{
    derive_status_index_update, extract_persistable_tool_part, truncate_text, AppState,
};
use crate::http::context_runs::{
    append_context_run_event, ensure_session_context_run, session_run_status_to_context,
};
use crate::http::context_types::{ContextRunEventAppendInput, ContextRunStatus};
use crate::incident_monitor::types::{IncidentMonitorConfig, IncidentMonitorIncidentRecord};
use crate::routines::types::{RoutineHistoryEvent, RoutineRunRecord, RoutineRunStatus};
use crate::runtime_event_log::{
    append_runtime_event_log_row, prune_runtime_event_log, RuntimeEventLogRow,
};
use crate::stateful_runtime::{
    compact_stateful_run_event_log, prune_stateful_reliability_store, prune_stateful_run_snapshots,
    prune_stateful_wait_store, stateful_reliability_path_from_runtime_events_path,
    StatefulRuntimeStoragePaths,
};
use crate::util::time::now_ms;

async fn wait_for_runtime_ready_or_exit(state: &AppState, component: &str) -> bool {
    if state.wait_until_ready_or_failed(120, 250).await {
        return true;
    }
    let startup = state.startup_snapshot().await;
    tracing::warn!(
        component,
        startup_status = ?startup.status,
        startup_phase = %startup.phase,
        attempt_id = %startup.attempt_id,
        "background task exiting before runtime access because startup did not become ready"
    );
    false
}

async fn wait_for_runtime_installed_or_exit(state: &AppState, component: &str) -> bool {
    for _ in 0..120 {
        if state.runtime.get().is_some() {
            return true;
        }
        let startup = state.startup_snapshot().await;
        if matches!(startup.status, crate::app::startup::StartupStatus::Failed) {
            tracing::warn!(
                component,
                startup_status = ?startup.status,
                startup_phase = %startup.phase,
                attempt_id = %startup.attempt_id,
                "background task exiting before runtime access because startup failed before runtime installed"
            );
            return false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let startup = state.startup_snapshot().await;
    tracing::warn!(
        component,
        startup_status = ?startup.status,
        startup_phase = %startup.phase,
        attempt_id = %startup.attempt_id,
        "background task exiting before runtime access because runtime was not installed"
    );
    false
}

fn extract_event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .or_else(|| properties.get("sessionId"))
        .or_else(|| properties.get("id"))
        .or_else(|| {
            properties
                .get("record")
                .and_then(|record| record.get("session_id"))
        })
        .or_else(|| {
            properties
                .get("part")
                .and_then(|part| part.get("sessionID"))
        })
        .or_else(|| {
            properties
                .get("part")
                .and_then(|part| part.get("sessionId"))
        })
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_event_correlation_id(properties: &Value) -> Option<String> {
    properties
        .get("correlationID")
        .or_else(|| properties.get("correlationId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

const CONTEXT_TOOL_EVENT_STRING_LIMIT: usize = 2_000;

#[derive(Debug, Default)]
struct RuntimeEventLogContextCache {
    sessions: HashMap<String, RuntimeEventLogSessionContext>,
}

#[derive(Debug, Clone, Default)]
struct RuntimeEventLogSessionContext {
    run_id: Option<String>,
    tenant_context: Option<TenantContext>,
}

fn is_running_tool_args_delta(properties: &Value) -> bool {
    let Some(part) = properties.get("part") else {
        return false;
    };
    let part_type = part
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if !matches!(
        part_type,
        "tool" | "tool-invocation" | "tool-result" | "tool_invocation" | "tool_result"
    ) {
        return false;
    }
    let tool_state = part
        .get("state")
        .and_then(|value| value.as_str())
        .unwrap_or("running");
    if matches!(tool_state, "completed" | "failed") {
        return false;
    }
    properties
        .get("toolCallDelta")
        .and_then(|delta| delta.get("argsDelta").or_else(|| delta.get("args_delta")))
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty())
}

fn compact_large_context_event_strings(value: &mut Value) {
    match value {
        Value::String(text) if text.len() > CONTEXT_TOOL_EVENT_STRING_LIMIT => {
            *text = truncate_text(text, CONTEXT_TOOL_EVENT_STRING_LIMIT);
        }
        Value::Array(items) => {
            for item in items {
                compact_large_context_event_strings(item);
            }
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                compact_large_context_event_strings(child);
            }
        }
        _ => {}
    }
}

fn compact_context_tool_value(value: Option<&Value>) -> Value {
    let mut compacted = value.cloned().unwrap_or(Value::Null);
    compact_large_context_event_strings(&mut compacted);
    compacted
}

async fn enrich_runtime_event_log_row(
    state: &AppState,
    row: RuntimeEventLogRow,
    context_cache: &mut RuntimeEventLogContextCache,
) -> RuntimeEventLogRow {
    let Some(session_id) = row.session_id().map(str::to_string) else {
        return row;
    };

    if row.run_id().is_none() {
        if let Some(active_run) = state.run_registry.get(&session_id).await {
            context_cache
                .sessions
                .entry(session_id.clone())
                .or_default()
                .run_id = Some(active_run.run_id);
        }
    }

    let session = if row.tenant_context().is_none() {
        state.storage.get_session(&session_id).await
    } else {
        None
    };

    enrich_runtime_event_log_row_from_session(row, session.as_ref(), context_cache)
}

fn enrich_runtime_event_log_row_from_session(
    mut row: RuntimeEventLogRow,
    session: Option<&Session>,
    context_cache: &mut RuntimeEventLogContextCache,
) -> RuntimeEventLogRow {
    let Some(session_id) = row.session_id().map(str::to_string) else {
        return row;
    };

    let run_id = row.run_id().map(str::to_string);
    let tenant_context = row
        .tenant_context()
        .cloned()
        .or_else(|| session.map(|session| session.tenant_context.clone()));

    if run_id.is_some() || tenant_context.is_some() {
        let context = context_cache
            .sessions
            .entry(session_id.clone())
            .or_default();
        if let Some(run_id) = run_id {
            context.run_id = Some(run_id);
        }
        if let Some(tenant_context) = tenant_context {
            context.tenant_context = Some(tenant_context);
        }
    }

    if let Some(context) = context_cache.sessions.get(&session_id) {
        if row.event.envelope.run_id.is_none() {
            row.event.envelope.run_id = context.run_id.clone();
        }
        if row.event.envelope.tenant_context.is_none() {
            row.event.envelope.tenant_context = context.tenant_context.clone();
        }
    }

    row
}

async fn apply_provider_usage_to_routine_run(
    state: &AppState,
    run_id: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
) {
    let rate = state.token_cost_per_1k_usd.max(0.0);
    let delta_cost = (total_tokens as f64 / 1000.0) * rate;
    let mut guard = state.routine_runs.write().await;
    if let Some(run) = guard.get_mut(run_id) {
        run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
        run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
        run.total_tokens = run.total_tokens.saturating_add(total_tokens);
        run.estimated_cost_usd += delta_cost;
        run.updated_at_ms = now_ms();
    }
    drop(guard);
    let _ = state.persist_routine_runs().await;
}

async fn apply_provider_usage_to_automation_v2_run(
    state: &AppState,
    run_id: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
) {
    let rate = state.token_cost_per_1k_usd.max(0.0);
    let delta_cost = (total_tokens as f64 / 1000.0) * rate;
    let mut guard = state.automation_v2_runs.write().await;
    if let Some(run) = guard.get_mut(run_id) {
        run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
        run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
        run.total_tokens = run.total_tokens.saturating_add(total_tokens);
        run.estimated_cost_usd += delta_cost;
        run.updated_at_ms = now_ms();
    }
    drop(guard);
    let _ = state.persist_automation_v2_runs().await;
    let _ = state
        .record_automation_v2_spend(
            run_id,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            delta_cost,
        )
        .await;
}

fn event_tenant_context_value(event: &EngineEvent) -> Value {
    event
        .properties
        .get("tenantContext")
        .cloned()
        .unwrap_or(Value::Null)
}

#[path = "tasks_context_run.rs"]
mod tasks_context_run;
use tasks_context_run::*;

#[cfg(test)]
#[path = "tasks_core_tests.rs"]
mod tests;

pub async fn run_session_part_persister(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("session part persister: skipped because runtime did not become ready");
        return;
    }
    let Some(mut rx) = state.event_bus.take_session_part_receiver() else {
        tracing::warn!("session part persister: skipped because receiver was already taken");
        return;
    };
    while let Some(event) = rx.recv().await {
        if event.event_type != "message.part.updated" {
            continue;
        }
        let Some(session_id) = extract_event_session_id(&event.properties) else {
            continue;
        };
        let Some((message_id, part)) = extract_persistable_tool_part(&event.properties) else {
            continue;
        };
        if let Err(error) = state
            .storage
            .append_message_part(&session_id, &message_id, part)
            .await
        {
            tracing::warn!(
                "session part persister failed for session={} message={}: {error:#}",
                session_id,
                message_id
            );
        }
    }
}

pub async fn run_status_indexer(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("status indexer: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Some(update) = derive_status_index_update(&event) {
                    if let Err(error) = state
                        .put_shared_resource(
                            update.key,
                            update.value,
                            None,
                            "system.status_indexer".to_string(),
                            None,
                        )
                        .await
                    {
                        tracing::warn!("status indexer failed to persist update: {error:?}");
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_session_context_run_journaler(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!(
            "session context run journaler: skipped because runtime did not become ready"
        );
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                let Some(session_id) = extract_event_session_id(&event.properties) else {
                    continue;
                };
                let Some(input) = session_context_run_event_input(&event) else {
                    continue;
                };
                let Some(session) = state.storage.get_session(&session_id).await else {
                    continue;
                };
                let Ok(run_id) = ensure_session_context_run(&state, &session).await else {
                    tracing::warn!(
                        "session context run journaler could not ensure context run for session={session_id}"
                    );
                    continue;
                };
                if let Err(error) = append_context_run_event(&state, &run_id, input).await {
                    tracing::warn!(
                        "session context run journaler failed for session={} run={}: {:?}",
                        session_id,
                        run_id,
                        error
                    );
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_runtime_event_log_persister(state: AppState) {
    run_runtime_event_log_persister_with_registration_signal(state, None).await;
}

/// One retention pass over every stateful-runtime store: runtime event log,
/// stateful event compaction (snapshot-aware), snapshot pruning, terminal
/// waits, settled reliability records, and a WAL checkpoint so reclaimed
/// space returns to the filesystem.
async fn run_stateful_retention_sweep(state: &AppState, retention_ms: u64, retention_days: u64) {
    match prune_runtime_event_log(&state.runtime_events_path, retention_ms, now_ms()).await {
        Ok(pruned) if pruned > 0 => {
            tracing::info!(
                pruned,
                retention_days,
                "runtime event log persister pruned stale events"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "runtime event log persister could not prune stale events"
            );
        }
    }

    let stateful_paths =
        StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    match compact_stateful_run_event_log(&stateful_paths.run_events_path, retention_ms, now_ms())
        .await
    {
        Ok(pruned) if pruned > 0 => {
            tracing::info!(
                pruned,
                retention_days,
                "runtime event log persister compacted stale stateful events"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "runtime event log persister could not compact stale stateful events"
            );
        }
    }

    let snapshot_keep_last = std::env::var("TANDEM_RUNTIME_SNAPSHOT_KEEP_LAST")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(3);
    match prune_stateful_run_snapshots(
        &stateful_paths.snapshots_root,
        retention_ms,
        snapshot_keep_last,
        now_ms(),
    )
    .await
    {
        Ok(pruned) if pruned > 0 => {
            tracing::info!(
                pruned,
                retention_days,
                snapshot_keep_last,
                "runtime event log persister pruned stale stateful snapshots"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "runtime event log persister could not prune stale stateful snapshots"
            );
        }
    }

    match prune_stateful_wait_store(&stateful_paths.waits_path, retention_ms, now_ms()).await {
        Ok(pruned) if pruned > 0 => {
            tracing::info!(
                pruned,
                retention_days,
                "runtime event log persister pruned stale stateful waits"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "runtime event log persister could not prune stale stateful waits"
            );
        }
    }

    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    if let Err(error) =
        prune_stateful_reliability_store(&reliability_path, retention_ms, now_ms()).await
    {
        tracing::warn!(
            error = %error,
            "runtime event log persister could not prune settled stateful reliability records"
        );
    }

    match crate::stateful_runtime::compatibility::retire_stateful_runtime_sidecars(
        &stateful_paths,
        &reliability_path,
    )
    .await
    {
        Ok(retired) if retired > 0 => {
            tracing::info!(
                retired,
                "retention sweep retired stateful compatibility sidecars"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "retention sweep could not retire stateful compatibility sidecars"
            );
        }
    }

    let automation_runs_path = state.automation_v2_runs_path.clone();
    let checkpoint = tokio::task::spawn_blocking(move || {
        crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
            &automation_runs_path,
        )?
        .checkpoint_wal()
    })
    .await;
    match checkpoint {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error,
                "runtime event log persister could not checkpoint the stateful store WAL"
            );
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "stateful store WAL checkpoint task failed"
            );
        }
    }
}

async fn run_runtime_event_log_persister_with_registration_signal(
    state: AppState,
    registered: Option<tokio::sync::oneshot::Sender<()>>,
) {
    // Register the queue as soon as RuntimeState exists so ready-gated
    // publishers cannot race ahead and drop early runtime events.
    if !wait_for_runtime_installed_or_exit(&state, "runtime_event_log_persister").await {
        return;
    }

    let Some(mut rx) = state.event_bus.register_runtime_event_log_receiver() else {
        tracing::warn!("runtime event log persister: skipped because queue was already registered");
        return;
    };
    if let Some(registered) = registered {
        let _ = registered.send(());
    }

    let retention_days = std::env::var("TANDEM_RUNTIME_EVENT_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(30);
    if retention_days > 0 {
        let retention_ms = retention_days.saturating_mul(24 * 60 * 60 * 1_000);
        run_stateful_retention_sweep(&state, retention_ms, retention_days).await;
        // Long-lived engines re-prune on an interval; a boot-only sweep lets
        // the store grow without bound under continuous workloads.
        let sweep_hours = std::env::var("TANDEM_RUNTIME_RETENTION_SWEEP_HOURS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|hours| *hours > 0)
            .unwrap_or(6);
        let sweep_state = state.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(sweep_hours.saturating_mul(3600)));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await; // the boot sweep above covers the first tick
            loop {
                interval.tick().await;
                run_stateful_retention_sweep(&sweep_state, retention_ms, retention_days).await;
            }
        });
    }

    let mut context_cache = RuntimeEventLogContextCache::default();
    while let Some(event) = rx.recv().await {
        tandem_observability::record_engine_event_metrics(&event.event_type, &event.properties);
        let Some(row) = RuntimeEventLogRow::from_engine_event(&event) else {
            continue;
        };
        let row = enrich_runtime_event_log_row(&state, row, &mut context_cache).await;
        if let Err(error) = append_runtime_event_log_row(&state.runtime_events_path, &row).await {
            tracing::warn!(
                error = %error,
                event_id = row.event_id(),
                seq = row.seq(),
                "runtime event log persister failed to append event"
            );
        }
    }
}

pub async fn run_automation_webhook_retention_reaper(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "automation_webhook_retention_reaper").await {
        return;
    }
    loop {
        if state.is_automation_scheduler_stopping() {
            return;
        }
        match state.prune_automation_webhook_retention(now_ms()).await {
            Ok(report)
                if report.pruned_events > 0
                    || report.pruned_payloads > 0
                    || report.pruned_deliveries > 0 =>
            {
                tracing::info!(
                    pruned_events = report.pruned_events,
                    pruned_payloads = report.pruned_payloads,
                    pruned_deliveries = report.pruned_deliveries,
                    "automation webhook retention reaper pruned expired records"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "automation webhook retention reaper could not prune expired records"
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(60 * 60)).await;
    }
}

#[cfg(test)]
mod runtime_event_log_persister_tests {
    use serde_json::json;
    use tandem_types::EngineEvent;

    use super::*;

    async fn wait_for_persisted_event(
        path: &std::path::Path,
        tenant: &TenantContext,
        run_id: &str,
    ) -> Vec<RuntimeEventLogRow> {
        for _ in 0..50 {
            let rows = crate::runtime_event_log::query_runtime_event_log(
                path,
                tenant,
                crate::runtime_event_log::RuntimeEventLogQuery {
                    run_id,
                    after_seq: None,
                    limit: None,
                },
            );
            if !rows.is_empty() {
                return rows;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Vec::new()
    }

    #[tokio::test]
    async fn persister_flushes_events_published_after_queue_registration() {
        let mut state = crate::test_support::test_state().await;
        state.runtime_events_path = std::env::temp_dir().join(format!(
            "runtime-events-prestart-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        {
            let mut startup = state.startup.write().await;
            startup.status = crate::app::startup::StartupStatus::Starting;
            startup.phase = "loading-fixtures".to_string();
        }
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");

        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel();
        let persister = tokio::spawn(run_runtime_event_log_persister_with_registration_signal(
            state.clone(),
            Some(registered_tx),
        ));
        registered_rx
            .await
            .expect("persister should remain active through queue registration");
        assert!(
            state.event_bus.runtime_event_log_queue_is_registered(),
            "persister should register its queue before consuming runtime events"
        );

        state.event_bus.publish(EngineEvent::new(
            "session.run.started",
            json!({
                "sessionID": "session-a",
                "runID": "run-a",
                "tenantContext": tenant.clone(),
            }),
        ));
        {
            let mut startup = state.startup.write().await;
            startup.status = crate::app::startup::StartupStatus::Ready;
            startup.phase = "ready".to_string();
        }

        let rows = wait_for_persisted_event(&state.runtime_events_path, &tenant, "run-a").await;

        persister.abort();
        let _ = tokio::fs::remove_file(&state.runtime_events_path).await;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id(), Some("run-a"));
        assert_eq!(rows[0].session_id(), Some("session-a"));
        assert_eq!(rows[0].event.event_type.as_str(), "session.run.started");
    }
}

pub async fn run_agent_team_supervisor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("agent team supervisor: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                state.agent_teams.handle_engine_event(&state, &event).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

fn is_incident_monitor_candidate_event(event: &EngineEvent) -> bool {
    if event.event_type.starts_with("incident_monitor.") {
        return false;
    }
    if is_automation_v2_context_mirror_failure(event) {
        return false;
    }
    matches!(
        event.event_type.as_str(),
        "context.task.failed"
            | "context.task.blocked"
            | "context.run.failed"
            | "workflow.run.failed"
            | "workflow.validation.failed"
            | "routine.run.failed"
            | "session.error"
            | "automation.run.failed"
            | "automation_v2.run.failed"
            | "automation_v2.run.paused_stale_no_provider_activity"
            | "coder.run.failed"
    )
}

fn event_string_property<'a>(event: &'a EngineEvent, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| event.properties.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_automation_v2_context_mirror_failure(event: &EngineEvent) -> bool {
    if !matches!(
        event.event_type.as_str(),
        "context.task.failed" | "context.task.blocked" | "context.run.failed"
    ) {
        return false;
    }

    if event_string_property(event, &["source"]).is_some_and(|source| source == "automation_v2") {
        return true;
    }
    if event_string_property(event, &["automation_id", "automationID"]).is_some() {
        return true;
    }
    event_string_property(event, &["run_id", "runID"]).is_some_and(|run_id| {
        run_id.starts_with("automation-v2-") || run_id.starts_with("automation_v2-")
    })
}

pub async fn run_incident_monitor(state: AppState) {
    let mut wait_ms = 250u64;
    loop {
        let startup = state.startup_snapshot().await;
        if matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            break;
        }
        if matches!(startup.status, crate::app::startup::StartupStatus::Failed) {
            tracing::warn!(
                startup_status = ?startup.status,
                startup_phase = %startup.phase,
                attempt_id = %startup.attempt_id,
                "incident monitor: exiting because startup failed before monitoring began"
            );
            return;
        }

        state
            .update_incident_monitor_runtime_status(|runtime| {
                runtime.monitoring_active = false;
                runtime.last_runtime_error = Some(
                    "Waiting for runtime readiness before starting incident monitor".to_string(),
                );
            })
            .await;

        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        wait_ms = (wait_ms * 2).min(2_000);
    }

    state
        .update_incident_monitor_runtime_status(|runtime| {
            runtime.monitoring_active = false;
            runtime.last_runtime_error = None;
        })
        .await;
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if !is_incident_monitor_candidate_event(&event) {
                    continue;
                }
                let status = state.incident_monitor_status().await;
                if !status.config.enabled || status.config.paused || !status.readiness.repo_valid {
                    state
                        .update_incident_monitor_runtime_status(|runtime| {
                            runtime.monitoring_active = status.config.enabled
                                && !status.config.paused
                                && status.readiness.repo_valid;
                            runtime.paused = status.config.paused;
                            runtime.last_runtime_error = status.last_error.clone();
                        })
                        .await;
                    continue;
                }
                match crate::incident_monitor::service::process_event(
                    &state,
                    &event,
                    &status.config,
                )
                .await
                {
                    Ok(incident) => {
                        state
                            .update_incident_monitor_runtime_status(|runtime| {
                                runtime.monitoring_active = true;
                                runtime.paused = status.config.paused;
                                runtime.last_processed_at_ms = Some(now_ms());
                                runtime.last_incident_event_type =
                                    Some(incident.event_type.clone());
                                runtime.last_runtime_error = None;
                            })
                            .await;
                    }
                    Err(error) => {
                        let detail = truncate_text(&error.to_string(), 500);
                        state
                            .update_incident_monitor_runtime_status(|runtime| {
                                runtime.monitoring_active = true;
                                runtime.paused = status.config.paused;
                                runtime.last_processed_at_ms = Some(now_ms());
                                runtime.last_incident_event_type = Some(event.event_type.clone());
                                runtime.last_runtime_error = Some(detail.clone());
                            })
                            .await;
                        state.event_bus.publish(EngineEvent::new(
                            "incident_monitor.error",
                            serde_json::json!({
                                "eventType": event.event_type,
                                "detail": detail,
                            }),
                        ));
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                state
                    .update_incident_monitor_runtime_status(|runtime| {
                        runtime.last_runtime_error = Some(format!(
                            "Incident monitor lagged and dropped {count} events."
                        ));
                    })
                    .await;
            }
        }
    }
}

pub(crate) async fn publish_incident_monitor_recovery_draft(
    state: &AppState,
    draft_id: String,
    incident_id: Option<String>,
) -> anyhow::Result<crate::incident_monitor_github::PublishOutcome> {
    crate::incident_monitor::router::publish_draft(
        state,
        crate::incident_monitor::router::IncidentMonitorPublishRequest {
            draft_id,
            incident_id,
            mode: crate::incident_monitor_github::PublishMode::Recovery,
            destination_ids: Vec::new(),
        },
    )
    .await
}

/// Periodic deadline sweep for Incident Monitor triage runs. Without this
/// loop, `recover_overdue_incident_monitor_triage_runs` only fires when
/// something polls `incident_monitor_status` (the status panel, the
/// dashboard, an API caller). On a quiet engine — UI closed, no
/// outside pollers — an overdue triage just sits there: issue #60
/// ran 3.95 hours past its 30-minute deadline before anything noticed.
/// A 30-second tick guarantees the timeout fires within ~30s of the
/// deadline regardless of UI state.
///
/// Concurrency note: `try_mark_triage_timed_out` (called inside
/// `recover_overdue_incident_monitor_triage_runs`) is an atomic CAS, so
/// running this loop alongside an on-demand `incident_monitor_status` call
/// can't double-publish — only one caller wins the status flip per
/// draft.
pub async fn run_incident_monitor_recovery_sweep(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "run_incident_monitor_recovery_sweep").await {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let status = state.incident_monitor_status_snapshot().await;
        if !status.config.enabled || status.config.paused {
            continue;
        }
        // TAN-556: enforce the retention window (receipts / incidents / evidence
        // artifacts) on the same sweep so old data doesn't accumulate unbounded.
        if let Some(retention_days) = status
            .config
            .safety_defaults
            .retention_days
            .filter(|days| *days > 0)
        {
            match state.prune_incident_monitor_retention(retention_days).await {
                Ok((posts, incidents, artifacts)) if posts + incidents + artifacts > 0 => {
                    tracing::info!(
                        posts,
                        incidents,
                        artifacts,
                        retention_days,
                        "incident monitor retention sweep pruned stale data"
                    );
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    error = %error,
                    "incident monitor retention sweep failed to prune stale data"
                ),
            }
        }
        let recovered =
            match crate::incident_monitor::service::recover_overdue_incident_monitor_triage_runs(
                &state,
            )
            .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "incident monitor recovery sweep: recover_overdue failed"
                    );
                    continue;
                }
            };
        for (draft_id, incident_id) in recovered {
            if let Err(error) =
                publish_incident_monitor_recovery_draft(&state, draft_id.clone(), incident_id).await
            {
                tracing::warn!(
                    draft_id = %draft_id,
                    error = %error,
                    "incident monitor recovery sweep: publish_draft failed"
                );
            }
        }
    }
}

pub async fn run_usage_aggregator(state: AppState) {
    if crate::benchmarking::benchmark_config_from_env().profiling_enabled {
        tokio::spawn(crate::benchmarking::run_benchmark_profiler(state.clone()));
    }
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("usage aggregator: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if event.event_type != "provider.usage" {
                    continue;
                }
                let prompt_tokens = event
                    .properties
                    .get("promptTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let completion_tokens = event
                    .properties
                    .get("completionTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let total_tokens = event
                    .properties
                    .get("totalTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
                if let Some(correlation_id) = extract_event_correlation_id(&event.properties) {
                    if let Some(run_id) = correlation_id.strip_prefix("routine:") {
                        apply_provider_usage_to_routine_run(
                            &state,
                            run_id,
                            prompt_tokens,
                            completion_tokens,
                            total_tokens,
                        )
                        .await;
                        continue;
                    }
                    if let Some(run_id) = correlation_id.strip_prefix("automation-v2:") {
                        apply_provider_usage_to_automation_v2_run(
                            &state,
                            run_id,
                            prompt_tokens,
                            completion_tokens,
                            total_tokens,
                        )
                        .await;
                        continue;
                    }
                }
                let session_id = event
                    .properties
                    .get("sessionID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if session_id.is_empty() {
                    continue;
                }
                state
                    .apply_provider_usage_to_runs(
                        session_id,
                        prompt_tokens,
                        completion_tokens,
                        total_tokens,
                    )
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_routine_scheduler(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "routine_scheduler").await {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = now_ms();
        let plans = state.evaluate_routine_misfires(now).await;
        for plan in plans {
            let Some(routine) = state.get_routine_by_identity(&plan.identity).await else {
                continue;
            };
            match crate::app::state::evaluate_routine_execution_policy(&routine, "scheduled") {
                crate::app::state::RoutineExecutionDecision::Allowed => {
                    let _ = state
                        .mark_routine_fired_by_identity(&plan.identity, now)
                        .await;
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::Queued,
                            None,
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.identity.routine_id.clone(),
                            tenant_context: plan.tenant_context.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "queued".to_string(),
                            detail: None,
                        })
                        .await;
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.fired",
                            &plan.tenant_context,
                            serde_json::json!({
                                "routineID": plan.identity.routine_id,
                                "runID": run.run_id,
                                "runCount": plan.run_count,
                                "scheduledAtMs": plan.scheduled_at_ms,
                                "nextFireAtMs": plan.next_fire_at_ms,
                            }),
                        ));
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.run.created",
                            &plan.tenant_context,
                            serde_json::json!({
                                "run": run,
                            }),
                        ));
                }
                crate::app::state::RoutineExecutionDecision::RequiresApproval { reason } => {
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::PendingApproval,
                            Some(reason.clone()),
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.identity.routine_id.clone(),
                            tenant_context: plan.tenant_context.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "pending_approval".to_string(),
                            detail: Some(reason.clone()),
                        })
                        .await;
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.approval_required",
                            &plan.tenant_context,
                            serde_json::json!({
                                "routineID": plan.identity.routine_id,
                                "runID": run.run_id,
                                "runCount": plan.run_count,
                                "triggerType": "scheduled",
                                "reason": reason,
                            }),
                        ));
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.run.created",
                            &plan.tenant_context,
                            serde_json::json!({
                                "run": run,
                            }),
                        ));
                }
                crate::app::state::RoutineExecutionDecision::Blocked { reason } => {
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::BlockedPolicy,
                            Some(reason.clone()),
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.identity.routine_id.clone(),
                            tenant_context: plan.tenant_context.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "blocked_policy".to_string(),
                            detail: Some(reason.clone()),
                        })
                        .await;
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.blocked",
                            &plan.tenant_context,
                            serde_json::json!({
                                "routineID": plan.identity.routine_id,
                                "runID": run.run_id,
                                "runCount": plan.run_count,
                                "triggerType": "scheduled",
                                "reason": reason,
                            }),
                        ));
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.run.created",
                            &plan.tenant_context,
                            serde_json::json!({
                                "run": run,
                            }),
                        ));
                }
            }
        }
    }
}

pub async fn run_routine_executor(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "routine_executor").await {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(run) = state.claim_next_queued_routine_run().await else {
            continue;
        };

        state
            .event_bus
            .publish(crate::routines::types::tenant_scoped_engine_event(
                "routine.run.started",
                &run.tenant_context,
                serde_json::json!({
                    "runID": run.run_id,
                    "routineID": run.routine_id,
                    "triggerType": run.trigger_type,
                    "startedAtMs": now_ms(),
                }),
            ));

        let workspace_root = state.workspace_index.snapshot().await.root;
        let session = routine_execution_session(&run, workspace_root);
        let session_id = session.id.clone();
        let tenant_context = run.tenant_context.clone();

        if let Err(error) = state.storage.save_session(session).await {
            let detail = format!("failed to create routine session: {error}");
            let _ = state
                .update_routine_run_status(
                    &run.run_id,
                    RoutineRunStatus::Failed,
                    Some(detail.clone()),
                )
                .await;
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.run.failed",
                    &run.tenant_context,
                    serde_json::json!({
                        "runID": run.run_id,
                        "routineID": run.routine_id,
                        "reason": detail,
                    }),
                ));
            continue;
        }

        state
            .set_routine_session_policy(
                session_id.clone(),
                run.run_id.clone(),
                run.routine_id.clone(),
                run.tenant_context.clone(),
                run.allowed_tools.clone(),
            )
            .await;
        state
            .add_active_session_id(&run.run_id, session_id.clone())
            .await;
        state
            .engine_loop
            .set_session_allowed_tools(&session_id, run.allowed_tools.clone())
            .await;
        state
            .engine_loop
            .set_session_auto_approve_permissions(&session_id, true)
            .await;

        let (selected_model, model_source) =
            crate::app::routines::resolve_routine_model_spec_for_run(&state, &run).await;
        if let Some(spec) = selected_model.as_ref() {
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.run.model_selected",
                    &run.tenant_context,
                    serde_json::json!({
                        "runID": run.run_id,
                        "routineID": run.routine_id,
                        "providerID": spec.provider_id,
                        "modelID": spec.model_id,
                        "source": model_source,
                    }),
                ));
        }

        let request = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: crate::app::routines::build_routine_prompt(&state, &run).await,
            }],
            model: selected_model,
            agent: None,
            tool_mode: None,
            tool_allowlist: None,
            strict_kb_grounding: None,
            context_mode: None,
            write_required: None,
            prewrite_requirements: None,
            sampling: Default::default(),
        };

        let execution_surface = if run.trigger_type == "scheduled" {
            crate::http::session_run_retry::PromptExecutionSurface::Scheduled
        } else {
            crate::http::session_run_retry::PromptExecutionSurface::Routine
        };
        let run_result = crate::http::session_run_retry::run_prompt_with_auth_recovery(
            &state,
            &session_id,
            &run.run_id,
            execution_surface,
            request,
            Some(format!("routine:{}", run.run_id)),
            &tenant_context,
        )
        .await;

        state.clear_routine_session_policy(&session_id).await;
        state
            .clear_active_session_id(&run.run_id, &session_id)
            .await;
        state
            .engine_loop
            .clear_session_allowed_tools(&session_id)
            .await;
        state
            .engine_loop
            .clear_session_auto_approve_permissions(&session_id)
            .await;

        match run_result {
            Ok(()) => {
                crate::app::routines::append_configured_output_artifacts(&state, &run).await;
                let _ = state
                    .update_routine_run_status(
                        &run.run_id,
                        RoutineRunStatus::Completed,
                        Some("routine run completed".to_string()),
                    )
                    .await;
                state
                    .event_bus
                    .publish(crate::routines::types::tenant_scoped_engine_event(
                        "routine.run.completed",
                        &run.tenant_context,
                        serde_json::json!({
                            "runID": run.run_id,
                            "routineID": run.routine_id,
                            "sessionID": session_id,
                            "finishedAtMs": now_ms(),
                        }),
                    ));
            }
            Err(error) => {
                if let Some(latest) = state.get_routine_run(&run.run_id).await {
                    if latest.status == RoutineRunStatus::Paused {
                        state.event_bus.publish(
                            crate::routines::types::tenant_scoped_engine_event(
                                "routine.run.paused",
                                &run.tenant_context,
                                serde_json::json!({
                                    "runID": run.run_id,
                                    "routineID": run.routine_id,
                                    "sessionID": session_id,
                                    "finishedAtMs": now_ms(),
                                }),
                            ),
                        );
                        continue;
                    }
                }
                let detail = truncate_text(&error.to_string(), 500);
                let _ = state
                    .update_routine_run_status(
                        &run.run_id,
                        RoutineRunStatus::Failed,
                        Some(detail.clone()),
                    )
                    .await;
                state
                    .event_bus
                    .publish(crate::routines::types::tenant_scoped_engine_event(
                        "routine.run.failed",
                        &run.tenant_context,
                        serde_json::json!({
                            "runID": run.run_id,
                            "routineID": run.routine_id,
                            "sessionID": session_id,
                            "reason": detail,
                            "finishedAtMs": now_ms(),
                        }),
                    ));
            }
        }
    }
}

pub(crate) fn routine_execution_session(run: &RoutineRunRecord, workspace_root: String) -> Session {
    let mut session = Session::new(
        Some(format!("Routine {}", run.routine_id)),
        Some(workspace_root.clone()),
    );
    session.workspace_root = Some(workspace_root);
    session.tenant_context = run.tenant_context.clone();
    session
}

pub async fn run_automation_v2_scheduler(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if state.is_automation_scheduler_stopping() {
            break;
        }
        let startup = state.startup_snapshot().await;
        if !matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            continue;
        }
        let tick_started = std::time::Instant::now();
        let now = now_ms();

        // --- Existing: timer-based misfires ---
        let due = state.evaluate_automation_v2_misfires(now).await;
        for automation_id in due {
            let Some(automation) = state.get_automation_v2(&automation_id).await else {
                continue;
            };
            if let Ok(run) = state
                .create_automation_v2_run(&automation, "scheduled")
                .await
            {
                let tenant_context = run.tenant_context.clone();
                state
                    .event_bus
                    .publish(crate::routines::types::tenant_scoped_engine_event(
                        "automation.v2.run.created",
                        &tenant_context,
                        serde_json::json!({
                            "automationID": automation_id,
                            "run": run,
                            "tenantContext": tenant_context,
                            "triggerType": "scheduled",
                        }),
                    ));
            }
        }

        // --- New (Phase 1): watch-condition-based triggers ---
        let watch_due = state.evaluate_automation_v2_watches().await;
        for (automation_id, trigger_reason, maybe_handoff) in watch_due {
            let Some(automation) = state.get_automation_v2(&automation_id).await else {
                continue;
            };

            // If this watch was triggered by a handoff, consume it before creating
            // the run so no other automation on this tick can claim the same handoff.
            let consumed_handoff_id = if let Some(ref handoff) = maybe_handoff {
                let workspace_root = state.workspace_index.snapshot().await.root;
                let handoff_cfg = automation.effective_handoff_config();
                match state
                    .consume_automation_v2_handoff(
                        &workspace_root,
                        handoff,
                        &handoff_cfg,
                        // Use a placeholder run ID; the real run ID is assigned below.
                        // consume_automation_v2_handoff writes to the archive immediately,
                        // so we pass the handoff_id so the audit trail is useful even
                        // if run creation subsequently fails.
                        &format!("pending-{}", handoff.handoff_id),
                        &automation_id,
                    )
                    .await
                {
                    Ok(Some(_)) => Some(handoff.handoff_id.clone()),
                    Ok(None) => {
                        // Already consumed by a race — skip this trigger.
                        tracing::warn!(
                            automation_id = %automation_id,
                            handoff_id = %handoff.handoff_id,
                            "handoff watch: skipping — handoff already consumed (race)"
                        );
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!(
                            automation_id = %automation_id,
                            handoff_id = %handoff.handoff_id,
                            "handoff watch: failed to consume handoff: {err}"
                        );
                        continue;
                    }
                }
            } else {
                None
            };

            match state
                .create_automation_v2_watch_run(
                    &automation,
                    trigger_reason.clone(),
                    consumed_handoff_id,
                )
                .await
            {
                Ok(run) => {
                    let tenant_context = run.tenant_context.clone();
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "automation.v2.run.created",
                            &tenant_context,
                            serde_json::json!({
                                "automationID": automation_id,
                                "run": run,
                                "tenantContext": tenant_context,
                                "triggerType": "watch_condition",
                                "triggerReason": trigger_reason,
                            }),
                        ));
                }
                Err(err) => {
                    tracing::warn!(
                        automation_id = %automation_id,
                        "watch condition run creation failed: {err}"
                    );
                }
            }
        }
        tandem_observability::record_scheduler_tick_latency_ms(
            tick_started.elapsed().as_millis() as u64
        );
    }
}

pub async fn run_optimization_scheduler(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let startup = state.startup_snapshot().await;
        if !matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            continue;
        }
        if let Err(error) = state.reconcile_optimization_campaigns().await {
            tracing::warn!("optimization scheduler reconciliation failed: {error}");
        }
    }
}

#[cfg(test)]
#[path = "tasks_tests.rs"]
mod incident_monitor_candidate_tests;
