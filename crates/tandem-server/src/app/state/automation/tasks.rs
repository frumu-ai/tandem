use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use futures::FutureExt;
use tokio::task::JoinSet;

use crate::app::state::automation::{record_automation_lifecycle_event, QueueReason};
use crate::app::state::AppState;
use crate::automation_v2::executor::run_automation_v2_run;
use crate::automation_v2::types::{AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord};

pub async fn run_automation_v2_executor(state: AppState) {
    // Step 6: Call recover_in_flight_runs on startup
    let _ = state.recover_in_flight_runs().await;

    if crate::config::env::resolve_scheduler_mode() == crate::config::env::SchedulerMode::Multi {
        run_automation_v2_executor_multi(state).await;
    } else {
        run_automation_v2_executor_single(state).await;
    }
}

async fn run_automation_v2_executor_single(state: AppState) {
    loop {
        if state.is_automation_scheduler_stopping() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        if state.is_automation_scheduler_stopping() {
            break;
        }
        let Some(run) = state.claim_next_queued_automation_v2_run().await else {
            continue;
        };
        run_automation_v2_run(state.clone(), run).await;
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

        let capacity = {
            let scheduler = state.automation_scheduler.read().await;
            scheduler.max_concurrent_runs
        };

        while active.len() < capacity {
            let queued = queued_runs_for_admission(&state).await;
            if queued.is_empty() {
                break;
            }

            let mut admitted_any = false;
            for run in queued {
                if active.len() >= capacity {
                    break;
                }

                let workspace_root = queued_run_workspace_root(&state, &run).await;
                let required_providers = queued_run_required_providers(&run);
                let admission = {
                    let scheduler = state.automation_scheduler.read().await;
                    scheduler.can_admit(&run.run_id, workspace_root.as_deref(), &required_providers)
                };

                match admission {
                    Ok(()) => {
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
                        let mut scheduler = state.automation_scheduler.write().await;
                        scheduler.track_queue_state(&run.run_id, meta.clone());
                        if run.scheduler.as_ref() != Some(&meta) {
                            let _ = state
                                .set_automation_v2_run_scheduler_metadata(&run.run_id, meta)
                                .await;
                        }
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
    let mut queued = state
        .automation_v2_runs
        .read()
        .await
        .values()
        .filter(|run| run.status == AutomationRunStatus::Queued)
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
