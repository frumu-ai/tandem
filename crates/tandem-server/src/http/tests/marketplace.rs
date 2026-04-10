use super::*;

#[tokio::test]
async fn marketplace_catalog_and_files_roundtrip() {
    let _guard = marketplace_test_lock();
    let state = test_state().await;
    let root = std::env::temp_dir().join(format!("tandem-marketplace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let zip_path = root.join("seed-pack-1.0.0.zip");
    write_pack_zip_with_entries(
        &zip_path,
        r#"
manifest_schema_version: 1
pack_id: seed-pack
name: seed-pack
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
    display_name: Seed Pack
    description: Starter marketplace pack for testing.
    categories: ["planning"]
    tags: ["seed", "planning"]
    license_spdx: Apache-2.0
    icon: resources/marketplace/icon.svg
    screenshots: ["resources/marketplace/screenshot-1.svg"]
    changelog: CHANGELOG.md
entrypoints:
  workflows:
    - seed_workflow
contents:
  workflows:
    - id: seed_workflow
      path: workflows/seed_workflow.yaml
"#,
        &[
            ("README.md", "# Seed Pack\n\nA compact marketplace seed."),
            ("CHANGELOG.md", "## 1.0.0\n- Initial seed pack"),
            (
                "resources/marketplace/icon.svg",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"6\" fill=\"#1d4ed8\"/></svg>",
            ),
            (
                "resources/marketplace/screenshot-1.svg",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"6\" fill=\"#0f172a\"/></svg>",
            ),
            (
                "workflows/seed_workflow.yaml",
                "workflow:\n  id: seed_workflow\n  name: Seed Workflow\n  steps:\n    - action: agent:project-manager\n      with:\n        prompt: Hello from the marketplace seed pack\n",
            ),
        ],
    );
    let catalog_path = root.join("catalog.json");
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "1",
            "generated_at": "2026-01-01T00:00:00Z",
            "packs": [
                {
                    "schema_version": "1",
                    "pack_id": "seed-pack",
                    "name": "seed-pack",
                    "version": "1.0.0",
                    "publisher": {
                        "publisher_id": "pub_tandem_official",
                        "display_name": "Tandem",
                        "verification_tier": "official",
                        "website": "https://tandem.ac/",
                        "support": "support@tandem.ac"
                    },
                    "listing": {
                        "display_name": "Seed Pack",
                        "description": "Starter marketplace pack for testing.",
                        "categories": ["planning"],
                        "tags": ["seed", "planning"],
                        "license_spdx": "Apache-2.0",
                        "icon_url": "resources/marketplace/icon.svg",
                        "screenshot_urls": ["resources/marketplace/screenshot-1.svg"],
                        "changelog_url": "CHANGELOG.md"
                    },
                    "distribution": {
                        "download_url": "seed-pack-1.0.0.zip",
                        "sha256": "abc",
                        "size_bytes": 123,
                        "signature_status": "missing"
                    },
                    "pack_source_dir": "private/seed-pack",
                    "workflow_ids": ["seed_workflow"],
                    "capabilities": {}
                }
            ]
        }))
        .expect("catalog json"),
    )
    .expect("write catalog");
    let previous = std::env::var("TANDEM_MARKETPLACE_CATALOG_PATH").ok();
    std::env::set_var("TANDEM_MARKETPLACE_CATALOG_PATH", &catalog_path);

    let app = app_router(state);
    let catalog_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/marketplace/catalog")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(catalog_resp.status(), StatusCode::OK);
    let catalog_body = to_bytes(catalog_resp.into_body(), usize::MAX)
        .await
        .expect("catalog body");
    let catalog_json: Value = serde_json::from_slice(&catalog_body).expect("catalog json");
    assert_eq!(
        catalog_json
            .get("packs")
            .and_then(|v| v.as_array())
            .map(|rows| rows.len()),
        Some(1)
    );
    let zip_path_value = catalog_json["packs"][0]["distribution"]["zip_path"]
        .as_str()
        .unwrap_or("");
    assert!(zip_path_value.ends_with("seed-pack-1.0.0.zip"));

    let file_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/marketplace/packs/seed-pack/files/README.md")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
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
        .expect("file body");
    let body_text = String::from_utf8(file_body.to_vec()).expect("utf8");
    assert!(body_text.contains("Seed Pack"));

    match previous {
        Some(value) => std::env::set_var("TANDEM_MARKETPLACE_CATALOG_PATH", value),
        None => std::env::remove_var("TANDEM_MARKETPLACE_CATALOG_PATH"),
    }
    let _ = std::fs::remove_dir_all(root);
}

fn marketplace_test_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("lock marketplace test")
}
