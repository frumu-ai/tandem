// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use futures::FutureExt;
use tokio::task::JoinSet;

use crate::app::state::automation::{record_automation_lifecycle_event, QueueReason};
use crate::app::state::AppState;
use crate::automation_v2::executor::run_automation_v2_run;
use crate::automation_v2::types::{AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord};
use crate::stateful_runtime::{process_due_stateful_waits, StatefulRuntimeStoragePaths};

const STALE_RUNNING_AUTOMATION_RUN_MS: u64 = 600_000;
const AUTOMATION_WEBHOOK_INBOX_BATCH_LIMIT: usize = 50;

pub async fn run_automation_v2_executor(state: AppState) {
    // Self-supervise: if any panic escapes, log it and respawn the inner loop
    // so queued automation runs don't get stranded forever when a single
    // deref-or-lookup panics deep in state code. Without this, one bad run
    // can kill the executor task permanently for the lifetime of the engine.
    loop {
        let state_clone = state.clone();
        let result = AssertUnwindSafe(run_automation_v2_executor_supervised(state_clone))
            .catch_unwind()
            .await;
        match result {
            Ok(()) => return,
            Err(_) => {
                tracing::error!(
                    "automation_v2_executor panicked; respawning in 1s so queued runs can be polled"
                );
                if state.is_automation_scheduler_stopping() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn run_automation_v2_executor_supervised(state: AppState) {
    // Wait for startup to reach Ready before touching runtime-backed state.
    // `recover_in_flight_runs` derefs `AppState::runtime`; if the OnceLock
    // isn't populated yet, the deref panics and kills this task permanently,
    // which leaves queued automation runs stranded with no executor polling.
    loop {
        if state.is_automation_scheduler_stopping() {
            return;
        }
        let startup = state.startup_snapshot().await;
        if state.is_ready() {
            break;
        }
        if matches!(startup.status, crate::app::startup::StartupStatus::Failed) {
            tracing::warn!("automation_v2_executor exiting: startup failed");
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    tracing::info!("automation_v2_executor: startup ready, beginning recovery");
    let _ = state.recover_in_flight_runs().await;
    tracing::info!("automation_v2_executor: recovery complete, entering main loop");

    if crate::config::env::resolve_scheduler_mode() == crate::config::env::SchedulerMode::Multi {
        run_automation_v2_executor_multi(state).await;
    } else {
        run_automation_v2_executor_single(state).await;
    }
    tracing::info!("automation_v2_executor: main loop exited");
}

async fn process_stateful_wait_scheduler_tick(state: &AppState) {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let tick =
        process_due_stateful_waits(&paths, crate::util::time::now_ms(), Default::default()).await;
    for outcome in &tick.outcomes {
        let _ = state.apply_stateful_wait_scheduler_outcome(outcome).await;
        state
            .event_bus
            .publish(crate::routines::types::tenant_scoped_engine_event(
                outcome.event_type.clone(),
                &outcome.tenant_context,
                serde_json::json!({
                    "runID": &outcome.run_id,
                    "waitID": &outcome.wait_id,
                    "tenantContext": &outcome.tenant_context,
                    "eventSeq": outcome.event_seq,
                    "waitStatus": &outcome.wait_status,
                    "runStatus": &outcome.run_status,
                    "lagMs": outcome.lag_ms,
                }),
            ));
    }
    for error in &tick.errors {
        tracing::warn!(error = %error, "stateful wait scheduler tick failed for a wait");
    }
    if tick.completed > 0 || tick.failed > 0 {
        tracing::info!(
            checked = tick.checked,
            claimed = tick.claimed,
            completed = tick.completed,
            failed = tick.failed,
            max_lag_ms = tick.max_lag_ms,
            "stateful wait scheduler tick completed"
        );
    }
}

async fn process_dead_letter_retry_tick(state: &AppState) {
    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    if acted > 0 {
        tracing::info!(
            acted,
            "stateful dead-letter retry dispatcher re-drove dead letters"
        );
    }
}

async fn process_automation_webhook_inbox_tick(state: &AppState) {
    let report = state
        .process_automation_webhook_inbox_once(AUTOMATION_WEBHOOK_INBOX_BATCH_LIMIT)
        .boxed()
        .await;
    if report.processed > 0 || report.failed > 0 {
        tracing::info!(
            checked = report.checked,
            processed = report.processed,
            failed = report.failed,
            "automation webhook inbox tick completed"
        );
    }
}

async fn process_ready_executor_maintenance(state: &AppState) {
    if !state.is_ready() {
        return;
    }
    let _ = state
        .reap_stale_running_automation_runs(STALE_RUNNING_AUTOMATION_RUN_MS)
        .await;
    if !state.is_ready() {
        return;
    }
    let _ = state.process_awaiting_approval_gate_policies().await;
    if !state.is_ready() {
        return;
    }
    let _ = state.mark_stale_awaiting_approval_runs().await;
    if !state.is_ready() {
        return;
    }
    let _ = state.auto_resume_stale_reaped_runs().await;
    if !state.is_ready() {
        return;
    }
    process_stateful_wait_scheduler_tick(state).await;
    if !state.is_ready() {
        return;
    }
    process_automation_webhook_inbox_tick(state).await;
    if !state.is_ready() {
        return;
    }
    process_dead_letter_retry_tick(state).await;
}

async fn run_automation_v2_executor_single(state: AppState) {
    let mut active = JoinSet::new();
    loop {
        while let Some(result) = active.try_join_next() {
            if let Err(error) = result {
                tracing::warn!("automation single-run supervisor task join error: {error}");
            }
        }

        if state.is_automation_scheduler_stopping() {
            if active.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }

        if !state.is_ready() {
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }

        process_ready_executor_maintenance(&state).await;

        if active.is_empty() && state.is_ready() {
            if let Some(run) = state.claim_next_queued_automation_v2_run().await {
                active.spawn(execute_run_and_release_wrapped(state.clone(), run));
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn run_automation_v2_executor_multi(state: AppState) {
    let mut active = JoinSet::new();
    loop {
        while let Some(result) = active.try_join_next() {
            if let Err(error) = result {
                tracing::warn!("automation multi-run supervisor task join error: {error}");
            }
        }

        if state.is_automation_scheduler_stopping() {
            if active.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }

        if !state.is_ready() {
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }

        process_ready_executor_maintenance(&state).await;

        let capacity = {
            let scheduler = state.automation_scheduler.read().await;
            scheduler.max_concurrent_runs
        };

        while active.len() < capacity && state.is_ready() {
            let queued = queued_runs_for_admission(&state).await;
            if queued.is_empty() {
                break;
            }

            let mut admitted_any = false;
            for run in queued {
                if active.len() >= capacity || !state.is_ready() {
                    break;
                }

                let workspace_root = queued_run_workspace_root(&state, &run).await;
                let required_providers = queued_run_required_providers(&run);
                let admission = {
                    let scheduler = state.automation_scheduler.read().await;
                    scheduler.can_admit_for_tenant(
                        &run.run_id,
                        workspace_root.as_deref(),
                        &required_providers,
                        &run.tenant_context,
                    )
                };

                match admission {
                    Ok(()) => {
                        if !state.is_ready() {
                            break;
                        }
                        if let Some(claimed) =
                            state.claim_specific_automation_v2_run(&run.run_id).await
                        {
                            let mut scheduler = state.automation_scheduler.write().await;
                            scheduler.admit_run(&run.run_id, workspace_root.as_deref());
                            active.spawn(execute_run_and_release_wrapped(state.clone(), claimed));
                            admitted_any = true;
                        }
                    }
                    Err(meta) => {
                        persist_queue_metadata_if_changed(&state, &run, meta).await;
                    }
                }
            }

            if !admitted_any {
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

async fn queued_runs_for_admission(state: &AppState) -> Vec<AutomationV2RunRecord> {
    let now = crate::util::time::now_ms();
    let mut queued = state
        .automation_v2_runs
        .read()
        .await
        .values()
        .filter(|run| run.status == AutomationRunStatus::Queued)
        .filter(|run| crate::automation_v2::retry_backoff_queue::retry_backoff_is_due(run, now))
        .cloned()
        .collect::<Vec<AutomationV2RunRecord>>();
    queued.sort_by(|a, b| {
        let a_priority = matches!(
            a.scheduler
                .as_ref()
                .and_then(|meta| meta.queue_reason.as_ref()),
            Some(QueueReason::WorkspaceLock)
        );
        let b_priority = matches!(
            b.scheduler
                .as_ref()
                .and_then(|meta| meta.queue_reason.as_ref()),
            Some(QueueReason::WorkspaceLock)
        );
        b_priority
            .cmp(&a_priority)
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
    });
    queued
}

async fn queued_run_workspace_root(
    state: &AppState,
    run: &AutomationV2RunRecord,
) -> Option<String> {
    if let Some(root) = run
        .automation_snapshot
        .as_ref()
        .and_then(|automation| automation.workspace_root.as_ref())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return Some(root.to_string());
    }
    state
        .get_automation_v2(&run.automation_id)
        .await
        .and_then(|automation| automation.workspace_root)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn queued_run_required_providers(run: &AutomationV2RunRecord) -> Vec<String> {
    let mut providers = HashSet::new();
    if let Some(automation) = &run.automation_snapshot {
        for agent in &automation.agents {
            if let Some(policy) = &agent.model_policy {
                if let Some(default_provider) = policy
                    .get("default_model")
                    .or_else(|| policy.get("defaultModel"))
                    .and_then(|m| m.get("provider_id").or_else(|| m.get("providerId")))
                    .and_then(|v| v.as_str())
                {
                    providers.insert(default_provider.to_string());
                }
                if let Some(role_models) = policy
                    .get("role_models")
                    .or_else(|| policy.get("roleModels"))
                    .and_then(|v| v.as_object())
                {
                    for model in role_models.values() {
                        if let Some(provider) = model
                            .get("provider_id")
                            .or_else(|| model.get("providerId"))
                            .and_then(|v| v.as_str())
                        {
                            providers.insert(provider.to_string());
                        }
                    }
                }
            }
        }
    }
    providers.into_iter().collect()
}

async fn execute_run_and_release_wrapped(state: AppState, run: AutomationV2RunRecord) {
    let run_id = run.run_id.clone();
    let result = AssertUnwindSafe(run_automation_v2_run(state.clone(), run))
        .catch_unwind()
        .await;

    if result.is_err() {
        let detail = "automation run panicked".to_string();
        let _ = state
            .update_automation_v2_run(&run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(detail.clone());
                row.stop_kind = Some(AutomationStopKind::Panic);
                row.stop_reason = Some(detail.clone());
                record_automation_lifecycle_event(
                    row,
                    "run_failed_panic",
                    Some(detail.clone()),
                    Some(AutomationStopKind::Panic),
                );
            })
            .await;
    }

    // Explicitly release capacity and lock
    let mut scheduler = state.automation_scheduler.write().await;
    scheduler.release_run(&run_id);
}

async fn persist_queue_metadata_if_changed(
    state: &AppState,
    run: &AutomationV2RunRecord,
    meta: crate::app::state::automation::SchedulerMetadata,
) {
    {
        let mut scheduler = state.automation_scheduler.write().await;
        scheduler.track_queue_state(&run.run_id, meta.clone());
    }

    if run.scheduler.as_ref() != Some(&meta) {
        let _ = state
            .set_automation_v2_run_scheduler_metadata(&run.run_id, meta)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::tests::ready_test_state;
    use crate::{
        AutomationAgentMcpPolicy, AutomationAgentProfile, AutomationAgentToolPolicy,
        AutomationExecutionPolicy, AutomationFlowNode, AutomationFlowSpec, AutomationV2Schedule,
        AutomationV2ScheduleType, AutomationV2Spec, AutomationV2Status, RoutineMisfirePolicy,
    };
    use serde_json::json;

    #[tokio::test]
    async fn hosted_startup_recovery_resumes_after_policy_outage() {
        let state = ready_test_state().await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("policy.json");
        let mut automation = test_automation(temp.path().to_str().unwrap());
        automation.agents.clear();
        automation.flow.nodes.clear();
        let run = state
            .create_automation_v2_run(&automation, "manual")
            .await
            .unwrap();
        state
            .claim_specific_automation_v2_run(&run.run_id)
            .await
            .unwrap();
        state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Pausing;
            })
            .await
            .unwrap();
        let guard = state.automation_v2_runs.write().await;
        let pending = state.recover_in_flight_runs();
        tokio::pin!(pending);
        assert!(futures::poll!(pending.as_mut()).is_pending());
        state
            .enterprise
            .hosted_policy
            .configure_test_source("org-a", "dep-a", path.clone());
        drop(guard);
        assert!(
            futures::poll!(pending.as_mut()).is_pending(),
            "recovery must wait, not report completion"
        );
        assert_eq!(
            state
                .get_automation_v2_run(&run.run_id)
                .await
                .unwrap()
                .status,
            AutomationRunStatus::Pausing
        );
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schema_version": 1, "policy_version": 1,
                "organization_id": "org-a", "deployment_id": "dep-a",
                "generated_at": chrono::Utc::now(), "users": [], "org_units": [],
                "org_unit_memberships": [], "deployment_grants": [],
            }))
            .unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        state.reload_hosted_policy().await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), pending)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            state
                .get_automation_v2_run(&run.run_id)
                .await
                .unwrap()
                .status,
            AutomationRunStatus::Paused
        );
        assert!(!state
            .automation_scheduler
            .read()
            .await
            .locked_workspaces
            .contains_key(temp.path().to_str().unwrap()));
    }

    #[tokio::test]
    async fn hosted_readiness_loss_during_run_lock_wait_blocks_claim_and_recovery() {
        for recovery in [false, true] {
            let state = ready_test_state().await;
            let temp = tempfile::tempdir().unwrap();
            let mut automation = test_automation(temp.path().to_str().unwrap());
            automation.agents.clear();
            automation.flow.nodes.clear();
            let run = state
                .create_automation_v2_run(&automation, "manual")
                .await
                .unwrap();
            if recovery {
                state
                    .update_automation_v2_run(&run.run_id, |row| {
                        row.status = AutomationRunStatus::Pausing;
                    })
                    .await
                    .unwrap();
            }
            let before = state.get_automation_v2_run(&run.run_id).await.unwrap();
            assert!(state.is_ready());
            let guard = state.automation_v2_runs.write().await;
            let make_unready = || {
                state.enterprise.hosted_policy.configure_test_source(
                    "org-a",
                    "dep-a",
                    temp.path().join("missing-policy.json"),
                )
            };
            if recovery {
                let pending = state.recover_in_flight_runs();
                tokio::pin!(pending);
                assert!(futures::poll!(pending.as_mut()).is_pending());
                make_unready();
                drop(guard);
                assert!(futures::poll!(pending.as_mut()).is_pending());
                state.set_automation_scheduler_stopping(true);
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(5), pending)
                        .await
                        .unwrap(),
                    0
                );
            } else {
                let pending = state.claim_specific_automation_v2_run(&run.run_id);
                tokio::pin!(pending);
                assert!(futures::poll!(pending.as_mut()).is_pending());
                make_unready();
                drop(guard);
                assert!(pending.await.is_none());
            }
            assert!(!state.is_ready());
            let after = state.get_automation_v2_run(&run.run_id).await.unwrap();
            assert_eq!(
                serde_json::to_value(after).unwrap(),
                serde_json::to_value(before).unwrap()
            );
        }
    }

    #[tokio::test]
    async fn hosted_unready_executors_leave_queued_runs_untouched_and_stop() {
        for multi in [false, true] {
            let state = ready_test_state().await;
            let temp = tempfile::tempdir().unwrap();
            let mut automation = test_automation(temp.path().to_str().unwrap());
            // No provider or tool work is possible even if admission regresses.
            automation.agents.clear();
            automation.flow.nodes.clear();
            let run = state
                .create_automation_v2_run(&automation, "manual")
                .await
                .unwrap();
            state.enterprise.hosted_policy.configure_test_source(
                "org-a",
                "dep-a",
                temp.path().join("missing-policy.json"),
            );
            assert!(!state.is_ready());
            let worker_state = state.clone();
            let worker = tokio::spawn(async move {
                if multi {
                    run_automation_v2_executor_multi(worker_state).await;
                } else {
                    run_automation_v2_executor_single(worker_state).await;
                }
            });
            tokio::time::sleep(Duration::from_millis(650)).await;
            state.set_automation_scheduler_stopping(true);
            tokio::time::timeout(Duration::from_secs(5), worker)
                .await
                .unwrap()
                .unwrap();
            let stored = state.get_automation_v2_run(&run.run_id).await.unwrap();
            assert_eq!(stored.status, AutomationRunStatus::Queued, "multi={multi}");
            assert_eq!(stored.updated_at_ms, run.updated_at_ms, "multi={multi}");
        }
    }

    #[tokio::test]
    async fn hosted_executor_resumes_after_policy_becomes_ready() {
        let state = ready_test_state().await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("policy.json");
        let mut automation = test_automation(temp.path().to_str().unwrap());
        automation.agents.clear();
        automation.flow.nodes.clear();
        let run = state
            .create_automation_v2_run(&automation, "manual")
            .await
            .unwrap();
        state
            .enterprise
            .hosted_policy
            .configure_test_source("org-a", "dep-a", path.clone());
        let worker = tokio::spawn(run_automation_v2_executor_supervised(state.clone()));
        tokio::time::sleep(Duration::from_millis(350)).await;
        assert_eq!(
            state
                .get_automation_v2_run(&run.run_id)
                .await
                .unwrap()
                .status,
            AutomationRunStatus::Queued
        );
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schema_version": 1, "policy_version": 1,
                "organization_id": "org-a", "deployment_id": "dep-a",
                "generated_at": chrono::Utc::now(), "users": [], "org_units": [],
                "org_unit_memberships": [], "deployment_grants": [],
            }))
            .unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        state.reload_hosted_policy().await.unwrap();
        assert!(state.is_ready());
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if state
                    .get_automation_v2_run(&run.run_id)
                    .await
                    .unwrap()
                    .status
                    == AutomationRunStatus::Completed
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        state.set_automation_scheduler_stopping(true);
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap();
        completed.expect("fresh policy must resume queued work");
    }

    fn test_automation(workspace_root: &str) -> AutomationV2Spec {
        AutomationV2Spec {
            automation_id: "auto-queue-metadata-deadlock-test".to_string(),
            name: "Queue Metadata Deadlock Test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: vec![AutomationAgentProfile {
                agent_id: "agent_researcher".to_string(),
                template_id: None,
                display_name: "Researcher".to_string(),
                avatar_url: None,
                model_policy: Some(json!({
                    "default_model": "openai-codex/gpt-5.4-mini"
                })),
                skills: Vec::new(),
                tool_policy: AutomationAgentToolPolicy {
                    allowlist: vec!["*".to_string()],
                    denylist: Vec::new(),
                },
                mcp_policy: AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                    allowed_connections: Vec::new(),
                },
                approval_policy: None,
            }],
            flow: AutomationFlowSpec {
                nodes: vec![AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "assess".to_string(),
                    agent_id: "agent_researcher".to_string(),
                    objective: "Assess the workspace.".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: None,
                    gate: None,
                    wait: None,
                    metadata: None,
                }],
            },
            execution: AutomationExecutionPolicy {
                profile: None,
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
            creator_id: "test".to_string(),
            workspace_root: Some(workspace_root.to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        }
    }

    #[tokio::test]
    async fn persist_queue_metadata_if_changed_returns_without_scheduler_deadlock() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-queue-meta-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_root).expect("workspace");
        let workspace_root = workspace_root.to_string_lossy().to_string();
        let state = ready_test_state().await;
        let automation = test_automation(&workspace_root);
        let run = state
            .create_automation_v2_run(&automation, "manual")
            .await
            .expect("create queued run");

        let meta = crate::app::state::automation::SchedulerMetadata {
            tenant_context: tandem_types::TenantContext::local_implicit(),
            queue_reason: Some(crate::app::state::automation::QueueReason::WorkspaceLock),
            resource_key: Some(workspace_root.clone()),
            rate_limited_provider: None,
            queued_at_ms: crate::util::time::now_ms(),
            retry_node_id: None,
            retry_attempt: None,
            retry_backoff_ms: None,
            retry_after_ms: None,
            retry_reason: None,
        };

        tokio::time::timeout(
            Duration::from_secs(1),
            persist_queue_metadata_if_changed(&state, &run, meta.clone()),
        )
        .await
        .expect("queue metadata update should not deadlock");

        let persisted = state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("persisted run");
        assert_eq!(persisted.scheduler, Some(meta));
    }

    #[tokio::test]
    async fn scheduler_wait_wake_requeues_authoritative_automation_run() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-stateful-wait-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_root).expect("workspace");
        let workspace_root = workspace_root.to_string_lossy().to_string();
        let state = ready_test_state().await;
        let automation = test_automation(&workspace_root);
        state
            .put_automation_v2(automation.clone())
            .await
            .expect("put automation");
        let run = state
            .create_automation_v2_run(&automation, "manual")
            .await
            .expect("create queued run");
        let run_id = run.run_id.clone();
        let paused = state
            .update_automation_v2_run(&run_id, |row| {
                row.status = AutomationRunStatus::Paused;
                row.detail = Some("sleeping until timer wait".to_string());
                row.pause_reason = Some("timer wait".to_string());
            })
            .await
            .expect("pause run");
        let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
            &state.runtime_events_path,
        );
        let now = crate::util::time::now_ms();
        crate::stateful_runtime::upsert_stateful_wait(
            &paths.waits_path,
            crate::stateful_runtime::StatefulWaitRecord {
                schema_version: 1,
                wait_id: "timer-wait-resume".to_string(),
                run_id: run_id.clone(),
                wait_kind: crate::stateful_runtime::StatefulWaitKind::Timer,
                status: crate::stateful_runtime::StatefulWaitStatus::Waiting,
                scope: crate::stateful_runtime::StatefulRuntimeScope::from_tenant_context(
                    paused.tenant_context.clone(),
                ),
                phase_id: Some("sleep".to_string()),
                reason: Some("resume paused automation after timer".to_string()),
                created_at_ms: now.saturating_sub(10_000),
                updated_at_ms: now.saturating_sub(10_000),
                wake_at_ms: Some(now.saturating_sub(1)),
                timeout_policy: None,
                event_seq: None,
                wake_idempotency_key: None,
                claimed_by: None,
                claimed_at_ms: None,
                claim_expires_at_ms: None,
                completed_at_ms: None,
                metadata: None,
            },
        )
        .await
        .expect("upsert wait");

        process_stateful_wait_scheduler_tick(&state).await;

        let updated = state
            .get_automation_v2_run(&run_id)
            .await
            .expect("updated run");
        assert_eq!(updated.status, AutomationRunStatus::Queued);
        assert_eq!(
            updated.resume_reason.as_deref(),
            Some("stateful_runtime.wait.timer_woken")
        );
        assert!(updated
            .checkpoint
            .lifecycle_history
            .iter()
            .any(|entry| entry.event == "stateful_wait_woken_requeued"));
        let waits = crate::stateful_runtime::list_stateful_waits(
            &paths.waits_path,
            &updated.tenant_context,
            crate::stateful_runtime::StatefulWaitQuery {
                run_id: Some(&run_id),
                wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Timer),
                status: None,
                limit: None,
            },
        );
        assert_eq!(waits.len(), 1);
        assert_eq!(
            waits[0].status,
            crate::stateful_runtime::StatefulWaitStatus::Woken
        );
    }
}
