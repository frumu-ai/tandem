use super::*;

#[tokio::test]
async fn dead_letter_retry_honors_backoff_before_redispatch() {
    let state = test_state().await;
    let tenant = tenant("org-dl-backoff", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-dead-letter-backoff";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let run = failed_run_with_snapshot(&state, run_id, tenant.clone(), "review").await;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    // Already dispatched far in the future -- its backoff window has not elapsed.
    let mut dead_letter = retry_requested_dead_letter("dead-backoff-effect", run_id, scope);
    dead_letter.status = StatefulDeadLetterStatus::Retrying;
    dead_letter.metadata = Some(json!({
        "retry_dispatched_at_ms": 9_000_000_000_000_u64,
        "retry_backoff_ms": 4_000,
    }));
    upsert_stateful_dead_letter(&path, dead_letter)
        .await
        .expect("seed retrying dead letter");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(
        acted, 0,
        "a dead letter inside its backoff window is skipped"
    );

    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Failed);
}

#[tokio::test]
async fn superseded_dead_letter_resolves_after_successful_replay() {
    let state = test_state().await;
    let tenant = tenant("org-dl-success", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-dead-letter-success";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let run = failed_run_with_snapshot(&state, run_id, tenant.clone(), "review").await;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    let mut dead_letter = retry_requested_dead_letter("dead-success-effect", run_id, scope);
    dead_letter.status = StatefulDeadLetterStatus::Retrying;
    dead_letter.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-success",
        "superseded_at_ms": 1_500,
    }));
    upsert_stateful_dead_letter(&path, dead_letter)
        .await
        .expect("seed superseded dead letter");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(acted, 1, "a superseded dead letter is resolved");

    let dead_letter = read_dead_letter(&state, &tenant, run_id, "dead-success-effect").await;
    assert_eq!(dead_letter.status, StatefulDeadLetterStatus::Resolved);
    assert_eq!(
        dead_letter.operator_disposition.as_deref(),
        Some("retry_succeeded")
    );
    // A resolved dead letter must not also re-drive the run.
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Failed);
}

#[tokio::test]
async fn exhausted_dead_letter_is_parked_for_operator_review() {
    let state = test_state().await;
    let tenant = tenant("org-dl-exhausted", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-dead-letter-exhausted";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let run = failed_run_with_snapshot(&state, run_id, tenant.clone(), "review").await;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    let mut dead_letter = retry_requested_dead_letter("dead-exhausted-effect", run_id, scope);
    dead_letter.status = StatefulDeadLetterStatus::Retrying;
    // Five dispatcher retries already recorded -- the cap counts these, not the
    // node/tool `attempts` field.
    dead_letter.metadata = Some(json!({ "retry_dispatch_count": 5 }));
    upsert_stateful_dead_letter(&path, dead_letter)
        .await
        .expect("seed exhausted dead letter");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(acted, 1, "an exhausted dead letter is parked");

    let dead_letter = read_dead_letter(&state, &tenant, run_id, "dead-exhausted-effect").await;
    assert_eq!(dead_letter.status, StatefulDeadLetterStatus::Ignored);
    assert_eq!(
        dead_letter.operator_disposition.as_deref(),
        Some("retry_exhausted")
    );
    // Exhausted retries must not re-drive the run.
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Failed);
}

#[tokio::test]
async fn dead_letter_retry_skips_non_recoverable_run() {
    // A run that is already executing must never be re-driven out from under
    // its own executor by a dead-letter retry.
    let state = test_state().await;
    let tenant = tenant("org-dl-active", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-dead-letter-active";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let mut run = failed_run_with_snapshot(&state, run_id, tenant.clone(), "review").await;
    run.status = AutomationRunStatus::Running;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    upsert_stateful_dead_letter(
        &path,
        retry_requested_dead_letter("dead-active-effect", run_id, scope),
    )
    .await
    .expect("seed dead letter for active run");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(acted, 0, "a running run must not be re-driven");

    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Running);
    let dead_letter = read_dead_letter(&state, &tenant, run_id, "dead-active-effect").await;
    assert_eq!(dead_letter.status, StatefulDeadLetterStatus::RetryRequested);
}

#[tokio::test]
async fn dead_letter_born_on_high_node_attempt_still_retries() {
    // Regression: `attempts` is the node/tool execution attempt at creation
    // (already at the executor cap for required-tool nodes). The retry cap must
    // count *dispatcher* retries, so the first requested retry must still
    // re-drive the run even when `attempts` is high.
    let state = test_state().await;
    let tenant = tenant("org-dl-high-attempt", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-dead-letter-high-attempt";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let run = failed_run_with_snapshot(&state, run_id, tenant.clone(), "review").await;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    let mut dead_letter = retry_requested_dead_letter("dead-high-attempt-effect", run_id, scope);
    // Node/tool attempts already at the executor cap -- must NOT pre-exhaust.
    dead_letter.attempts = 5;
    upsert_stateful_dead_letter(&path, dead_letter)
        .await
        .expect("seed dead letter");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(
        acted, 1,
        "a high node-attempt count must not block the first dispatcher retry"
    );

    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Queued);
    let dead_letter = read_dead_letter(&state, &tenant, run_id, "dead-high-attempt-effect").await;
    assert_eq!(dead_letter.status, StatefulDeadLetterStatus::Retrying);
    assert_eq!(
        dead_letter
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("retry_dispatch_count"))
            .and_then(|value| value.as_u64()),
        Some(1)
    );
}
