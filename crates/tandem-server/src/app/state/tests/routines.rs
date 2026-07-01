use super::*;
#[test]
fn misfire_skip_drops_runs_and_advances_next_fire() {
    let (count, next_fire) =
        compute_misfire_plan(10_500, 5_000, 1_000, &RoutineMisfirePolicy::Skip);
    assert_eq!(count, 0);
    assert_eq!(next_fire, 11_000);
}

#[test]
fn misfire_run_once_emits_single_trigger() {
    let (count, next_fire) =
        compute_misfire_plan(10_500, 5_000, 1_000, &RoutineMisfirePolicy::RunOnce);
    assert_eq!(count, 1);
    assert_eq!(next_fire, 11_000);
}

#[test]
fn misfire_catch_up_caps_trigger_count() {
    let (count, next_fire) = compute_misfire_plan(
        25_000,
        5_000,
        1_000,
        &RoutineMisfirePolicy::CatchUp { max_runs: 3 },
    );
    assert_eq!(count, 3);
    assert_eq!(next_fire, 26_000);
}

#[test]
fn cron_next_fire_uses_schedule_timezone_wall_clock() {
    let schedule = RoutineSchedule::Cron {
        expression: "0 9 * * 1-5".to_string(),
    };
    let from_ms = Utc
        .with_ymd_and_hms(2026, 5, 4, 12, 0, 0)
        .unwrap()
        .timestamp_millis() as u64;
    let next_fire = compute_next_schedule_fire_at_ms(&schedule, "Europe/Budapest", from_ms)
        .expect("compute next Budapest weekday fire");
    let expected = Utc
        .with_ymd_and_hms(2026, 5, 5, 7, 0, 0)
        .unwrap()
        .timestamp_millis() as u64;

    assert_eq!(next_fire, expected);
}

#[tokio::test]
async fn routine_put_persists_and_loads() {
    let routines_path = tmp_routines_file("persist-load");
    let mut state = AppState::new_starting("routines-put".to_string(), true);
    state.routines_path = routines_path.clone();

    let routine = RoutineSpec {
        routine_id: "routine-1".to_string(),
        name: "Digest".to_string(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({"topic":"status"}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".to_string(),
        creator_id: "user-1".to_string(),
        requires_approval: true,
        external_integrations_allowed: false,
        next_fire_at_ms: Some(5_000),
        last_fired_at_ms: None,
    };

    state.put_routine(routine).await.expect("store routine");

    let mut reloaded = AppState::new_starting("routines-reload".to_string(), true);
    reloaded.routines_path = routines_path.clone();
    reloaded.load_routines().await.expect("load routines");
    let list = reloaded.list_routines().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].routine_id, "routine-1");

    let _ = tokio::fs::remove_file(routines_path).await;
}

#[tokio::test]
async fn persist_routines_does_not_clobber_existing_store_with_empty_state() {
    let routines_path = tmp_routines_file("persist-guard");
    let mut writer = AppState::new_starting("routines-writer".to_string(), true);
    writer.routines_path = routines_path.clone();
    writer
        .put_routine(RoutineSpec {
            routine_id: "automation-guarded".to_string(),
            name: "Guarded Automation".to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 300 },
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({
                "prompt": "Keep this saved across restart"
            }),
            allowed_tools: vec!["read".to_string()],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "user-1".to_string(),
            requires_approval: false,
            external_integrations_allowed: false,
            next_fire_at_ms: Some(5_000),
            last_fired_at_ms: None,
        })
        .await
        .expect("persist baseline routine");

    let mut empty_state = AppState::new_starting("routines-empty".to_string(), true);
    empty_state.routines_path = routines_path.clone();
    let persist = empty_state.persist_routines().await;
    assert!(
        persist.is_err(),
        "empty state should not overwrite existing routines store"
    );

    let raw = tokio::fs::read_to_string(&routines_path)
        .await
        .expect("read guarded routines file");
    let parsed: std::collections::HashMap<String, RoutineSpec> =
        serde_json::from_str(&raw).expect("parse guarded routines file");
    assert!(parsed.contains_key("automation-guarded"));

    let _ = tokio::fs::remove_file(routines_path.clone()).await;
    let _ = tokio::fs::remove_file(config::paths::sibling_backup_path(&routines_path)).await;
}

#[tokio::test]
async fn load_routines_recovers_from_backup_when_primary_corrupt() {
    let routines_path = tmp_routines_file("backup-recovery");
    let backup_path = config::paths::sibling_backup_path(&routines_path);
    let mut state = AppState::new_starting("routines-backup-recovery".to_string(), true);
    state.routines_path = routines_path.clone();

    let primary = "{ not valid json";
    tokio::fs::write(&routines_path, primary)
        .await
        .expect("write corrupt primary");
    let backup = serde_json::json!({
        "routine-1": {
            "routine_id": "routine-1",
            "name": "Recovered",
            "status": "active",
            "schedule": { "interval_seconds": { "seconds": 60 } },
            "timezone": "UTC",
            "misfire_policy": { "type": "run_once" },
            "entrypoint": "mission.default",
            "args": {},
            "allowed_tools": [],
            "output_targets": [],
            "creator_type": "user",
            "creator_id": "u-1",
            "requires_approval": true,
            "external_integrations_allowed": false,
            "next_fire_at_ms": null,
            "last_fired_at_ms": null
        }
    });
    tokio::fs::write(&backup_path, serde_json::to_string_pretty(&backup).unwrap())
        .await
        .expect("write backup");

    state.load_routines().await.expect("load from backup");
    let list = state.list_routines().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].routine_id, "routine-1");

    let _ = tokio::fs::remove_file(routines_path).await;
    let _ = tokio::fs::remove_file(backup_path).await;
}

#[tokio::test]
async fn evaluate_routine_misfires_respects_skip_run_once_and_catch_up() {
    let routines_path = tmp_routines_file("misfire-eval");
    let mut state = AppState::new_starting("routines-eval".to_string(), true);
    state.routines_path = routines_path.clone();

    let base = |id: &str, policy: RoutineMisfirePolicy| RoutineSpec {
        routine_id: id.to_string(),
        name: id.to_string(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 1 },
        timezone: "UTC".to_string(),
        misfire_policy: policy,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".to_string(),
        creator_id: "u-1".to_string(),
        requires_approval: false,
        external_integrations_allowed: false,
        next_fire_at_ms: Some(5_000),
        last_fired_at_ms: None,
    };

    state
        .put_routine(base("routine-skip", RoutineMisfirePolicy::Skip))
        .await
        .expect("put skip");
    state
        .put_routine(base("routine-once", RoutineMisfirePolicy::RunOnce))
        .await
        .expect("put once");
    state
        .put_routine(base(
            "routine-catch",
            RoutineMisfirePolicy::CatchUp { max_runs: 3 },
        ))
        .await
        .expect("put catch");

    let plans = state.evaluate_routine_misfires(10_500).await;
    let plan_skip = plans.iter().find(|p| p.routine_id == "routine-skip");
    let plan_once = plans.iter().find(|p| p.routine_id == "routine-once");
    let plan_catch = plans.iter().find(|p| p.routine_id == "routine-catch");

    assert!(plan_skip.is_none());
    assert_eq!(plan_once.map(|p| p.run_count), Some(1));
    assert_eq!(plan_catch.map(|p| p.run_count), Some(3));

    let stored = state.list_routines().await;
    let skip_next = stored
        .iter()
        .find(|r| r.routine_id == "routine-skip")
        .and_then(|r| r.next_fire_at_ms)
        .expect("skip next");
    assert!(skip_next > 10_500);

    let _ = tokio::fs::remove_file(routines_path).await;
}

#[test]
fn routine_policy_blocks_external_side_effects_by_default() {
    let routine = RoutineSpec {
        routine_id: "routine-policy-1".to_string(),
        name: "Connector routine".to_string(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
        entrypoint: "connector.email.reply".to_string(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".to_string(),
        creator_id: "u-1".to_string(),
        requires_approval: true,
        external_integrations_allowed: false,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    let decision = evaluate_routine_execution_policy(&routine, "manual");
    assert!(matches!(decision, RoutineExecutionDecision::Blocked { .. }));
}

#[test]
fn routine_policy_requires_approval_for_external_side_effects_when_enabled() {
    let routine = RoutineSpec {
        routine_id: "routine-policy-2".to_string(),
        name: "Connector routine".to_string(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
        entrypoint: "connector.email.reply".to_string(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".to_string(),
        creator_id: "u-1".to_string(),
        requires_approval: true,
        external_integrations_allowed: true,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    let decision = evaluate_routine_execution_policy(&routine, "manual");
    assert!(matches!(
        decision,
        RoutineExecutionDecision::RequiresApproval { .. }
    ));
}

#[test]
fn routine_policy_allows_non_external_entrypoints() {
    let routine = RoutineSpec {
        routine_id: "routine-policy-3".to_string(),
        name: "Internal mission routine".to_string(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".to_string(),
        creator_id: "u-1".to_string(),
        requires_approval: true,
        external_integrations_allowed: false,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    let decision = evaluate_routine_execution_policy(&routine, "manual");
    assert_eq!(decision, RoutineExecutionDecision::Allowed);
}

#[tokio::test]
async fn record_external_action_appends_routine_receipt_artifact() {
    let state = test_state_with_path(tmp_resource_file("external-action-artifact"));
    let run = RoutineRunRecord {
        run_id: "run-1".to_string(),
        routine_id: "routine-1".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Completed,
        created_at_ms: 1,
        updated_at_ms: 1,
        fired_at_ms: Some(1),
        started_at_ms: Some(1),
        finished_at_ms: Some(1),
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "workflow.publish".to_string(),
        args: Value::Null,
        allowed_tools: Vec::new(),
        output_targets: Vec::new(),
        artifacts: Vec::new(),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };
    state
        .routine_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    state
        .record_external_action(ExternalActionRecord {
            action_id: "action-1".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            routine_run_id: Some("run-1".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some("idem-1".to_string()),
            receipt: Some(json!({"issue_number": 101})),
            error: None,
            metadata: None,
            created_at_ms: 10,
            updated_at_ms: 10,
        })
        .await
        .expect("record external action");

    let duplicate = state
        .record_external_action(ExternalActionRecord {
            action_id: "action-2".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            routine_run_id: Some("run-1".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some("idem-1".to_string()),
            receipt: Some(json!({"issue_number": 101})),
            error: None,
            metadata: None,
            created_at_ms: 11,
            updated_at_ms: 11,
        })
        .await
        .expect("record duplicate external action");

    let updated = state.get_routine_run("run-1").await.expect("routine run");
    assert_eq!(updated.artifacts.len(), 1);
    assert_eq!(updated.artifacts[0].kind, "external_action_receipt");
    assert_eq!(updated.artifacts[0].uri, "external-action://action-1");
    assert_eq!(
        updated.artifacts[0]
            .metadata
            .as_ref()
            .and_then(|row| row.get("actionID"))
            .and_then(Value::as_str),
        Some("action-1")
    );
    assert_eq!(
        duplicate.action_id, "action-1",
        "duplicate idempotency key should return the original action"
    );
    assert_eq!(state.list_external_actions(10).await.len(), 1);
    assert_eq!(
        state
            .get_external_action("action-1")
            .await
            .and_then(|row| row.capability_id),
        Some("github.create_issue".to_string())
    );
}

#[tokio::test]
async fn record_external_action_without_idempotency_key_keeps_current_behavior() {
    let state = test_state_with_path(tmp_resource_file("external-action-no-idempotency"));
    let run = RoutineRunRecord {
        run_id: "run-2".to_string(),
        routine_id: "routine-2".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Completed,
        created_at_ms: 1,
        updated_at_ms: 1,
        fired_at_ms: Some(1),
        started_at_ms: Some(1),
        finished_at_ms: Some(1),
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "workflow.publish".to_string(),
        args: Value::Null,
        allowed_tools: Vec::new(),
        output_targets: Vec::new(),
        artifacts: Vec::new(),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };
    state
        .routine_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    state
        .record_external_action(ExternalActionRecord {
            action_id: "action-a".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-2".to_string()),
            routine_run_id: Some("run-2".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: None,
            receipt: Some(json!({"issue_number": 201})),
            error: None,
            metadata: None,
            created_at_ms: 20,
            updated_at_ms: 20,
        })
        .await
        .expect("record first external action");
    state
        .record_external_action(ExternalActionRecord {
            action_id: "action-b".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-3".to_string()),
            routine_run_id: Some("run-2".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: None,
            receipt: Some(json!({"issue_number": 202})),
            error: None,
            metadata: None,
            created_at_ms: 21,
            updated_at_ms: 21,
        })
        .await
        .expect("record second external action");

    let updated = state.get_routine_run("run-2").await.expect("routine run");
    assert_eq!(updated.artifacts.len(), 2);
    assert_eq!(state.list_external_actions(10).await.len(), 2);
}

#[tokio::test]
async fn record_external_action_dedupes_by_idempotency_key() {
    let state = test_state_with_path(tmp_resource_file("external-action-dedupe"));
    let run = RoutineRunRecord {
        run_id: "run-1".to_string(),
        routine_id: "routine-1".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Completed,
        created_at_ms: 1,
        updated_at_ms: 1,
        fired_at_ms: Some(1),
        started_at_ms: Some(1),
        finished_at_ms: Some(1),
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "workflow.publish".to_string(),
        args: Value::Null,
        allowed_tools: Vec::new(),
        output_targets: Vec::new(),
        artifacts: Vec::new(),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };
    state
        .routine_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    let first = state
        .record_external_action(ExternalActionRecord {
            action_id: "action-1".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            routine_run_id: Some("run-1".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some("idem-1".to_string()),
            receipt: Some(json!({"issue_number": 101})),
            error: None,
            metadata: None,
            created_at_ms: 10,
            updated_at_ms: 10,
        })
        .await
        .expect("record first external action");
    let second = state
        .record_external_action(ExternalActionRecord {
            action_id: "action-2".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            routine_run_id: Some("run-1".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some("idem-1".to_string()),
            receipt: Some(json!({"issue_number": 102})),
            error: None,
            metadata: None,
            created_at_ms: 20,
            updated_at_ms: 20,
        })
        .await
        .expect("dedupe external action");

    assert_eq!(first.action_id, "action-1");
    assert_eq!(second.action_id, "action-1");
    assert_eq!(state.list_external_actions(10).await.len(), 1);

    let updated = state.get_routine_run("run-1").await.expect("routine run");
    assert_eq!(updated.artifacts.len(), 1);
    assert_eq!(updated.artifacts[0].uri, "external-action://action-1");
}

#[tokio::test]
async fn record_external_action_without_idempotency_key_preserves_existing_behavior() {
    let state = test_state_with_path(tmp_resource_file("external-action-no-idem"));
    let run = RoutineRunRecord {
        run_id: "run-1".to_string(),
        routine_id: "routine-1".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Completed,
        created_at_ms: 1,
        updated_at_ms: 1,
        fired_at_ms: Some(1),
        started_at_ms: Some(1),
        finished_at_ms: Some(1),
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "workflow.publish".to_string(),
        args: Value::Null,
        allowed_tools: Vec::new(),
        output_targets: Vec::new(),
        artifacts: Vec::new(),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };
    state
        .routine_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    state
        .record_external_action(ExternalActionRecord {
            action_id: "action-1".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            routine_run_id: Some("run-1".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: None,
            receipt: Some(json!({"issue_number": 101})),
            error: None,
            metadata: None,
            created_at_ms: 10,
            updated_at_ms: 10,
        })
        .await
        .expect("record first external action");
    state
        .record_external_action(ExternalActionRecord {
            action_id: "action-2".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            routine_run_id: Some("run-1".to_string()),
            context_run_id: None,
            capability_id: Some("github.create_issue".to_string()),
            provider: Some("incident-monitor".to_string()),
            target: Some("acme/platform".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: None,
            receipt: Some(json!({"issue_number": 102})),
            error: None,
            metadata: None,
            created_at_ms: 20,
            updated_at_ms: 20,
        })
        .await
        .expect("record second external action");

    assert_eq!(state.list_external_actions(10).await.len(), 2);
    let updated = state.get_routine_run("run-1").await.expect("routine run");
    assert_eq!(updated.artifacts.len(), 2);
}

#[tokio::test]
async fn record_external_action_dedupes_under_concurrent_retries() {
    let state = test_state_with_path(tmp_resource_file("external-action-concurrent-dedupe"));
    let run = RoutineRunRecord {
        run_id: "run-1".to_string(),
        routine_id: "routine-1".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Completed,
        created_at_ms: 1,
        updated_at_ms: 1,
        fired_at_ms: Some(1),
        started_at_ms: Some(1),
        finished_at_ms: Some(1),
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "workflow.publish".to_string(),
        args: Value::Null,
        allowed_tools: Vec::new(),
        output_targets: Vec::new(),
        artifacts: Vec::new(),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };
    state
        .routine_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    let action_a = ExternalActionRecord {
        action_id: "action-a".to_string(),
        operation: "create_issue".to_string(),
        status: "posted".to_string(),
        source_kind: Some("incident_monitor".to_string()),
        source_id: Some("draft-1".to_string()),
        routine_run_id: Some("run-1".to_string()),
        context_run_id: None,
        capability_id: Some("github.create_issue".to_string()),
        provider: Some("incident-monitor".to_string()),
        target: Some("acme/platform".to_string()),
        approval_state: Some("executed".to_string()),
        idempotency_key: Some("idem-1".to_string()),
        receipt: Some(json!({"issue_number": 101})),
        error: None,
        metadata: None,
        created_at_ms: 10,
        updated_at_ms: 10,
    };
    let action_b = ExternalActionRecord {
        action_id: "action-b".to_string(),
        receipt: Some(json!({"issue_number": 102})),
        created_at_ms: 20,
        updated_at_ms: 20,
        ..action_a.clone()
    };

    let (first, second) = tokio::join!(
        state.record_external_action(action_a),
        state.record_external_action(action_b)
    );
    let first = first.expect("first concurrent action");
    let second = second.expect("second concurrent action");

    assert_eq!(first.action_id, "action-a");
    assert_eq!(second.action_id, "action-a");
    assert_eq!(state.list_external_actions(10).await.len(), 1);
    let updated = state.get_routine_run("run-1").await.expect("routine run");
    assert_eq!(updated.artifacts.len(), 1);
}

#[tokio::test]
async fn record_external_action_dedupes_under_retry_storm() {
    let state = test_state_with_path(tmp_resource_file("external-action-retry-storm"));
    let run = RoutineRunRecord {
        run_id: "run-1".to_string(),
        routine_id: "routine-1".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Completed,
        created_at_ms: 1,
        updated_at_ms: 1,
        fired_at_ms: Some(1),
        started_at_ms: Some(1),
        finished_at_ms: Some(1),
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "workflow.publish".to_string(),
        args: Value::Null,
        allowed_tools: Vec::new(),
        output_targets: Vec::new(),
        artifacts: Vec::new(),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };
    state
        .routine_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    let make_action = |action_id: &str, created_at_ms: u64| ExternalActionRecord {
        action_id: action_id.to_string(),
        operation: "create_issue".to_string(),
        status: "posted".to_string(),
        source_kind: Some("incident_monitor".to_string()),
        source_id: Some("draft-1".to_string()),
        routine_run_id: Some("run-1".to_string()),
        context_run_id: None,
        capability_id: Some("github.create_issue".to_string()),
        provider: Some("incident-monitor".to_string()),
        target: Some("acme/platform".to_string()),
        approval_state: Some("executed".to_string()),
        idempotency_key: Some("idem-storm".to_string()),
        receipt: Some(json!({"issue_number": created_at_ms})),
        error: None,
        metadata: None,
        created_at_ms,
        updated_at_ms: created_at_ms,
    };

    let (a, b, c, d) = tokio::join!(
        state.record_external_action(make_action("action-a", 10)),
        state.record_external_action(make_action("action-b", 20)),
        state.record_external_action(make_action("action-c", 30)),
        state.record_external_action(make_action("action-d", 40)),
    );

    let a = a.expect("storm action a");
    let b = b.expect("storm action b");
    let c = c.expect("storm action c");
    let d = d.expect("storm action d");

    assert_eq!(a.action_id, "action-a");
    assert_eq!(b.action_id, "action-a");
    assert_eq!(c.action_id, "action-a");
    assert_eq!(d.action_id, "action-a");
    assert_eq!(state.list_external_actions(10).await.len(), 1);
    let updated = state.get_routine_run("run-1").await.expect("routine run");
    assert_eq!(updated.artifacts.len(), 1);
}

#[tokio::test]
async fn claim_next_queued_routine_run_marks_oldest_running() {
    let mut state = AppState::new_starting("routine-claim".to_string(), true);
    state.routine_runs_path = tmp_routines_file("routine-claim-runs");

    let mk = |run_id: &str, created_at_ms: u64| RoutineRunRecord {
        run_id: run_id.to_string(),
        routine_id: "routine-claim".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Queued,
        created_at_ms,
        updated_at_ms: created_at_ms,
        fired_at_ms: Some(created_at_ms),
        started_at_ms: None,
        finished_at_ms: None,
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        artifacts: vec![],
        active_session_ids: vec![],
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };

    {
        let mut guard = state.routine_runs.write().await;
        guard.insert("run-late".to_string(), mk("run-late", 2_000));
        guard.insert("run-early".to_string(), mk("run-early", 1_000));
    }
    state.persist_routine_runs().await.expect("persist");

    let claimed = state
        .claim_next_queued_routine_run()
        .await
        .expect("claimed run");
    assert_eq!(claimed.run_id, "run-early");
    assert_eq!(claimed.status, RoutineRunStatus::Running);
    assert!(claimed.started_at_ms.is_some());
}

#[tokio::test]
async fn routine_session_policy_roundtrip_normalizes_tools() {
    let state = AppState::new_starting("routine-policy-hook".to_string(), true);
    state
        .set_routine_session_policy(
            "session-routine-1".to_string(),
            "run-1".to_string(),
            "routine-1".to_string(),
            vec![
                "read".to_string(),
                " mcp.arcade.search ".to_string(),
                "read".to_string(),
                "".to_string(),
            ],
        )
        .await;

    let policy = state
        .routine_session_policy("session-routine-1")
        .await
        .expect("policy");
    assert_eq!(
        policy.allowed_tools,
        vec!["read".to_string(), "mcp.arcade.search".to_string()]
    );
}

#[tokio::test]
async fn routine_run_preserves_latest_session_id_after_session_clears() {
    let state = AppState::new_starting("routine-latest-session".to_string(), true);
    let routine = RoutineSpec {
        routine_id: "routine-session-link".to_string(),
        name: "Routine Session Link".to_string(),
        status: RoutineStatus::Active,
        schedule: RoutineSchedule::IntervalSeconds { seconds: 300 },
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::Skip,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({}),
        allowed_tools: vec![],
        output_targets: vec![],
        creator_type: "user".to_string(),
        creator_id: "test".to_string(),
        requires_approval: false,
        external_integrations_allowed: false,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    let run = state
        .create_routine_run(&routine, "manual", 1, RoutineRunStatus::Queued, None)
        .await;
    state
        .add_active_session_id(&run.run_id, "session-123".to_string())
        .await
        .expect("active session added");
    state
        .clear_active_session_id(&run.run_id, "session-123")
        .await
        .expect("active session cleared");

    let updated = state
        .get_routine_run(&run.run_id)
        .await
        .expect("run exists");
    assert!(updated.active_session_ids.is_empty());
    assert_eq!(updated.latest_session_id.as_deref(), Some("session-123"));
}

#[test]
fn routine_mission_prompt_includes_orchestrated_contract() {
    let run = RoutineRunRecord {
        run_id: "run-orchestrated-1".to_string(),
        routine_id: "automation-orchestrated".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Queued,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
        fired_at_ms: Some(1_000),
        started_at_ms: None,
        finished_at_ms: None,
        requires_approval: true,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({
            "prompt": "Coordinate a multi-step release readiness check.",
            "mode": "orchestrated",
            "success_criteria": ["All blockers listed", "Output artifact written"],
            "orchestrator_only_tool_calls": true
        }),
        allowed_tools: vec!["read".to_string(), "webfetch".to_string()],
        output_targets: vec!["file://reports/release-readiness.md".to_string()],
        artifacts: vec![],
        active_session_ids: vec![],
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };

    let objective = crate::app::routines::routine_objective_from_args(&run).expect("objective");
    let prompt = crate::app::routines::build_routine_mission_prompt(&run, &objective);

    assert!(prompt.contains("Mode: orchestrated"));
    assert!(prompt.contains("Plan -> Do -> Verify -> Notify"));
    assert!(prompt.contains("only the orchestrator may execute tools"));
    assert!(prompt.contains("Allowed Tools: read, webfetch"));
    assert!(prompt.contains("file://reports/release-readiness.md"));
}

#[test]
fn routine_mission_prompt_includes_standalone_defaults() {
    let run = RoutineRunRecord {
        run_id: "run-standalone-1".to_string(),
        routine_id: "automation-standalone".to_string(),
        trigger_type: "manual".to_string(),
        run_count: 1,
        status: RoutineRunStatus::Queued,
        created_at_ms: 2_000,
        updated_at_ms: 2_000,
        fired_at_ms: Some(2_000),
        started_at_ms: None,
        finished_at_ms: None,
        requires_approval: false,
        approval_reason: None,
        denial_reason: None,
        paused_reason: None,
        detail: None,
        entrypoint: "mission.default".to_string(),
        args: serde_json::json!({
            "prompt": "Summarize top engineering updates.",
            "success_criteria": ["Three bullet summary"]
        }),
        allowed_tools: vec![],
        output_targets: vec![],
        artifacts: vec![],
        active_session_ids: vec![],
        latest_session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    };

    let objective = crate::app::routines::routine_objective_from_args(&run).expect("objective");
    let prompt = crate::app::routines::build_routine_mission_prompt(&run, &objective);

    assert!(prompt.contains("Mode: standalone"));
    assert!(prompt.contains("Execution Pattern: Standalone mission run"));
    assert!(prompt.contains("Allowed Tools: all available by current policy"));
    assert!(prompt.contains("Output Targets: none configured"));
}
