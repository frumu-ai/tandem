// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! TAN-707 production-surface proof for a long-lived Goal -> Plan -> Execute -> Verify loop.
//!
//! PR #1877 owns engine-driven workflow completion. Until it lands, this test advances only
//! that boundary explicitly; authoring, transitions, waits, recovery, replay, HTTP, and MCP
//! inspection all use the same durable production stores and public handlers as the server.

use super::*;

use crate::app::state::tests::AutomationSpecBuilder;
use crate::stateful_runtime::{
    automation_definition_snapshot_hash, begin_claimed_stateful_wait_wake_completion,
    claim_due_stateful_wait, claim_matching_stateful_webhook_wait,
    claim_stateful_wait_for_resolution, due_stateful_waits,
    finish_claimed_stateful_wait_completion, stateful_webhook_wait_metadata,
    upsert_stateful_tool_effect, upsert_stateful_wait, LegacyRuntimeMigrationPaths,
    OrchestrationStateStore, StatefulRunEventRecord, StatefulRuntimeScope,
    StatefulRuntimeStoragePaths, StatefulToolEffectRecord, StatefulToolEffectStatus,
    StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus, StatefulWebhookWaitEvent,
    StatefulWebhookWaitMatch, STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use tandem_automation::AutomationRunStatus;
use tandem_tools::Tool;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
const VIRTUAL_START_MS: u64 = 4_102_444_800_000; // 2100-01-01T00:00:00Z

#[derive(Clone, Copy)]
struct VirtualClock {
    now_ms: u64,
}

impl VirtualClock {
    fn day(day: u64) -> Self {
        Self {
            now_ms: VIRTUAL_START_MS + day * DAY_MS,
        }
    }
}

fn request(method: &str, uri: impl Into<String>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.into())
        .header("x-tandem-org-id", "local")
        .header("x-tandem-workspace-id", "local")
        .header("x-tandem-actor-id", "tan-707-operator");
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("TAN-707 request")
}

async fn dispatch(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("dispatch");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn seed_workflows(state: &AppState) -> (String, String, String) {
    let mut hashes = Vec::new();
    for automation_id in ["tan-707-plan", "tan-707-execute", "tan-707-verify"] {
        let spec = state
            .put_automation_v2(AutomationSpecBuilder::new(automation_id).build())
            .await
            .expect("seed proof workflow");
        hashes.push(automation_definition_snapshot_hash(&spec));
    }
    (hashes.remove(0), hashes.remove(0), hashes.remove(0))
}

fn orchestration_payload(plan_hash: &str, execute_hash: &str, verify_hash: &str) -> Value {
    json!({
        "orchestration_id": "tan-707-goal-loop",
        "name": "Goal Plan Execute Verify Replan",
        "root_node_id": "plan",
        "nodes": [
            {
                "node_id": "plan", "name": "Plan", "kind": "workflow",
                "automation_id": "tan-707-plan", "pinned_definition_hash": plan_hash,
                "allowed_transition_keys": ["planned"], "emits_artifact_types": ["plan"]
            },
            {
                "node_id": "execute", "name": "Execute", "kind": "workflow",
                "automation_id": "tan-707-execute", "pinned_definition_hash": execute_hash,
                "accepts_artifact_types": ["plan"],
                "allowed_transition_keys": ["executed"], "emits_artifact_types": ["result"]
            },
            {
                "node_id": "verify", "name": "Verify", "kind": "workflow",
                "automation_id": "tan-707-verify", "pinned_definition_hash": verify_hash,
                "accepts_artifact_types": ["result"],
                "allowed_transition_keys": ["complete", "replan"]
            },
            {"node_id": "complete", "name": "Complete", "kind": "terminal", "outcome": "complete"}
        ],
        "edges": [
            {
                "edge_id": "plan-execute", "from_node_id": "plan", "to_node_id": "execute",
                "transition_key": "planned", "artifact_contract": {"artifact_type": "plan", "required": true}
            },
            {
                "edge_id": "execute-verify", "from_node_id": "execute", "to_node_id": "verify",
                "transition_key": "executed", "artifact_contract": {"artifact_type": "result", "required": true}
            },
            {"edge_id": "verify-complete", "from_node_id": "verify", "to_node_id": "complete", "transition_key": "complete"},
            {"edge_id": "verify-replan", "from_node_id": "verify", "to_node_id": "plan", "transition_key": "replan"}
        ],
        "goal_policy": {
            "max_hops": 6,
            "deadline_at_ms": VirtualClock::day(180).now_ms,
            "max_total_tokens": 20000,
            "max_total_cost_usd": 5.0,
            "on_limit": "pause_for_review"
        },
        "metadata": {
            "proof": "TAN-707",
            "virtual_clock": {"start_ms": VIRTUAL_START_MS, "duration_days": 180},
            "runtime_completion_dependency": "PR #1877"
        }
    })
}

async fn publish(app: &Router, state: &AppState) {
    let (plan_hash, execute_hash, verify_hash) = seed_workflows(state).await;
    let (status, created) = dispatch(
        app,
        request(
            "POST",
            "/orchestrations",
            Some(orchestration_payload(
                &plan_hash,
                &execute_hash,
                &verify_hash,
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, published) = dispatch(
        app,
        request("POST", "/orchestrations/tan-707-goal-loop/publish", None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
}

fn enable_authoritative_runtime_store(state: &AppState) {
    let store = OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)
        .expect("open authoritative runtime store");
    let legacy_paths = LegacyRuntimeMigrationPaths::from_runtime_paths(
        state.automation_v2_runs_path.clone(),
        &state.runtime_events_path,
    );
    store
        .import_legacy_runtime_state(&legacy_paths, VIRTUAL_START_MS)
        .expect("initialize authoritative runtime store");
}

async fn restart_from_persisted_state(source: &AppState) -> AppState {
    let mut restarted = test_state().await;
    restarted.automations_v2_path = source.automations_v2_path.clone();
    restarted.automation_v2_runs_path = source.automation_v2_runs_path.clone();
    restarted.runtime_events_path = source.runtime_events_path.clone();
    restarted
        .load_automations_v2()
        .await
        .expect("reload persisted workflow definitions");
    restarted
        .load_automation_v2_runs()
        .await
        .expect("reload persisted workflow runs");
    restarted
}

async fn finish_claimed_wait(
    waits_path: &std::path::Path,
    claimed: &StatefulWaitRecord,
    idempotency_key: &str,
    event_seq: u64,
    clock: VirtualClock,
) -> StatefulWaitRecord {
    let tenant = TenantContext::local_implicit();
    let reserved = begin_claimed_stateful_wait_wake_completion(
        waits_path,
        &tenant,
        claimed,
        idempotency_key,
        clock.now_ms,
    )
    .await
    .expect("reserve wait completion")
    .expect("claimed wait remains authoritative");
    finish_claimed_stateful_wait_completion(
        waits_path,
        &tenant,
        &reserved,
        idempotency_key,
        event_seq,
        StatefulWaitStatus::Woken,
        clock.now_ms,
    )
    .await
    .expect("finish wait completion")
    .expect("wait completion persisted")
}

/// Test-only substitute for the engine completion callback owned by PR #1877.
async fn complete_run_at_test_boundary(state: &AppState, run_id: &str, clock: VirtualClock) {
    let mut run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("goal run exists");
    run.status = AutomationRunStatus::Completed;
    run.finished_at_ms = Some(clock.now_ms);
    run.updated_at_ms = clock.now_ms;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run.run_id.clone(), run.clone());
    OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)
        .expect("store")
        .upsert_automation_runs([&run])
        .expect("persist completed run");
}

fn wait(
    wait_id: &str,
    run_id: &str,
    kind: StatefulWaitKind,
    day: u64,
    metadata: Option<Value>,
) -> StatefulWaitRecord {
    let clock = VirtualClock::day(day);
    StatefulWaitRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        wait_id: wait_id.to_string(),
        run_id: run_id.to_string(),
        wait_kind: kind.clone(),
        status: StatefulWaitStatus::Waiting,
        scope: StatefulRuntimeScope::local_implicit(),
        phase_id: Some(format!("day-{day}")),
        reason: Some(format!("TAN-707 {kind:?} wait")),
        created_at_ms: clock.now_ms,
        updated_at_ms: clock.now_ms,
        wake_at_ms: matches!(kind, StatefulWaitKind::Timer).then_some(clock.now_ms),
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata,
    }
}

fn uncertain_effect(run_id: &str) -> StatefulToolEffectRecord {
    StatefulToolEffectRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        effect_id: "effect-day-120-release".to_string(),
        outbox_id: Some("outbox-day-120-release".to_string()),
        action_id: Some("notify-release".to_string()),
        run_id: Some(run_id.to_string()),
        scope: StatefulRuntimeScope::local_implicit(),
        status: StatefulToolEffectStatus::Unknown,
        operation: "mcp.release.notify".to_string(),
        source_kind: Some("automation_v2".to_string()),
        source_id: Some("tan-707-execute".to_string()),
        node_id: Some("notify".to_string()),
        provider: Some("mcp".to_string()),
        tool: Some("mcp.release.notify".to_string()),
        target: Some("release-channel".to_string()),
        external_resource: None,
        policy_decision_id: Some("policy-day-120".to_string()),
        context_assertion_id: Some("assertion-day-120".to_string()),
        input_digest: Some("sha256:tan707-input".to_string()),
        output_digest: None,
        receipt_payload_digest: None,
        receipt_payload_redacted: None,
        receipt_pointer: None,
        redaction_tier: "metadata_only".to_string(),
        audit_hash: "sha256:tan707-audit".to_string(),
        error: Some("provider acknowledged request but receipt was not observed".to_string()),
        compensation_id: None,
        created_at_ms: VirtualClock::day(120).now_ms,
        updated_at_ms: VirtualClock::day(120).now_ms,
        metadata: Some(json!({"effect_semantics": "uncertain", "virtual_day": 120})),
    }
}

async fn emit_transition(
    app: &Router,
    goal_id: &str,
    transition_key: &str,
    idempotency_key: &str,
    artifact_type: &str,
) -> Value {
    let (status, body) = dispatch(
        app,
        request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(json!({
                "transition_key": transition_key,
                "idempotency_key": idempotency_key,
                "artifact": {"artifact_type": artifact_type, "value": {"proof": "TAN-707"}}
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn graph_without_historical_run_statuses(graph: &Value) -> Value {
    let mut graph = graph.clone();
    for node in graph["nodes"].as_array_mut().into_iter().flatten() {
        for run in node["runs"].as_array_mut().into_iter().flatten() {
            run.as_object_mut().unwrap().remove("status");
        }
    }
    graph
}

fn budgets_without_wall_clock_countdown(budgets: &Value) -> Value {
    let mut budgets = budgets.clone();
    budgets["remaining"]
        .as_object_mut()
        .expect("remaining goal budgets")
        .remove("deadline_ms");
    budgets
}

#[tokio::test]
async fn tan_707_goal_plan_execute_verify_complete_survives_180_day_store_journey() {
    let directory = tempfile::tempdir().expect("proof directory");
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    state.runtime_events_path = directory.path().join("runtime_events.jsonl");
    enable_authoritative_runtime_store(&state);
    let app = app_router(state.clone());
    publish(&app, &state).await;

    let start_payload = json!({
        "orchestration_id": "tan-707-goal-loop",
        "objective": "Deliver and verify the 180-day program",
        "idempotency_key": "tan-707-start",
        "metadata": {"proof_clock_start_ms": VIRTUAL_START_MS}
    });
    let (status, started) =
        dispatch(&app, request("POST", "/goals", Some(start_payload.clone()))).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let plan_run = started["root_run_id"].as_str().unwrap().to_string();

    let (status, duplicate) = dispatch(&app, request("POST", "/goals", Some(start_payload))).await;
    assert_eq!(status, StatusCode::OK, "{duplicate}");
    assert_eq!(duplicate["replayed"], true);
    assert_eq!(duplicate["root_run_id"], plan_run);

    complete_run_at_test_boundary(&state, &plan_run, VirtualClock::day(1)).await;
    let planned = emit_transition(&app, &goal_id, "planned", "day-1-planned", "plan").await;
    let execute_run = planned["downstream_run_id"].as_str().unwrap().to_string();
    let duplicate_hop = emit_transition(&app, &goal_id, "planned", "day-1-planned", "plan").await;
    assert_eq!(duplicate_hop["commit"], "AlreadyCommitted");
    assert_eq!(duplicate_hop["downstream_run_id"], execute_run);

    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let webhook_metadata = stateful_webhook_wait_metadata(
        StatefulWebhookWaitMatch {
            trigger_id: Some("tan-707-release".to_string()),
            provider: Some("github".to_string()),
            provider_event_kind: Some("deployment_status".to_string()),
            provider_event_id: Some("deployment-707".to_string()),
            body_digest: Some("sha256:tan707-webhook".to_string()),
            idempotency_key: Some("github:deployment-707".to_string()),
        },
        None,
    );
    for record in [
        wait(
            "wait-day-30-timer",
            &execute_run,
            StatefulWaitKind::Timer,
            30,
            None,
        ),
        wait(
            "wait-day-60-approval",
            &execute_run,
            StatefulWaitKind::Approval,
            60,
            None,
        ),
        wait(
            "wait-day-90-webhook",
            &execute_run,
            StatefulWaitKind::Webhook,
            90,
            Some(webhook_metadata),
        ),
        wait(
            "wait-day-120-external",
            &execute_run,
            StatefulWaitKind::ExternalCondition,
            120,
            Some(json!({"condition": "release_receipt_present"})),
        ),
    ] {
        upsert_stateful_wait(&paths.waits_path, record)
            .await
            .expect("persist durable wait");
    }

    assert!(due_stateful_waits(
        &paths.waits_path,
        &TenantContext::local_implicit(),
        VirtualClock::day(29).now_ms,
        None,
    )
    .is_empty());
    let due = due_stateful_waits(
        &paths.waits_path,
        &TenantContext::local_implicit(),
        VirtualClock::day(30).now_ms,
        None,
    );
    assert_eq!(
        due.iter()
            .map(|row| row.wait_id.as_str())
            .collect::<Vec<_>>(),
        ["wait-day-30-timer"]
    );
    let claimed_timer = claim_due_stateful_wait(
        &paths.waits_path,
        &TenantContext::local_implicit(),
        &execute_run,
        "wait-day-30-timer",
        "scheduler-after-restart",
        VirtualClock::day(30).now_ms,
        DAY_MS,
    )
    .await
    .expect("claim timer")
    .expect("timer due");
    assert_eq!(claimed_timer.status, StatefulWaitStatus::Claimed);

    let claimed_webhook = claim_matching_stateful_webhook_wait(
        &paths.waits_path,
        &TenantContext::local_implicit(),
        &StatefulWebhookWaitEvent {
            trigger_id: "tan-707-release".to_string(),
            provider: "github".to_string(),
            provider_event_kind: Some("deployment_status".to_string()),
            provider_event_id: Some("deployment-707".to_string()),
            body_digest: "sha256:tan707-webhook".to_string(),
            idempotency_key: "github:deployment-707".to_string(),
        },
        "webhook-worker-after-restart",
        VirtualClock::day(90).now_ms,
        DAY_MS,
    )
    .await
    .expect("claim webhook")
    .expect("matching webhook");
    assert_eq!(claimed_webhook.wait_id, "wait-day-90-webhook");

    upsert_stateful_tool_effect(
        &crate::stateful_runtime::stateful_reliability_path_from_runtime_events_path(
            &state.runtime_events_path,
        ),
        uncertain_effect(&execute_run),
    )
    .await
    .expect("persist uncertain effect");

    let (status, waits) = dispatch(
        &app,
        request("GET", format!("/goals/{goal_id}/waits"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{waits}");
    assert_eq!(waits["count"], 4);
    let kinds = waits["waits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["wait_kind"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        kinds,
        ["timer", "approval", "webhook", "external_condition"]
            .into_iter()
            .collect()
    );

    let (status, recovery) = dispatch(
        &app,
        request(
            "GET",
            format!("/stateful-runtime/runs/{execute_run}/resume-plan"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recovery}");
    assert_eq!(recovery["audit_summary"]["uncertain_effect_count"], 1);
    assert_eq!(recovery["uncertain_effects"][0]["status"], "unknown");

    let tools = crate::http::orchestration_tools::orchestration_tools(state.clone());
    let goal_tool = tools
        .iter()
        .find(|tool| tool.schema().name == "goal_get")
        .unwrap();
    let goal_inspection = goal_tool
        .execute_for_tenant(json!({"goal_id": goal_id}), TenantContext::local_implicit())
        .await
        .expect("MCP goal inspection");
    assert_eq!(
        goal_inspection.metadata["run_links"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        goal_inspection.metadata["waits"].as_array().unwrap().len(),
        4
    );
    let wait_tool = tools
        .iter()
        .find(|tool| tool.schema().name == "wait_inspect")
        .unwrap();
    let wait_inspection = wait_tool
        .execute_for_tenant(
            json!({"goal_id": goal_id, "wait_id": "wait-day-120-external"}),
            TenantContext::local_implicit(),
        )
        .await
        .expect("MCP wait inspection");
    assert_eq!(
        wait_inspection.metadata["wait"]["wait_kind"],
        "external_condition"
    );

    let tenant = TenantContext::local_implicit();
    let claimed_approval = claim_stateful_wait_for_resolution(
        &paths.waits_path,
        &tenant,
        "wait-day-60-approval",
        StatefulWaitKind::Approval,
        "approval-resolver-after-restart",
        VirtualClock::day(60).now_ms,
        DAY_MS,
    )
    .await
    .expect("claim approval")
    .expect("approval remains resolvable");
    let claimed_external = claim_stateful_wait_for_resolution(
        &paths.waits_path,
        &tenant,
        "wait-day-120-external",
        StatefulWaitKind::ExternalCondition,
        "external-resolver-after-restart",
        VirtualClock::day(120).now_ms,
        DAY_MS,
    )
    .await
    .expect("claim external condition")
    .expect("external condition remains resolvable");
    for (claimed, key, event_seq, day) in [
        (&claimed_timer, "tan-707-timer-wake", 30, 30),
        (&claimed_approval, "tan-707-approval-wake", 60, 60),
        (&claimed_webhook, "tan-707-webhook-wake", 90, 90),
        (&claimed_external, "tan-707-external-wake", 120, 120),
    ] {
        finish_claimed_wait(
            &paths.waits_path,
            claimed,
            key,
            event_seq,
            VirtualClock::day(day),
        )
        .await;
    }
    let (status, settled_waits) = dispatch(
        &app,
        request("GET", format!("/goals/{goal_id}/waits"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settled_waits}");
    assert_eq!(settled_waits["count"], 4);
    let settled_wait_ids = settled_waits["waits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["wait_id"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        settled_wait_ids,
        [
            "wait-day-30-timer",
            "wait-day-60-approval",
            "wait-day-90-webhook",
            "wait-day-120-external",
        ]
        .into_iter()
        .collect()
    );
    assert!(settled_waits["waits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["status"] == "woken"));

    complete_run_at_test_boundary(&state, &execute_run, VirtualClock::day(120)).await;
    let executed = emit_transition(&app, &goal_id, "executed", "day-120-executed", "result").await;
    let verify_run = executed["downstream_run_id"].as_str().unwrap().to_string();
    OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)
        .unwrap()
        .append_stateful_runtime_event_once_with_next_seq(&StatefulRunEventRecord {
            schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
            event_id: "tan-707-day-179-projection-checkpoint".to_string(),
            run_id: verify_run.clone(),
            seq: 0,
            event_type: "tan_707.verify.started".to_string(),
            occurred_at_ms: VirtualClock::day(179).now_ms,
            scope: StatefulRuntimeScope::local_implicit(),
            actor: None,
            phase_id: Some("verify".to_string()),
            phase_transition: None,
            wait_kind: None,
            causation_id: Some("day-120-executed".to_string()),
            correlation_id: Some(goal_id.clone()),
            payload: json!({"goal_id": goal_id, "virtual_day": 179}),
        })
        .expect("append deterministic projection checkpoint");

    let (status, live_before_complete) = dispatch(
        &app,
        request("GET", format!("/goals/{goal_id}/projection"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{live_before_complete}");
    assert_eq!(live_before_complete["mode"], "live");
    assert_eq!(live_before_complete["goal"]["current_node_id"], "verify");
    let replay_cursor = live_before_complete["cursor"].as_i64().unwrap();

    // Reloading a fresh AppState from the durable paths models a process restart.
    drop(
        OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path).unwrap(),
    );
    let restarted_state = restart_from_persisted_state(&state).await;
    let restarted_app = app_router(restarted_state.clone());
    let (status, replay) = dispatch(
        &restarted_app,
        request(
            "GET",
            format!("/goals/{goal_id}/projection?cursor={replay_cursor}"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["mode"], "replay");
    assert_eq!(replay["historical_state"]["exact"], true);
    for key in ["goal", "workflow", "waits", "handoffs"] {
        assert_eq!(
            replay[key], live_before_complete[key],
            "live/replay mismatch at {key}"
        );
    }
    assert_eq!(
        budgets_without_wall_clock_countdown(&replay["budgets"]),
        budgets_without_wall_clock_countdown(&live_before_complete["budgets"]),
        "live/replay budgets must match apart from the time-relative deadline countdown"
    );
    assert_eq!(
        graph_without_historical_run_statuses(&replay["graph"]),
        graph_without_historical_run_statuses(&live_before_complete["graph"]),
        "historical replay must preserve canonical graph topology and lineage"
    );

    complete_run_at_test_boundary(&restarted_state, &verify_run, VirtualClock::day(180)).await;
    let (status, completed) = dispatch(
        &restarted_app,
        request(
            "POST",
            format!("/goals/{goal_id}/completion"),
            Some(json!({
                "transition_key": "complete",
                "final_artifact": {"artifact_type": "verification", "value": {"outcome": "complete", "virtual_day": 180}}
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{completed}");
    assert_eq!(completed["outcome"], "terminal");
    assert_eq!(completed["goal"]["status"], "completed");

    let (status, runs) = dispatch(
        &restarted_app,
        request("GET", format!("/goals/{goal_id}/runs"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{runs}");
    assert_eq!(runs["count"], 3);
    assert_eq!(runs["runs"][0]["link"]["hop_index"], 0);
    assert_eq!(runs["runs"][2]["link"]["hop_index"], 2);

    let (status, budgets) = dispatch(
        &restarted_app,
        request("GET", format!("/goals/{goal_id}/budgets"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{budgets}");
    assert_eq!(budgets["budgets"]["consumed"]["hops"], 2);
    assert_eq!(budgets["budgets"]["remaining"]["hops"], 4);
    assert_eq!(budgets["budgets"]["policy"]["max_total_tokens"], 20000);
    assert_eq!(budgets["budgets"]["policy"]["max_total_cost_usd"], 5.0);
}

#[tokio::test]
async fn tan_707_named_replan_edge_and_day_180_limit_are_explicit() {
    let directory = tempfile::tempdir().expect("proof directory");
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    state.runtime_events_path = directory.path().join("runtime_events.jsonl");
    enable_authoritative_runtime_store(&state);
    let app = app_router(state.clone());
    publish(&app, &state).await;

    for (transition_key, target) in [("complete", "complete"), ("replan", "plan")] {
        let (status, preview) = dispatch(
            &app,
            request(
                "POST",
                "/orchestrations/tan-707-goal-loop/dry-run",
                Some(json!({
                    "from_node_id": "verify",
                    "transition_key": transition_key,
                    "version": 1
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        assert_eq!(preview["allowed"], true);
        assert_eq!(preview["target"]["node_id"], target);
    }
    let (status, published) = dispatch(
        &app,
        request("GET", "/orchestrations/tan-707-goal-loop/versions/1", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let complete_node = published["orchestration"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == "complete")
        .unwrap();
    assert_eq!(complete_node["outcome"], "complete");

    let (status, started) = dispatch(
        &app,
        request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "tan-707-goal-loop",
                "objective": "Prove the named replan route",
                "idempotency_key": "tan-707-replan-start"
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let plan_run = started["root_run_id"].as_str().unwrap().to_string();
    complete_run_at_test_boundary(&state, &plan_run, VirtualClock::day(1)).await;
    let execute = emit_transition(&app, &goal_id, "planned", "replan-planned", "plan").await;
    let execute_run = execute["downstream_run_id"].as_str().unwrap().to_string();
    complete_run_at_test_boundary(&state, &execute_run, VirtualClock::day(90)).await;
    let verify = emit_transition(&app, &goal_id, "executed", "replan-executed", "result").await;
    let verify_run = verify["downstream_run_id"].as_str().unwrap().to_string();
    complete_run_at_test_boundary(&state, &verify_run, VirtualClock::day(179)).await;
    let replanned =
        emit_transition(&app, &goal_id, "replan", "day-179-replan", "verification").await;
    assert_eq!(replanned["goal"]["current_node_id"], "plan");
    assert_eq!(replanned["goal"]["hop_count"], 3);
    let replanned_run = replanned["downstream_run_id"].as_str().unwrap();
    complete_run_at_test_boundary(&state, replanned_run, VirtualClock::day(180)).await;

    let store =
        OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path).unwrap();
    let mut stored = store.get_goal(&goal_id).unwrap().unwrap();
    let model_limit = stored.admit_transition(VirtualClock::day(180).now_ms, 0, 0.0);
    assert!(!model_limit.allowed);
    assert_eq!(
        model_limit.limit,
        Some(tandem_automation::GoalPolicyLimit::Deadline)
    );
    assert_eq!(
        model_limit.resulting_status,
        Some(tandem_automation::LongRunningGoalStatus::Expired)
    );

    // The HTTP surface uses the server clock. Move the equivalent deadline just
    // behind it, then prove the production transition path enforces the policy.
    stored.policy.deadline_at_ms = Some(crate::now_ms().saturating_sub(1));
    store.put_goal(&stored).expect("persist expired deadline");
    let (status, rejected) = dispatch(
        &app,
        request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(json!({
                "transition_key": "planned",
                "idempotency_key": "day-180-policy-rejection",
                "artifact": {"artifact_type": "plan", "value": {"virtual_day": 180}}
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    assert_eq!(rejected["error"], "invalid_goal_request");
    assert!(rejected["detail"].as_str().unwrap().contains("Deadline"));
    let expired = store.get_goal(&goal_id).unwrap().unwrap();
    assert_eq!(
        expired.status,
        tandem_automation::LongRunningGoalStatus::Expired
    );
    assert!(expired.active_run_id.is_none());
}
