use super::*;
use crate::automation_v2::types::{
    AutomationHandoffConfig, AutomationV2Schedule, AutomationV2ScheduleType, AutomationV2Status,
    HandoffArtifact, WatchCondition,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_automation(id: &str, workspace_root: &str) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
        automation_id: id.to_string(),
        name: id.to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::Skip,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![],
        flow: crate::automation_v2::types::AutomationFlowSpec { nodes: vec![] },
        execution: crate::automation_v2::types::AutomationExecutionPolicy {
            max_parallel_agents: None,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec![],
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: vec![],
        handoff_config: None,
    }
}

fn make_handoff(handoff_id: &str, source: &str, target: &str, atype: &str) -> HandoffArtifact {
    HandoffArtifact {
        handoff_id: handoff_id.to_string(),
        source_automation_id: source.to_string(),
        source_run_id: "run-src-1".to_string(),
        source_node_id: "node-src-1".to_string(),
        target_automation_id: target.to_string(),
        artifact_type: atype.to_string(),
        created_at_ms: 1_000,
        content_path: None,
        content_digest: None,
        metadata: Some(serde_json::json!({ "detail": "test" })),
        consumed_by_run_id: None,
        consumed_by_automation_id: None,
        consumed_at_ms: None,
    }
}

fn tmp_workspace(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tandem-handoff-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create workspace");
    dir
}

// ── deposit ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn handoff_auto_approve_deposits_to_approved_dir() {
    let ws = tmp_workspace("deposit-approve");
    let state = test_state_with_path(tmp_resource_file("hf-deposit-approve"));
    let cfg = AutomationHandoffConfig {
        auto_approve: true,
        ..AutomationHandoffConfig::default()
    };
    let handoff = make_handoff("hf-aa-001", "scout", "job-search", "lead");

    state
        .deposit_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg)
        .await
        .expect("deposit");

    assert!(ws.join("shared/handoffs/approved/hf-aa-001.json").exists());
    assert!(!ws.join("shared/handoffs/inbox/hf-aa-001.json").exists());
    let _ = std::fs::remove_dir_all(&ws);
}

#[tokio::test]
async fn handoff_manual_approve_deposits_to_inbox() {
    let ws = tmp_workspace("deposit-inbox");
    let state = test_state_with_path(tmp_resource_file("hf-deposit-inbox"));
    let cfg = AutomationHandoffConfig {
        auto_approve: false,
        ..AutomationHandoffConfig::default()
    };
    let handoff = make_handoff("hf-ma-001", "scout", "job-search", "lead");

    state
        .deposit_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg)
        .await
        .expect("deposit");

    assert!(ws.join("shared/handoffs/inbox/hf-ma-001.json").exists());
    assert!(!ws.join("shared/handoffs/approved/hf-ma-001.json").exists());
    let _ = std::fs::remove_dir_all(&ws);
}

// ── consume ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn handoff_consume_stamps_metadata_and_moves_to_archived() {
    let ws = tmp_workspace("consume");
    let state = test_state_with_path(tmp_resource_file("hf-consume"));
    let cfg = AutomationHandoffConfig::default(); // auto_approve = true
    let handoff = make_handoff("hf-con-001", "scout", "job-search", "lead");

    state
        .deposit_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg)
        .await
        .expect("deposit");

    let consumed = state
        .consume_automation_v2_handoff(
            ws.to_str().unwrap(),
            &handoff,
            &cfg,
            "run-consumer-1",
            "job-search",
        )
        .await
        .expect("consume")
        .expect("should be Some");

    assert!(!ws.join("shared/handoffs/approved/hf-con-001.json").exists());
    assert!(ws.join("shared/handoffs/archived/hf-con-001.json").exists());

    assert_eq!(
        consumed.consumed_by_run_id.as_deref(),
        Some("run-consumer-1")
    );
    assert_eq!(
        consumed.consumed_by_automation_id.as_deref(),
        Some("job-search")
    );
    assert!(consumed.consumed_at_ms.is_some());

    let _ = std::fs::remove_dir_all(&ws);
}

#[tokio::test]
async fn handoff_consume_is_idempotent_on_second_call() {
    let ws = tmp_workspace("idempotent");
    let state = test_state_with_path(tmp_resource_file("hf-idempotent"));
    let cfg = AutomationHandoffConfig::default();
    let handoff = make_handoff("hf-idem-001", "scout", "job-search", "lead");

    state
        .deposit_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg)
        .await
        .expect("deposit");
    state
        .consume_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg, "run-1", "job-search")
        .await
        .expect("first consume");

    let second = state
        .consume_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg, "run-2", "job-search")
        .await
        .expect("second consume");
    assert!(
        second.is_none(),
        "second consume should return None (race-safe)"
    );

    let _ = std::fs::remove_dir_all(&ws);
}

// ── handoff file content round-trip ──────────────────────────────────────────

#[tokio::test]
async fn handoff_deposit_writes_valid_json_with_correct_fields() {
    let ws = tmp_workspace("roundtrip");
    let state = test_state_with_path(tmp_resource_file("hf-roundtrip"));
    let cfg = AutomationHandoffConfig::default();
    let handoff = make_handoff("hf-rt-001", "opportunity-scout", "job-search", "lead");

    state
        .deposit_automation_v2_handoff(ws.to_str().unwrap(), &handoff, &cfg)
        .await
        .expect("deposit");

    let path = ws.join("shared/handoffs/approved/hf-rt-001.json");
    let content = std::fs::read_to_string(&path).expect("read file");
    let parsed: HandoffArtifact = serde_json::from_str(&content).expect("valid JSON");

    assert_eq!(parsed.handoff_id, "hf-rt-001");
    assert_eq!(parsed.source_automation_id, "opportunity-scout");
    assert_eq!(parsed.target_automation_id, "job-search");
    assert_eq!(parsed.artifact_type, "lead");
    assert!(parsed.consumed_by_run_id.is_none());

    let _ = std::fs::remove_dir_all(&ws);
}

// ── watch evaluation ──────────────────────────────────────────────────────────
// evaluate_automation_v2_watches returns Vec<(automation_id, trigger_reason, Option<HandoffArtifact>)>
// It uses workspace_index.snapshot().root to locate the approved/ dir.
// For these tests we use ready_test_state (which sets root to ".") and place
// the handoff file relative to that root so the evaluator can find it.

#[tokio::test]
async fn watch_evaluation_triggers_consumer_when_handoff_available() {
    let state = ready_test_state().await;

    // Resolve the evaluator's workspace root (whatever the index reports)
    let eval_root = state.workspace_index.snapshot().await.root;
    let eval_root = std::path::PathBuf::from(&eval_root);

    // Seed the consumer automation with a watch condition
    let mut job_search = make_automation("job-search-wt", &eval_root.to_string_lossy());
    job_search.watch_conditions = vec![WatchCondition::HandoffAvailable {
        source_automation_id: Some("opportunity-scout-wt".to_string()),
        artifact_type: Some("lead".to_string()),
    }];
    state
        .put_automation_v2(job_search)
        .await
        .expect("put job-search");

    // Build the handoff and write it directly to the evaluator's approved/ dir
    // (bypassing AppState.deposit so we control the path precisely)
    let cfg = AutomationHandoffConfig::default();
    let handoff = make_handoff("hf-wt-001", "opportunity-scout-wt", "job-search-wt", "lead");
    let approved_dir = eval_root.join(&cfg.approved_dir);
    std::fs::create_dir_all(&approved_dir).expect("approved dir");
    std::fs::write(
        approved_dir.join("hf-wt-001.json"),
        serde_json::to_string_pretty(&handoff).expect("serialize"),
    )
    .expect("write handoff");

    let triggered = state.evaluate_automation_v2_watches().await;

    // Find the trigger for job-search-wt
    let found = triggered
        .iter()
        .find(|(automation_id, _, _)| automation_id == "job-search-wt");

    assert!(
        found.is_some(),
        "job-search-wt should be triggered; got: {triggered:?}"
    );
    let (_, reason, handoff_artifact) = found.unwrap();
    assert!(
        reason.contains("opportunity-scout-wt"),
        "trigger_reason should mention source: {reason}"
    );
    assert!(
        reason.contains("hf-wt-001"),
        "trigger_reason should mention handoff id: {reason}"
    );
    assert!(
        handoff_artifact.is_some(),
        "handoff artifact should be returned"
    );
    assert_eq!(handoff_artifact.as_ref().unwrap().handoff_id, "hf-wt-001");

    // Cleanup
    let _ = std::fs::remove_file(approved_dir.join("hf-wt-001.json"));
}

#[tokio::test]
async fn watch_evaluation_does_not_trigger_when_no_handoff_present() {
    let state = ready_test_state().await;
    let eval_root = state.workspace_index.snapshot().await.root;

    let mut job_search = make_automation("job-search-empty", &eval_root);
    job_search.watch_conditions = vec![WatchCondition::HandoffAvailable {
        source_automation_id: Some("opportunity-scout-empty".to_string()),
        artifact_type: None,
    }];
    state.put_automation_v2(job_search).await.expect("put");

    // No handoff deposited
    let triggered = state.evaluate_automation_v2_watches().await;
    let found = triggered.iter().any(|(id, _, _)| id == "job-search-empty");
    assert!(!found, "should not trigger when no handoff in approved/");
}

#[tokio::test]
async fn watch_evaluation_filters_by_source_automation_id() {
    let state = ready_test_state().await;
    let eval_root = state.workspace_index.snapshot().await.root;
    let eval_root = std::path::PathBuf::from(&eval_root);

    // Consumer only accepts handoffs from "correct-source"
    let mut consumer = make_automation("consumer-src-filter", &eval_root.to_string_lossy());
    consumer.watch_conditions = vec![WatchCondition::HandoffAvailable {
        source_automation_id: Some("correct-source".to_string()),
        artifact_type: None,
    }];
    state.put_automation_v2(consumer).await.expect("put");

    // Deposit a handoff from a DIFFERENT source
    let cfg = AutomationHandoffConfig::default();
    let wrong_handoff = make_handoff(
        "hf-src-filter-001",
        "wrong-source",
        "consumer-src-filter",
        "lead",
    );
    let approved_dir = eval_root.join(&cfg.approved_dir);
    std::fs::create_dir_all(&approved_dir).expect("approved dir");
    std::fs::write(
        approved_dir.join("hf-src-filter-001.json"),
        serde_json::to_string_pretty(&wrong_handoff).expect("serialize"),
    )
    .expect("write");

    let triggered = state.evaluate_automation_v2_watches().await;
    let found = triggered
        .iter()
        .any(|(id, _, _)| id == "consumer-src-filter");
    assert!(
        !found,
        "should not trigger: source filter should exclude wrong-source"
    );

    let _ = std::fs::remove_file(approved_dir.join("hf-src-filter-001.json"));
}

// ── scope policy integration guard ───────────────────────────────────────────

#[test]
fn scope_policy_integration_guards_cross_agent_path_access() {
    use crate::automation_v2::types::AutomationScopePolicy;

    // job-search may only read shared/ and its own directory; write restricted
    let policy = AutomationScopePolicy {
        readable_paths: vec!["shared/".to_string(), "job-search/".to_string()],
        writable_paths: vec!["job-search/reports/".to_string()],
        denied_paths: vec![],
        watch_paths: vec![],
    };

    assert!(policy
        .check_read("shared/handoffs/approved/hf.json")
        .is_ok());
    assert!(policy.check_read("job-search/leads.json").is_ok());
    assert!(policy.check_write("job-search/reports/week1.md").is_ok());

    // Cannot read a different agent's workspace
    assert!(policy.check_read("opportunity-scout/raw.json").is_err());
    // Cannot write into shared/
    assert!(policy
        .check_write("shared/handoffs/approved/hijack.json")
        .is_err());
}
