use super::*;
use crate::app::state::{evaluate_routine_execution_policy, RoutineExecutionDecision};
use crate::routines::types::RoutineRunStatus;

fn fixture(org: &str) -> (RoutineSpec, SolutionRoutineOwner) {
    let mut tenant = TenantContext::local_implicit();
    tenant.org_id = org.into();
    tenant.workspace_id = "workspace".into();
    tenant.deployment_id = Some(format!("deployment-{org}"));
    let artifact = include_bytes!(
        "../../../../../tandem-solutions/fixtures/company-brain-text/routines/review-notes.json"
    );
    let routine =
        solution_routine_from_artifact(artifact, "solution-test-review-notes", &tenant).unwrap();
    let owner = SolutionRoutineOwner {
        instance_id: "brain".into(),
        component_id: "review-notes".into(),
        composition_sha256: "a".repeat(64),
        enabled: true,
    };
    (routine, owner)
}

fn state(directory: &std::path::Path) -> AppState {
    let mut state = AppState::new_starting("staged-routine-test".into(), true);
    state.routines_path = directory.join("routines.json");
    state.routine_runs_path = directory.join("runs.json");
    state
}

#[tokio::test]
async fn solution_routine_staging_is_scoped_durable_and_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let original = state(directory.path());
    let (a, owner) = fixture("org-a");
    let (mut b, _) = fixture("org-b");
    b.name = "Another customer's review".into();
    let (first, second) = tokio::join!(
        original.stage_solution_routine(a.clone(), owner.clone()),
        original.stage_solution_routine(b.clone(), owner.clone())
    );
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_ne!(first, second);
    let restarted = state(directory.path());
    restarted.load_routines().await.unwrap();
    assert_eq!(
        restarted
            .stage_solution_routine(a.clone(), owner.clone())
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        restarted
            .stage_solution_routine(b.clone(), owner)
            .await
            .unwrap(),
        second
    );
    let observed = restarted
        .get_routine_for_tenant(&a.routine_id, &a.tenant_context)
        .await
        .unwrap();
    assert_eq!(observed.status, RoutineStatus::Paused);
    assert!(observed.installation_disabled());
    assert!(observed.next_fire_at_ms.is_none());
    assert_eq!(sha256(&bytes(&observed).unwrap()), first);
    assert!(restarted
        .evaluate_routine_misfires(u64::MAX)
        .await
        .is_empty());
    assert_eq!(
        restarted
            .list_routines_for_tenant(&a.tenant_context)
            .await
            .len(),
        1
    );
    assert_eq!(
        restarted
            .list_routines_for_tenant(&b.tenant_context)
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn solution_routine_rejects_conflicts_and_generic_activation() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let (routine, owner) = fixture("org-a");
    let receipt = state
        .stage_solution_routine(routine.clone(), owner.clone())
        .await
        .unwrap();
    let mut changed = routine.clone();
    changed.name = "changed".into();
    assert!(state
        .stage_solution_routine(changed, owner.clone())
        .await
        .is_err());
    let mut other_owner = owner;
    other_owner.instance_id = "another-installation".into();
    assert!(state
        .stage_solution_routine(routine.clone(), other_owner)
        .await
        .is_err());
    assert!(state.put_routine(routine.clone()).await.is_err());
    assert!(state
        .update_routine_for_tenant(&routine.routine_id, &routine.tenant_context, |row| {
            row.status = RoutineStatus::Active;
            row.solution_owner = None;
        })
        .await
        .is_err());
    assert!(state
        .delete_routine_for_tenant(&routine.routine_id, &routine.tenant_context)
        .await
        .is_err());
    let observed = state
        .get_routine_for_tenant(&routine.routine_id, &routine.tenant_context)
        .await
        .unwrap();
    assert_eq!(sha256(&bytes(&observed).unwrap()), receipt);
}

#[tokio::test]
async fn solution_routine_blocks_manual_approval_replay_and_missing_resource_queue() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let (routine, owner) = fixture("org-a");
    state
        .stage_solution_routine(routine.clone(), owner)
        .await
        .unwrap();
    let observed = state
        .get_routine_for_tenant(&routine.routine_id, &routine.tenant_context)
        .await
        .unwrap();
    for trigger in ["manual", "scheduled", "approval"] {
        assert!(matches!(
            evaluate_routine_execution_policy(&observed, trigger),
            RoutineExecutionDecision::Blocked { .. }
        ));
    }
    let run = state
        .create_routine_run(&observed, "manual", 1, RoutineRunStatus::Queued, None)
        .await;
    assert_eq!(run.status, RoutineRunStatus::BlockedPolicy);
    // An old approval or restored run may put this back in the queue. The final
    // claimant independently checks the actual current native resource.
    for missing in [false, true] {
        if missing {
            state.routines.write().await.clear();
        }
        state
            .update_routine_run_status(&run.run_id, RoutineRunStatus::Queued, None)
            .await
            .unwrap();
        assert!(state.claim_next_queued_routine_run().await.is_none());
        assert_eq!(
            state.get_routine_run(&run.run_id).await.unwrap().status,
            RoutineRunStatus::BlockedPolicy
        );
    }
    let mut ordinary = routine;
    ordinary.routine_id = "ordinary-paused".into();
    assert_eq!(
        evaluate_routine_execution_policy(&ordinary, "manual"),
        RoutineExecutionDecision::Allowed
    );
    let regular = state
        .create_routine_run(&ordinary, "manual", 1, RoutineRunStatus::Queued, None)
        .await;
    assert_eq!(
        state.claim_next_queued_routine_run().await.unwrap().run_id,
        regular.run_id
    );
}

#[tokio::test]
async fn solution_routine_detects_disk_drift_without_overwriting_manual_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let (routine, owner) = fixture("org-a");
    let identity = RoutineIdentity::new(&routine.routine_id, &routine.tenant_context);
    let manual = bytes(&HashMap::from([(identity.storage_key(), routine.clone())])).unwrap();
    tokio::fs::write(&state.routines_path, &manual)
        .await
        .unwrap();
    assert!(state
        .stage_solution_routine(routine.clone(), owner.clone())
        .await
        .is_err());
    state.load_routines().await.unwrap();
    assert!(state.stage_solution_routine(routine, owner).await.is_err());
    assert_eq!(tokio::fs::read(&state.routines_path).await.unwrap(), manual);
}

#[tokio::test]
async fn solution_routine_failed_publication_rolls_back_cache() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let temporary = crate::config::paths::sibling_tmp_path(&state.routines_path);
    tokio::fs::create_dir(&temporary).await.unwrap();
    let (routine, owner) = fixture("org-a");
    assert!(state
        .stage_solution_routine(routine.clone(), owner.clone())
        .await
        .is_err());
    assert!(state.list_routines().await.is_empty());
    assert!(!state.routines_path.exists());
    tokio::fs::remove_dir(temporary).await.unwrap();
    assert!(state.stage_solution_routine(routine, owner).await.is_ok());
}

#[test]
fn solution_routine_artifact_cannot_supply_authority() {
    let (routine, _) = fixture("org-a");
    assert_eq!(routine.misfire_policy, RoutineMisfirePolicy::Skip);
    let mut artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../../tandem-solutions/fixtures/company-brain-text/routines/review-notes.json"
    ))
    .unwrap();
    artifact["tenant_context"] = serde_json::json!({"org_id": "org-b"});
    assert!(solution_routine_from_artifact(
        &serde_json::to_vec(&artifact).unwrap(),
        &routine.routine_id,
        &routine.tenant_context
    )
    .is_err());
}
