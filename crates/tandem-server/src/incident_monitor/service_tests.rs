use super::*;
use serde_json::json;

fn event_with(properties: Value) -> EngineEvent {
    EngineEvent::new("automation_v2.run.failed", properties)
}

#[test]
fn recursive_triage_skip_reason_detects_triage_automation_id_prefix() {
    let event = event_with(json!({
        "automation_id": "automation-v2-incident-monitor-triage-failure-draft-abc123",
        "agent_role": "agent_writer",
    }));
    let reason = recursive_triage_skip_reason(&event)
        .expect("triage automation_id prefix should trigger skip");
    assert!(reason.contains("automation-v2-incident-monitor-triage-"));
}

#[test]
fn recursive_triage_skip_reason_detects_workflow_id_alias() {
    // Some events use `workflow_id` instead of `automation_id`.
    let event = event_with(json!({
        "workflow_id": "automation-v2-incident-monitor-triage-failure-draft-xyz",
    }));
    assert!(recursive_triage_skip_reason(&event).is_some());
}

#[test]
fn recursive_triage_skip_reason_detects_triage_agent_role_when_id_missing() {
    let event = event_with(json!({
        "agent_role": "incident_monitor_triage_agent",
    }));
    let reason =
        recursive_triage_skip_reason(&event).expect("triage agent_role should trigger skip");
    assert!(reason.contains("incident_monitor_triage_agent"));
}

#[test]
fn recursive_triage_skip_reason_passes_normal_workflow_failures() {
    let event = event_with(json!({
        "automation_id": "automation-v2-9ee33834-bf6d-4f86-acb3-3cd41d9cef19",
        "agent_role": "agent_reddit_query_researcher",
    }));
    assert!(recursive_triage_skip_reason(&event).is_none());
}

/// Regression for the P2 Codex review on PR #53. If a user's
/// custom automation happens to use `incident_monitor_triage_agent`
/// as its agent_id string, the agent_role backstop must NOT
/// silently filter out its failures â€” the automation_id is
/// present and doesn't have the triage prefix, so this is a
/// real workflow failure and should be triaged normally.
#[test]
fn recursive_triage_skip_reason_does_not_fire_when_automation_id_is_real() {
    let event = event_with(json!({
        "automation_id": "automation-v2-9ee33834-bf6d-4f86-acb3-3cd41d9cef19",
        "agent_role": "incident_monitor_triage_agent",
    }));
    assert!(recursive_triage_skip_reason(&event).is_none());
}

#[test]
fn recursive_triage_skip_reason_handles_empty_properties() {
    let event = event_with(json!({}));
    assert!(recursive_triage_skip_reason(&event).is_none());
}

#[test]
fn normalize_reason_replaces_automation_run_id_in_artifact_path() {
    let reason = "required output `.tandem/runs/automation-v2-run-593051dc-78bf-4927-b7db-b831b81d8bdd/artifacts/collect-recent-files.json` was not created for node `collect_recent_files`";
    let normalized = normalize_reason_for_fingerprint(reason);
    assert!(
        normalized.contains("automation-v2-run-RUNID"),
        "expected RUNID placeholder, got: {normalized}"
    );
    assert!(
        !normalized.contains("593051dc"),
        "leftover run uuid: {normalized}"
    );
}

#[test]
fn normalize_reason_collapses_recurrences_to_same_fingerprint() {
    // Two reason strings from successive runs of the same node
    // failure â€” only the embedded run UUID differs.
    let r1 = "required output `.tandem/runs/automation-v2-run-593051dc-78bf-4927-b7db-b831b81d8bdd/artifacts/collect-recent-files.json` was not created for node `collect_recent_files`";
    let r2 = "required output `.tandem/runs/automation-v2-run-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/artifacts/collect-recent-files.json` was not created for node `collect_recent_files`";
    assert_eq!(
        normalize_reason_for_fingerprint(r1),
        normalize_reason_for_fingerprint(r2),
    );
}

#[test]
fn normalize_reason_preserves_numeric_values() {
    // 180000 vs 600000 are genuinely different failure shapes â€”
    // do not collapse them.
    let r1 = "automation node `prepare_search_manifest` timed out after 180000 ms";
    let r2 = "automation node `prepare_search_manifest` timed out after 600000 ms";
    assert_ne!(
        normalize_reason_for_fingerprint(r1),
        normalize_reason_for_fingerprint(r2),
    );
}

#[test]
fn normalize_reason_replaces_bare_uuids() {
    let reason = "session 0251b4cc-14f3-48d1-8d81-a11c780c7d7c failed validation";
    let normalized = normalize_reason_for_fingerprint(reason);
    assert!(normalized.contains("UUID"), "got: {normalized}");
    assert!(
        !normalized.contains("0251b4cc"),
        "leftover uuid: {normalized}"
    );
}

#[test]
fn normalize_reason_is_idempotent_for_already_clean_text() {
    // Reasons without any UUID-shaped tokens should pass through
    // unchanged.
    let reason = "failed to reach provider `openai-codex` at https://chatgpt.com/backend-api/codex (request error)";
    assert_eq!(normalize_reason_for_fingerprint(reason), reason);
}

#[test]
fn node_id_from_failure_reason_extracts_node_outcome() {
    assert_eq!(
        node_id_from_failure_reason("automation run failed from node outcomes: research_sources")
            .as_deref(),
        Some("research_sources")
    );
}

#[test]
fn node_incident_matches_aggregate_outcome_only_for_concrete_node_failure() {
    let incident = IncidentMonitorIncidentRecord {
        fingerprint: "node-fingerprint".to_string(),
        repo: "frumu-ai/tandem".to_string(),
        workspace_root: "/workspace".to_string(),
        event_type: "automation_v2.run.failed".to_string(),
        run_id: Some("automation-v2-run-1".to_string()),
        updated_at_ms: 10,
        event_payload: Some(json!({
            "workflow_id": "automation-v2-workflow",
            "run_id": "automation-v2-run-1",
            "node_id": "research_sources",
            "reason": "required_workspace_files_missing",
        })),
        ..Default::default()
    };
    assert!(node_incident_matches_aggregate_outcome(
        &incident,
        "frumu-ai/tandem",
        "/workspace",
        "automation_v2.run.failed",
        "automation-v2-workflow",
        "automation-v2-run-1",
        "research_sources"
    ));

    let aggregate_incident = IncidentMonitorIncidentRecord {
        event_payload: Some(json!({
            "workflow_id": "automation-v2-workflow",
            "run_id": "automation-v2-run-1",
            "node_id": "research_sources",
            "reason": "automation run failed from node outcomes: research_sources",
        })),
        ..incident.clone()
    };
    assert!(!node_incident_matches_aggregate_outcome(
        &aggregate_incident,
        "frumu-ai/tandem",
        "/workspace",
        "automation_v2.run.failed",
        "automation-v2-workflow",
        "automation-v2-run-1",
        "research_sources"
    ));

    let wrong_node = IncidentMonitorIncidentRecord {
        event_payload: Some(json!({
            "workflow_id": "automation-v2-workflow",
            "run_id": "automation-v2-run-1",
            "node_id": "generate_report",
            "reason": "required_workspace_files_missing",
        })),
        ..incident.clone()
    };
    assert!(!node_incident_matches_aggregate_outcome(
        &wrong_node,
        "frumu-ai/tandem",
        "/workspace",
        "automation_v2.run.failed",
        "automation-v2-workflow",
        "automation-v2-run-1",
        "research_sources"
    ));
}

#[tokio::test]
async fn aggregate_node_outcome_lookup_reuses_existing_node_fingerprint() {
    let state = AppState::new_starting("incident-monitor-aggregate-merge-test".to_string(), true);
    let node_incident = IncidentMonitorIncidentRecord {
        incident_id: "incident-node".to_string(),
        fingerprint: "node-fingerprint".to_string(),
        repo: "frumu-ai/tandem".to_string(),
        workspace_root: "/workspace".to_string(),
        event_type: "automation_v2.run.failed".to_string(),
        status: "draft_created".to_string(),
        title: "Node failure".to_string(),
        run_id: Some("automation-v2-run-1".to_string()),
        updated_at_ms: 10,
        event_payload: Some(json!({
            "workflow_id": "automation-v2-workflow",
            "run_id": "automation-v2-run-1",
            "node_id": "research_sources",
            "reason": "required_workspace_files_missing",
        })),
        ..Default::default()
    };
    state
        .put_incident_monitor_incident(node_incident)
        .await
        .expect("store incident");
    let event = event_with(json!({
        "repo": "frumu-ai/tandem",
        "workspace_root": "/workspace",
        "workflow_id": "automation-v2-workflow",
        "run_id": "automation-v2-run-1",
        "reason": "automation run failed from node outcomes: research_sources",
        "component": "automation_v2",
    }));
    let reason = first_string_deep(&event.properties, &["reason"]);
    let node_id = reason.as_deref().and_then(node_id_from_failure_reason);

    let fingerprint = existing_node_incident_fingerprint_for_aggregate_outcome(
        &state,
        "frumu-ai/tandem",
        "/workspace",
        &event.event_type,
        Some("automation-v2-workflow"),
        Some("automation-v2-run-1"),
        node_id.as_deref(),
        reason.as_deref(),
    )
    .await
    .expect("aggregate should reuse concrete node incident fingerprint");

    assert_eq!(fingerprint, "node-fingerprint");
}

#[test]
fn stale_node_id_from_properties_reads_stale_node_arrays_and_aliases() {
    assert_eq!(
        stale_node_id_from_properties(&json!({
            "stale_node_ids": ["assess_reddit_activity"],
        }))
        .as_deref(),
        Some("assess_reddit_activity")
    );
    assert_eq!(
        stale_node_id_from_properties(&json!({
            "staleNodeID": "collect_reddit_signals",
        }))
        .as_deref(),
        Some("collect_reddit_signals")
    );
}

#[test]
fn node_id_from_failure_reason_extracts_timed_out_node() {
    assert_eq!(
        node_id_from_failure_reason(
            "automation node `prepare_search_manifest` timed out after 180000 ms"
        )
        .as_deref(),
        Some("prepare_search_manifest")
    );
}
