// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn hosted_review_storage_failure_is_500_without_publishing_candidate() {
    let (mut state, _policy) = hosted_learning_state().await;
    let root = state.workspace_index.snapshot().await.root;
    let source = state
        .put_automation_v2(hosted_learning_automation(
            &root,
            "alice-storage-failure",
            "alice",
        ))
        .await
        .expect("Alice's hosted source workflow");
    let candidate = candidate_for_workflow(
        sample_candidate(
            "alice-review-storage-failure",
            &source.automation_id,
            crate::WorkflowLearningCandidateKind::PromptPatch,
            crate::WorkflowLearningCandidateStatus::Proposed,
        ),
        &source,
    );
    put_hosted_learning_candidate(&state, candidate)
        .await
        .expect("persist original hosted candidate");
    let original_path = state.workflow_learning_candidates_path.clone();
    let original_bytes = std::fs::read(&original_path).expect("sealed original candidate store");
    let original_candidate = serde_json::to_value(
        state
            .get_workflow_learning_candidate("alice-review-storage-failure")
            .await
            .unwrap(),
    )
    .unwrap();

    // A directory at the configured file path causes a real durable read
    // failure before any candidate mutation; the handler must preserve the
    // authorized candidate and distinguish storage failure from a missing ID.
    let blocked = tempfile::tempdir().unwrap();
    let blocked_path = blocked.path().join("blocked-candidate-file");
    std::fs::create_dir(&blocked_path).unwrap();
    std::fs::write(blocked_path.join("marker"), b"unchanged synthetic target").unwrap();
    state.workflow_learning_candidates_path = blocked_path.clone();
    let alice = hosted_learning_router(state.clone(), "alice");
    let (status, _) = hosted_learning_request(
        alice.clone(),
        "POST",
        "/workflow-learning/candidates/alice-review-storage-failure/review",
        Some(json!({"action":"approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(std::fs::read(&original_path).unwrap(), original_bytes);
    assert_eq!(
        std::fs::read(blocked_path.join("marker")).unwrap(),
        b"unchanged synthetic target"
    );
    assert_eq!(
        serde_json::to_value(
            state
                .get_workflow_learning_candidate("alice-review-storage-failure")
                .await
                .unwrap(),
        )
        .unwrap(),
        original_candidate
    );

    let (missing, _) = hosted_learning_request(
        alice,
        "POST",
        "/workflow-learning/candidates/absent-candidate/review",
        Some(json!({"action":"approve"})),
    )
    .await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}
