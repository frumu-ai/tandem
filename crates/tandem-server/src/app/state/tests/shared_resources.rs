// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
#[tokio::test]
async fn shared_resource_put_increments_revision() {
    let path = tmp_resource_file("shared-resource-put");
    let state = test_state_with_path(path.clone());

    let first = state
        .put_shared_resource(
            "project/demo/board".to_string(),
            serde_json::json!({"status":"todo"}),
            None,
            "agent-1".to_string(),
            None,
        )
        .await
        .expect("first put");
    assert_eq!(first.rev, 1);

    let second = state
        .put_shared_resource(
            "project/demo/board".to_string(),
            serde_json::json!({"status":"doing"}),
            Some(1),
            "agent-2".to_string(),
            Some(60_000),
        )
        .await
        .expect("second put");
    assert_eq!(second.rev, 2);
    assert_eq!(second.updated_by, "agent-2");
    assert_eq!(second.ttl_ms, Some(60_000));

    let raw = tokio::fs::read_to_string(path.clone())
        .await
        .expect("persisted");
    assert!(raw.contains("\"rev\": 2"));
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn shared_resource_put_detects_revision_conflict() {
    let path = tmp_resource_file("shared-resource-conflict");
    let state = test_state_with_path(path.clone());

    let _ = state
        .put_shared_resource(
            "mission/demo/card-1".to_string(),
            serde_json::json!({"title":"Card 1"}),
            None,
            "agent-1".to_string(),
            None,
        )
        .await
        .expect("seed put");

    let conflict = state
        .put_shared_resource(
            "mission/demo/card-1".to_string(),
            serde_json::json!({"title":"Card 1 edited"}),
            Some(99),
            "agent-2".to_string(),
            None,
        )
        .await
        .expect_err("expected conflict");

    match conflict {
        ResourceStoreError::RevisionConflict(conflict) => {
            assert_eq!(conflict.expected_rev, Some(99));
            assert_eq!(conflict.current_rev, Some(1));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn shared_resource_rejects_invalid_namespace_key() {
    let path = tmp_resource_file("shared-resource-invalid-key");
    let state = test_state_with_path(path.clone());

    let error = state
        .put_shared_resource(
            "global/demo/key".to_string(),
            serde_json::json!({"x":1}),
            None,
            "agent-1".to_string(),
            None,
        )
        .await
        .expect_err("invalid key should fail");

    match error {
        ResourceStoreError::InvalidKey { key } => assert_eq!(key, "global/demo/key"),
        other => panic!("unexpected error: {other:?}"),
    }

    assert!(!path.exists());
}

#[test]
fn shared_resource_key_validator_accepts_swarm_active_tasks() {
    assert!(is_valid_resource_key("swarm.active_tasks"));
    assert!(is_valid_resource_key("project/demo"));
    assert!(!is_valid_resource_key("swarm//active_tasks"));
    assert!(!is_valid_resource_key("misc/demo"));
}
