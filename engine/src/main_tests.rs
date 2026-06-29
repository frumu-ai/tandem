use super::*;
use chrono::Utc;
use serde_json::json;
use tandem_memory::db::MemoryDatabase;

#[test]
fn build_cli_overrides_targets_selected_provider() {
    let overrides = build_cli_overrides(
        Some("sk-test".to_string()),
        Some("openrouter".to_string()),
        Some("google/gemini-2.5-flash".to_string()),
    )
    .expect("overrides")
    .expect("some");

    assert_eq!(overrides["default_provider"], "openrouter");
    assert_eq!(
        overrides["providers"]["openrouter"]["api_key"],
        json!("sk-test")
    );
    assert_eq!(
        overrides["providers"]["openrouter"]["default_model"],
        json!("google/gemini-2.5-flash")
    );
}

#[test]
fn build_cli_overrides_defaults_model_and_key_to_openai_without_provider() {
    let overrides = build_cli_overrides(
        Some("sk-test".to_string()),
        None,
        Some("gpt-4o-mini".to_string()),
    )
    .expect("overrides")
    .expect("some");

    assert!(overrides.get("default_provider").is_none());
    assert_eq!(
        overrides["providers"]["openai"]["api_key"],
        json!("sk-test")
    );
    assert_eq!(
        overrides["providers"]["openai"]["default_model"],
        json!("gpt-4o-mini")
    );
}

#[test]
fn normalize_and_validate_provider_accepts_known_values_case_insensitive() {
    let provider =
        normalize_and_validate_provider(Some(" OpenRouter ".to_string())).expect("provider");
    assert_eq!(provider.as_deref(), Some("openrouter"));
}

#[test]
fn normalize_and_validate_provider_accepts_custom_values() {
    let provider =
        normalize_and_validate_provider(Some(" MiniMax ".to_string())).expect("provider");
    assert_eq!(provider.as_deref(), Some("minimax"));
}

#[test]
fn build_cli_overrides_accepts_custom_provider() {
    let overrides = build_cli_overrides(
        Some("sk-test".to_string()),
        Some("minimax".to_string()),
        Some("MiniMax-M2".to_string()),
    )
    .expect("overrides")
    .expect("some");

    assert_eq!(overrides["default_provider"], "minimax");
    assert_eq!(
        overrides["providers"]["minimax"]["api_key"],
        json!("sk-test")
    );
    assert_eq!(
        overrides["providers"]["minimax"]["default_model"],
        json!("MiniMax-M2")
    );
}

#[tokio::test]
async fn cleanup_default_knowledge_storage_removes_seed_rows_and_state_files() {
    let root = std::env::temp_dir().join(format!("tandem-default-knowledge-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("data").join("knowledge")).expect("create temp root");

    let db_path = root.join("memory.sqlite");
    let db = MemoryDatabase::new(&db_path).await.expect("open memory db");
    let chunk = tandem_memory::types::MemoryChunk {
        id: "guide-doc-1".to_string(),
        content: "Guide docs seed content".to_string(),
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        source: "guide_docs:seed.md".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: Some("seed-hash".to_string()),
        created_at: Utc::now(),
        token_count: 4,
        metadata: None,
        tenant_scope: tandem_memory::types::MemoryTenantScope::local(),
    };
    let embedding = vec![0.0f32; tandem_memory::types::DEFAULT_EMBEDDING_DIMENSION];
    db.store_chunk(&chunk, &embedding)
        .await
        .expect("store chunk");

    fs::write(root.join("default_knowledge_state.json"), "{ }\n").expect("write root state file");
    fs::write(
        root.join("data")
            .join("knowledge")
            .join("default_knowledge_state.json"),
        "{ }\n",
    )
    .expect("write canonical state file");

    let report =
        cleanup_default_knowledge_storage(&root, Some(db_path.as_path()), None, false, false)
            .await
            .expect("cleanup");

    assert_eq!(report.state_files_matched, 2);
    assert_eq!(report.rows_cleared, 1);
    assert!(!root.join("default_knowledge_state.json").exists());
    assert!(!root
        .join("data")
        .join("knowledge")
        .join("default_knowledge_state.json")
        .exists());

    let reopened = MemoryDatabase::new(&db_path)
        .await
        .expect("reopen memory db");
    let cleared = reopened
        .clear_global_memory_by_source_prefix(DEFAULT_KNOWLEDGE_SOURCE_PREFIX)
        .await
        .expect("verify cleared rows");
    assert_eq!(cleared, 0);

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn storage_cleanup_preserves_versioned_automation_run_hot_index() {
    let root = std::env::temp_dir().join(format!("tandem-storage-runs-v1-{}", Uuid::new_v4()));
    let data_dir = root.join("data");
    fs::create_dir_all(&data_dir).expect("create data dir");
    let active_path = data_dir.join("automation_v2_runs.json");
    let run = cleanup_test_automation_run(
        "run-cleanup-hot",
        tandem_server::AutomationRunStatus::Queued,
    );
    let runs = std::collections::HashMap::from([(run.run_id.clone(), run.clone())]);
    let payload = json!({
        "schema_version": 1,
        "runs": runs,
    });
    fs::write(
        &active_path,
        serde_json::to_string_pretty(&payload).expect("versioned run json"),
    )
    .expect("write active run index");

    let _report = storage_cleanup(&root, false, false, true, false, false, 7)
        .await
        .expect("storage cleanup");

    let rewritten: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&active_path).expect("read active runs"))
            .expect("rewritten active runs");
    assert_eq!(
        rewritten
            .get("schema_version")
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert!(rewritten
        .get("runs")
        .and_then(|runs| runs.get("run-cleanup-hot"))
        .is_some());

    let shard_path = storage_run_shard_path(&active_path, &run);
    let shard: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(shard_path).expect("read run shard"))
            .expect("run shard json");
    assert_eq!(
        shard.get("schema_version").and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(shard["run"]["run_id"], "run-cleanup-hot");

    let _ = fs::remove_dir_all(&root);
}

fn cleanup_test_automation_run(
    run_id: &str,
    status: tandem_server::AutomationRunStatus,
) -> tandem_server::AutomationV2RunRecord {
    tandem_server::AutomationV2RunRecord {
        run_id: run_id.to_string(),
        automation_id: "cleanup-auto".to_string(),
        tenant_context: tandem_types::TenantContext::local_implicit(),
        trigger_type: "manual".to_string(),
        status,
        created_at_ms: 1,
        updated_at_ms: 1,
        started_at_ms: None,
        finished_at_ms: None,
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint: tandem_server::AutomationRunCheckpoint {
            completed_nodes: Vec::new(),
            pending_nodes: vec!["draft".to_string()],
            node_outputs: std::collections::HashMap::new(),
            node_attempts: std::collections::HashMap::new(),
            node_attempt_verdicts: std::collections::HashMap::new(),
            blocked_nodes: Vec::new(),
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: Vec::new(),
            last_failure: None,
        },
        runtime_context: None,
        automation_snapshot: None,
        execution_claim: None,
        execution_claim_epoch: 0,
        pause_reason: None,
        resume_reason: None,
        detail: None,
        stop_kind: None,
        stop_reason: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
        scheduler: None,
        trigger_reason: None,
        consumed_handoff_id: None,
        learning_summary: None,
        effective_execution_profile: tandem_server::ExecutionProfile::Strict,
        requested_execution_profile: None,
    }
}

#[test]
fn parse_memory_import_format_accepts_openclaw() {
    assert_eq!(
        parse_memory_import_format("OpenClaw").unwrap(),
        MemoryImportFormat::Openclaw
    );
}

#[test]
fn parse_memory_import_tier_accepts_global() {
    assert_eq!(
        parse_memory_import_tier("global").unwrap(),
        MemoryTier::Global
    );
}
