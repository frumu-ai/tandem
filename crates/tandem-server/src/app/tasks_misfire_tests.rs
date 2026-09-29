// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use crate::app::state::tests::AutomationSpecBuilder;
use crate::routines::types::{RoutineMisfirePolicy, RoutineSchedule, RoutineSpec, RoutineStatus};

fn due_routine(id: &str, policy: RoutineMisfirePolicy) -> RoutineSpec {
    RoutineSpec {
        solution_owner: None,
        routine_id: id.into(),
        tenant_context: tandem_types::TenantContext::local_implicit(),
        name: id.into(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 1 },
        timezone: "UTC".into(),
        misfire_policy: policy,
        entrypoint: "mission.default".into(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".into(),
        creator_id: "u-1".into(),
        requires_approval: false,
        external_integrations_allowed: false,
        next_fire_at_ms: Some(5_000),
        last_fired_at_ms: None,
    }
}

async fn add_due_automations(state: &AppState, workspace: &std::path::Path) {
    for (id, policy) in [
        ("once", RoutineMisfirePolicy::RunOnce),
        ("catch", RoutineMisfirePolicy::CatchUp { max_runs: 3 }),
    ] {
        let mut automation = AutomationSpecBuilder::new(id).build();
        automation.agents.clear();
        automation.workspace_root = Some(workspace.to_string_lossy().into_owned());
        automation.schedule = crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Interval,
            cron_expression: None,
            interval_seconds: Some(1),
            timezone: "UTC".into(),
            misfire_policy: policy,
        };
        automation.next_fire_at_ms = Some(5_000);
        state
            .automations_v2
            .write()
            .await
            .insert(id.into(), automation);
    }
}

fn make_policy_unavailable(state: &AppState, workspace: &std::path::Path) {
    state.enterprise.hosted_policy.configure_test_source(
        "org-a",
        "dep-a",
        workspace.join("missing-policy.json"),
    );
    assert!(!state.is_ready());
}

#[tokio::test]
async fn routine_misfire_rechecks_policy_after_mutation_lock_wait() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    state
        .put_routine(due_routine("once", RoutineMisfirePolicy::RunOnce))
        .await
        .unwrap();
    let guard = state.routines.write().await;
    let before = serde_json::to_value(&*guard).unwrap();
    let evaluation = state.evaluate_routine_misfires(10_500);
    tokio::pin!(evaluation);
    assert!(futures::poll!(evaluation.as_mut()).is_pending());
    make_policy_unavailable(&state, temp.path());
    drop(guard);
    assert!(evaluation.await.is_empty());
    assert_eq!(
        before,
        serde_json::to_value(&*state.routines.read().await).unwrap()
    );
}

#[tokio::test]
async fn automation_misfire_rechecks_policy_after_mutation_lock_wait() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    add_due_automations(&state, temp.path()).await;
    let guard = state.automations_v2.write().await;
    let before = serde_json::to_value(&*guard).unwrap();
    let evaluation = state.evaluate_automation_v2_misfires(10_500);
    tokio::pin!(evaluation);
    assert!(futures::poll!(evaluation.as_mut()).is_pending());
    make_policy_unavailable(&state, temp.path());
    drop(guard);
    assert!(evaluation.await.is_empty());
    assert_eq!(
        before,
        serde_json::to_value(&*state.automations_v2.read().await).unwrap()
    );
}

#[tokio::test]
async fn routine_misfire_admitted_batch_survives_policy_loss_without_execution() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    for (id, policy) in [
        ("once", RoutineMisfirePolicy::RunOnce),
        ("catch", RoutineMisfirePolicy::CatchUp { max_runs: 3 }),
    ] {
        state.put_routine(due_routine(id, policy)).await.unwrap();
    }
    let plans = state.evaluate_routine_misfires(10_500).await;
    assert_eq!(plans.len(), 2);
    make_policy_unavailable(&state, temp.path());
    materialize_routine_timer_batch(&state, plans, 10_500).await;
    let runs = state.routine_runs.read().await;
    assert_eq!(runs.len(), 2, "the whole admitted batch must be retained");
    for run in runs.values() {
        assert_eq!(run.status, crate::routines::types::RoutineRunStatus::Queued);
        assert_eq!(run.run_count, if run.routine_id == "catch" { 3 } else { 1 });
    }
    drop(runs);
    assert!(state.claim_next_queued_routine_run().await.is_none());
}

#[tokio::test]
async fn automation_misfire_admitted_batch_survives_policy_loss_without_execution() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    add_due_automations(&state, temp.path()).await;
    let due = state.evaluate_automation_v2_misfires(10_500).await;
    assert_eq!(due.len(), 4);
    make_policy_unavailable(&state, temp.path());
    materialize_automation_timer_batch(&state, due).await;
    let runs = state.automation_v2_runs.read().await;
    assert_eq!(
        runs.len(),
        4,
        "the whole admitted catch-up batch must be retained"
    );
    assert_eq!(
        runs.values()
            .filter(|run| run.automation_id == "catch")
            .count(),
        3
    );
    assert!(runs
        .values()
        .all(|run| run.status == crate::automation_v2::types::AutomationRunStatus::Queued));
    drop(runs);
    assert!(state.claim_next_queued_automation_v2_run().await.is_none());
}
