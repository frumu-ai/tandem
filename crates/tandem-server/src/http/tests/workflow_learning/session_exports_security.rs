// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn hosted_pack_router(state: AppState, actor: &str) -> axum::Router {
    let tenant = hosted_learning_tenant(actor);
    let verified = hosted_learning_verified(&state, actor);
    axum::Router::<AppState>::new()
        .route(
            "/workflow-learning/candidates/{candidate_id}/spawn-revision",
            axum::routing::post(skills_memory::workflow_learning_candidate_spawn_revision),
        )
        .route(
            "/workflow-plans/export/pack",
            axum::routing::post(crate::http::workflow_planner::workflow_plan_export_pack),
        )
        .route(
            "/workflow-plans/export/pack/download",
            axum::routing::get(crate::http::workflow_planner::workflow_plan_export_pack_download),
        )
        .route(
            "/workflow-plans/import/pack/preview",
            axum::routing::post(crate::http::workflow_planner::workflow_plan_import_pack_preview),
        )
        .route(
            "/workflow-plans/import/pack",
            axum::routing::post(crate::http::workflow_planner::workflow_plan_import_pack),
        )
        .route(
            "/workflow-plans/apply",
            axum::routing::post(crate::http::workflow_planner::workflow_plan_apply),
        )
        .layer(axum::Extension(tenant))
        .layer(axum::Extension(verified))
        .with_state(state)
}

async fn hosted_revision_plan(state: &AppState, actor: &str) -> (String, String) {
    let root = state.workspace_index.snapshot().await.root;
    let source_id = format!("{actor}-pack-source");
    let candidate_id = format!("{actor}-pack-candidate");
    let mut source = hosted_learning_automation(&root, &source_id, actor);
    let mut plan = sample_plan_package_bundle().plan;
    plan.plan_id = format!("plan_{actor}_pack_security");
    let exported = tandem_plan_compiler::api::export_plan_package_bundle(&plan);
    source.metadata.as_mut().unwrap()["plan_package_bundle"] =
        json!(tandem_plan_compiler::api::PlanPackageImportBundle {
            bundle_version: exported.bundle_version,
            plan: exported.plan,
            scope_snapshot: Some(exported.scope_snapshot),
        });
    let source = state
        .put_automation_v2(source)
        .await
        .expect("private source");
    state
        .put_workflow_learning_candidate(candidate_for_workflow(
            sample_candidate(
                &candidate_id,
                &source.automation_id,
                crate::WorkflowLearningCandidateKind::PromptPatch,
                crate::WorkflowLearningCandidateStatus::Approved,
            ),
            &source,
        ))
        .await
        .expect("private candidate");
    let app = hosted_pack_router(state.clone(), actor);
    let (status, payload) = hosted_learning_request(
        app,
        "POST",
        &format!("/workflow-learning/candidates/{candidate_id}/spawn-revision"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    (
        payload["session"]["session_id"]
            .as_str()
            .expect("revision session ID")
            .to_string(),
        payload["session"]["current_plan_id"]
            .as_str()
            .expect("revision plan ID")
            .to_string(),
    )
}

#[tokio::test]
async fn hosted_pack_artifacts_and_plan_materialization_follow_current_owner() {
    let (state, _policy) = hosted_learning_state().await;
    let (alice_session_id, alice_plan_id) = hosted_revision_plan(&state, "alice").await;
    let alice = hosted_pack_router(state.clone(), "alice");
    let bob = hosted_pack_router(state.clone(), "bob");

    for body in [
        json!({"session_id": alice_session_id}),
        json!({"plan_id": alice_plan_id}),
    ] {
        let (status, payload) = hosted_learning_request(
            bob.clone(),
            "POST",
            "/workflow-plans/export/pack",
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "foreign export: {payload}");
    }
    let (status, payload) = hosted_learning_request(
        bob.clone(),
        "POST",
        "/workflow-plans/apply",
        Some(json!({"plan_id": alice_plan_id})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "foreign apply: {payload}");

    let cover_dir = tempfile::tempdir().expect("cover directory");
    let private_cover = cover_dir.path().join("unmanaged.png");
    std::fs::write(&private_cover, b"private image bytes").expect("private cover fixture");
    let (status, payload) = hosted_learning_request(
        alice.clone(),
        "POST",
        "/workflow-plans/export/pack",
        Some(json!({
            "plan_id": alice_plan_id,
            "cover_image_path": private_cover.to_string_lossy(),
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unmanaged cover path: {payload}"
    );

    let (status, alice_export) = hosted_learning_request(
        alice.clone(),
        "POST",
        "/workflow-plans/export/pack",
        Some(json!({"plan_id": alice_plan_id, "name": "shared-pack", "version": "1.0.0"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Alice export: {alice_export}");
    let alice_path = alice_export["exported"]["path"]
        .as_str()
        .expect("Alice export path");
    assert!(std::path::Path::new(alice_path)
        .starts_with(state.pack_manager.workflow_pack_exports_root()));
    let alice_zip = std::fs::read(alice_path).expect("Alice ZIP");
    let alice_download = alice_export["exported"]["download_url"]
        .as_str()
        .expect("Alice download URL");
    let (status, _) = hosted_learning_request(alice.clone(), "GET", alice_download, None).await;
    assert_eq!(status, StatusCode::OK, "Alice can download her export");
    let (status, preview) = hosted_learning_request(
        alice.clone(),
        "POST",
        "/workflow-plans/import/pack/preview",
        Some(json!({"path": alice_path, "plan_id": alice_plan_id})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Alice can preview her export: {preview}"
    );
    let (status, payload) = hosted_learning_request(
        alice,
        "POST",
        "/workflow-plans/import/pack",
        Some(json!({"path": alice_path, "plan_id": alice_plan_id})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "hosted install must stay scoped: {payload}"
    );

    let (_, bob_plan_id) = hosted_revision_plan(&state, "bob").await;
    let (status, bob_export) = hosted_learning_request(
        bob.clone(),
        "POST",
        "/workflow-plans/export/pack",
        Some(json!({"plan_id": bob_plan_id, "name": "shared-pack", "version": "1.0.0"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Bob export: {bob_export}");
    let bob_path = bob_export["exported"]["path"]
        .as_str()
        .expect("Bob export path");
    assert_ne!(
        alice_path, bob_path,
        "exports must not overwrite across actors"
    );
    assert_eq!(std::fs::read(alice_path).unwrap(), alice_zip);

    let foreign_download = format!(
        "/workflow-plans/export/pack/download?path={}&plan_id={}",
        urlencoding::encode(alice_path),
        urlencoding::encode(&bob_plan_id)
    );
    let (status, _) = hosted_learning_request(bob.clone(), "GET", &foreign_download, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "foreign artifact path");
    let (status, payload) = hosted_learning_request(
        bob.clone(),
        "POST",
        "/workflow-plans/import/pack/preview",
        Some(json!({"path": alice_path, "plan_id": bob_plan_id})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "foreign preview: {payload}");
    let (status, payload) = hosted_learning_request(
        bob,
        "POST",
        "/workflow-plans/import/pack",
        Some(json!({"path": alice_path, "plan_id": alice_plan_id})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "foreign import: {payload}");
}
