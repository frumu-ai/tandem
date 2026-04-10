use super::*;

#[tokio::test]
async fn packs_detect_requires_root_marker() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let root = std::env::temp_dir().join(format!("tandem-pack-detect-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let plain_zip = root.join("plain.zip");
    write_pack_zip(
        &plain_zip,
        "name: detect-test\nversion: 1.0.0\ntype: skill\npack_id: detect-test\n",
    );
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/packs/detect")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "path": plain_zip.to_string_lossy()
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("is_pack").and_then(|v| v.as_bool()), Some(true));
    let event = next_event_of_type(&mut rx, "pack.detected").await;
    assert_eq!(
        event.properties.get("marker").and_then(|v| v.as_str()),
        Some("tandempack.yaml")
    );
    assert_eq!(
        event.properties.get("path").and_then(|v| v.as_str()),
        Some(plain_zip.to_string_lossy().as_ref())
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn packs_detect_returns_false_without_marker() {
    let state = test_state().await;
    let root = std::env::temp_dir().join(format!("tandem-pack-detect-none-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let plain_zip = root.join("plain.zip");
    write_plain_zip_without_marker(&plain_zip);
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/packs/detect")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "path": plain_zip.to_string_lossy()
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("is_pack").and_then(|v| v.as_bool()),
        Some(false)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn packs_install_list_and_uninstall_roundtrip() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let root = std::env::temp_dir().join(format!("tandem-pack-install-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("pack.zip");
    write_pack_zip(
        &pack_zip,
        "name: roundtrip-pack\nversion: 1.2.3\ntype: workflow\npack_id: roundtrip-pack\n",
    );
    let app = app_router(state.clone());
    let install_req = Request::builder()
        .method("POST")
        .uri("/packs/install")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "path": pack_zip.to_string_lossy(),
                "source": {"kind":"test"}
            })
            .to_string(),
        ))
        .expect("request");
    let install_resp = app.clone().oneshot(install_req).await.expect("response");
    assert_eq!(install_resp.status(), StatusCode::OK);
    let install_body = to_bytes(install_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let install_payload: Value = serde_json::from_slice(&install_body).expect("json");
    let install_path = install_payload
        .get("installed")
        .and_then(|v| v.get("install_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(install_path.ends_with("/roundtrip-pack/1.2.3"));
    let current_pointer = std::path::PathBuf::from(install_path)
        .parent()
        .expect("parent")
        .join("current");
    let current_version = std::fs::read_to_string(&current_pointer).expect("read current");
    assert_eq!(current_version.trim(), "1.2.3");
    let started = next_event_of_type(&mut rx, "pack.install.started").await;
    assert_eq!(
        started.properties.get("path").and_then(|v| v.as_str()),
        Some(pack_zip.to_string_lossy().as_ref())
    );
    let succeeded = next_event_of_type(&mut rx, "pack.install.succeeded").await;
    assert_eq!(
        succeeded.properties.get("pack_id").and_then(|v| v.as_str()),
        Some("roundtrip-pack")
    );
    let registry = next_event_of_type(&mut rx, "registry.updated").await;
    assert_eq!(
        registry.properties.get("entity").and_then(|v| v.as_str()),
        Some("packs")
    );

    let list_req = Request::builder()
        .method("GET")
        .uri("/packs")
        .body(Body::empty())
        .expect("request");
    let list_resp = app.clone().oneshot(list_req).await.expect("response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    let packs = list_payload
        .get("packs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(packs.iter().any(|p| {
        p.get("pack_id").and_then(|v| v.as_str()) == Some("roundtrip-pack")
            && p.get("version").and_then(|v| v.as_str()) == Some("1.2.3")
    }));

    let uninstall_req = Request::builder()
        .method("POST")
        .uri("/packs/uninstall")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "pack_id": "roundtrip-pack",
                "version": "1.2.3"
            })
            .to_string(),
        ))
        .expect("request");
    let uninstall_resp = app.clone().oneshot(uninstall_req).await.expect("response");
    assert_eq!(uninstall_resp.status(), StatusCode::OK);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn packs_updates_endpoints_return_stub_payload() {
    let state = test_state().await;
    let root = std::env::temp_dir().join(format!("tandem-pack-updates-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("pack.zip");
    write_pack_zip(
        &pack_zip,
        "name: update-pack\nversion: 1.0.0\ntype: workflow\npack_id: update-pack\n",
    );
    let app = app_router(state.clone());
    let install_req = Request::builder()
        .method("POST")
        .uri("/packs/install")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "path": pack_zip.to_string_lossy(),
                "source": {"kind":"test"}
            })
            .to_string(),
        ))
        .expect("request");
    let install_resp = app.clone().oneshot(install_req).await.expect("response");
    assert_eq!(install_resp.status(), StatusCode::OK);

    let updates_req = Request::builder()
        .method("GET")
        .uri("/packs/update-pack/updates")
        .body(Body::empty())
        .expect("request");
    let updates_resp = app.clone().oneshot(updates_req).await.expect("response");
    assert_eq!(updates_resp.status(), StatusCode::OK);
    let updates_body = to_bytes(updates_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let updates_payload: Value = serde_json::from_slice(&updates_body).expect("json");
    assert_eq!(
        updates_payload
            .get("current_version")
            .and_then(|v| v.as_str()),
        Some("1.0.0")
    );
    assert_eq!(
        updates_payload
            .get("reapproval_required")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(updates_payload
        .get("permissions_diff")
        .and_then(|v| v.as_object())
        .is_some());

    let apply_req = Request::builder()
        .method("POST")
        .uri("/packs/update-pack/update")
        .header("content-type", "application/json")
        .body(Body::from(json!({"target_version":"1.1.0"}).to_string()))
        .expect("request");
    let apply_resp = app.clone().oneshot(apply_req).await.expect("response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("json");
    assert_eq!(
        apply_payload.get("reason").and_then(|v| v.as_str()),
        Some("updates_not_implemented")
    );
    assert_eq!(
        apply_payload
            .get("reapproval_required")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn packs_get_reports_workflow_extensions() {
    let state = test_state().await;
    let root = std::env::temp_dir().join(format!("tandem-pack-inspect-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("inspect-pack.zip");
    let file = std::fs::File::create(&pack_zip).expect("create zip");
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("tandempack.yaml", opts).expect("manifest");
    std::io::Write::write_all(
        &mut zip,
        br#"name: workflow-inspect-pack
version: 1.0.0
type: workflow
pack_id: workflow-inspect-pack
entrypoints:
  workflows:
    - build_feature
contents:
  workflows:
    - id: build_feature
      path: workflows/build_feature.yaml
  workflow_hooks:
    - id: build_feature.task_completed.notify
      path: hooks/notify.yaml
"#,
    )
    .expect("write manifest");
    zip.start_file("workflows/build_feature.yaml", opts)
        .expect("workflow file");
    std::io::Write::write_all(
        &mut zip,
        b"workflow:\n  id: build_feature\n  name: Build Feature\n  steps:\n    - planner\n",
    )
    .expect("write workflow file");
    zip.start_file("hooks/notify.yaml", opts)
        .expect("hook file");
    std::io::Write::write_all(
        &mut zip,
        b"hooks:\n  - id: build_feature.task_completed.notify\n    workflow_id: build_feature\n    event: task_completed\n    actions:\n      - slack.notify\n",
    )
    .expect("write hook file");
    zip.finish().expect("finish zip");

    let app = app_router(state.clone());
    let install_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/install")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "path": pack_zip.to_string_lossy(),
                        "source": {"kind":"test"}
                    })
                    .to_string(),
                ))
                .expect("install request"),
        )
        .await
        .expect("install response");
    assert_eq!(install_resp.status(), StatusCode::OK);

    let inspect_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/packs/workflow-inspect-pack")
                .body(Body::empty())
                .expect("inspect request"),
        )
        .await
        .expect("inspect response");
    assert_eq!(inspect_resp.status(), StatusCode::OK);
    let body = to_bytes(inspect_resp.into_body(), usize::MAX)
        .await
        .expect("inspect body");
    let payload: Value = serde_json::from_slice(&body).expect("inspect json");
    assert_eq!(
        payload["pack"]["workflow_extensions"]["workflow_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        payload["pack"]["workflow_extensions"]["workflow_hook_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        payload["pack"]["workflow_extensions"]["workflow_entrypoints"]
            .as_array()
            .map(|rows| rows.len()),
        Some(1)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn packs_install_with_marketplace_assets_exposes_files() {
    let state = test_state().await;
    let root = std::env::temp_dir().join(format!("tandem-pack-marketplace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("marketplace-pack.zip");
    write_pack_zip_with_entries(
        &pack_zip,
        r#"
manifest_schema_version: 1
pack_id: marketplace-pack
name: marketplace-pack
version: 1.0.0
type: workflow
engine:
  requires: ">=0.9.0 <2.0.0"
marketplace:
  publisher:
    publisher_id: pub_tandem_official
    display_name: Tandem
    verification_tier: official
  listing:
    display_name: Marketplace Pack
    description: Marketplace pack used for regression coverage.
    categories: ["testing"]
    tags: ["marketplace", "pack"]
    license_spdx: Apache-2.0
    icon: resources/marketplace/icon.svg
    screenshots: ["resources/marketplace/screenshot-1.svg"]
    changelog: CHANGELOG.md
entrypoints:
  workflows:
    - marketplace_workflow
capabilities:
  required: []
  optional: []
  non_portable: []
contents:
  workflows:
    - id: marketplace_workflow
      path: workflows/marketplace-workflow.yaml
"#,
        &[
            ("README.md", "# Marketplace Pack\n\nThis pack ships assets."),
            ("CHANGELOG.md", "## 1.0.0\n- Initial marketplace package"),
            (
                "resources/marketplace/icon.svg",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"6\" fill=\"#1d4ed8\"/></svg>",
            ),
            (
                "resources/marketplace/screenshot-1.svg",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"6\" fill=\"#0f172a\"/></svg>",
            ),
            (
                "workflows/marketplace-workflow.yaml",
                "workflow:\n  id: marketplace_workflow\n  name: Marketplace Workflow\n  steps:\n    - action: agent:project-manager\n      with:\n        prompt: Hello from marketplace\n",
            ),
        ],
    );
    let app = app_router(state.clone());
    let install_req = Request::builder()
        .method("POST")
        .uri("/packs/install")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "path": pack_zip.to_string_lossy(),
                "source": {"kind":"test"}
            })
            .to_string(),
        ))
        .expect("request");
    let install_resp = app.clone().oneshot(install_req).await.expect("response");
    assert_eq!(install_resp.status(), StatusCode::OK);

    let file_req = Request::builder()
        .method("GET")
        .uri("/packs/marketplace-pack/files/README.md")
        .body(Body::empty())
        .expect("request");
    let file_resp = app.clone().oneshot(file_req).await.expect("response");
    assert_eq!(file_resp.status(), StatusCode::OK);
    assert_eq!(
        file_resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/markdown; charset=utf-8")
    );
    let file_body = to_bytes(file_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let body_text = String::from_utf8(file_body.to_vec()).expect("utf8");
    assert!(body_text.contains("Marketplace Pack"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn packs_install_rejects_missing_marketplace_assets() {
    let state = test_state().await;
    let root = std::env::temp_dir().join(format!(
        "tandem-pack-marketplace-missing-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("marketplace-pack-missing.zip");
    write_pack_zip_with_entries(
        &pack_zip,
        r#"
manifest_schema_version: 1
pack_id: marketplace-pack-missing
name: marketplace-pack-missing
version: 1.0.0
type: workflow
engine:
  requires: ">=0.9.0 <2.0.0"
marketplace:
  publisher:
    publisher_id: pub_tandem_official
    display_name: Tandem
    verification_tier: official
  listing:
    display_name: Marketplace Pack Missing
    description: Missing asset coverage test.
    categories: ["testing"]
    tags: ["marketplace", "pack"]
    license_spdx: Apache-2.0
    icon: resources/marketplace/icon.svg
    screenshots: ["resources/marketplace/screenshot-1.svg"]
    changelog: CHANGELOG.md
entrypoints:
  workflows:
    - marketplace_workflow
capabilities:
  required: []
  optional: []
  non_portable: []
contents:
  workflows:
    - id: marketplace_workflow
      path: workflows/marketplace-workflow.yaml
"#,
        &[
            ("README.md", "# Marketplace Pack"),
            ("CHANGELOG.md", "## 1.0.0\n- Initial marketplace package"),
            (
                "workflows/marketplace-workflow.yaml",
                "workflow:\n  id: marketplace_workflow\n  name: Marketplace Workflow\n  steps:\n    - action: agent:project-manager\n      with:\n        prompt: Hello from marketplace\n",
            ),
        ],
    );
    let app = app_router(state);
    let install_req = Request::builder()
        .method("POST")
        .uri("/packs/install")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "path": pack_zip.to_string_lossy(),
                "source": {"kind":"test"}
            })
            .to_string(),
        ))
        .expect("request");
    let install_resp = app.oneshot(install_req).await.expect("response");
    assert_eq!(install_resp.status(), StatusCode::BAD_REQUEST);
    let _ = std::fs::remove_dir_all(root);
}
