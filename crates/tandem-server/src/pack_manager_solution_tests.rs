// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::tests::{write_signed_zip, EnvGuard};
use super::*;

pub(super) fn fixture() -> Vec<(String, String)> {
    [
        (
            MARKER_FILE,
            include_str!("../../tandem-solutions/fixtures/company-brain-text/tandempack.yaml"),
        ),
        (
            "solution.json",
            include_str!("../../tandem-solutions/fixtures/company-brain-text/solution.json"),
        ),
        (
            "agents/central-brain.json",
            include_str!(
                "../../tandem-solutions/fixtures/company-brain-text/agents/central-brain.json"
            ),
        ),
        (
            "routines/review-notes.json",
            include_str!(
                "../../tandem-solutions/fixtures/company-brain-text/routines/review-notes.json"
            ),
        ),
    ]
    .into_iter()
    .map(|(path, bytes)| (path.into(), bytes.into()))
    .collect()
}

pub(super) fn signed(path: &Path, entries: &[(String, String)]) -> String {
    write_signed_zip(
        path,
        &entries
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_str()))
            .collect::<Vec<_>>(),
    )
}

pub(super) fn request(path: &Path) -> PackInstallRequest {
    PackInstallRequest {
        path: Some(path.to_string_lossy().into_owned()),
        url: None,
        expected_sha256: None,
        source: Value::Null,
    }
}

fn export_request(name: &str) -> PackExportRequest {
    PackExportRequest {
        pack_id: Some("tandem.company-brain".into()),
        name: None,
        version: Some("0.1.0".into()),
        output_path: Some(name.into()),
    }
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_pack_snapshot_preserves_existing_nested_path_signature_order() {
    let root = tempfile::tempdir().unwrap();
    let mut entries = fixture();
    let mut blueprint: Value = serde_json::from_str(&entries[1].1).unwrap();
    entries[2].0 = "artifacts.json".into();
    entries[3].0 = "artifacts/review.json".into();
    blueprint["components"]["central-brain"]["artifact"]["path"] = entries[2].0.clone().into();
    blueprint["components"]["review-notes"]["artifact"]["path"] = entries[3].0.clone().into();
    entries[1].1 = serde_json::to_string(&blueprint).unwrap();
    let archive = root.path().join("ordered.zip");
    let key = signed(&archive, &entries);
    let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
    let manager = PackManager::new(root.path().join("packs"));
    manager.install(request(&archive)).await.unwrap();
    assert_eq!(
        manager
            .solution_artifacts("tandem.company-brain")
            .await
            .unwrap()
            .artifacts
            .len(),
        2
    );
    manager.export(export_request("ordered.zip")).await.unwrap();
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_pack_signed_artifacts_feed_existing_resolver_and_round_trip() {
    use std::collections::{BTreeMap, BTreeSet};
    use tandem_enterprise_contract::{
        AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
        VerifiedTenantContext,
    };
    use tandem_solutions::{InstallRequest, ModelBinding, ResolutionInput};
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("solution.zip");
    let key = signed(&archive, &fixture());
    let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
    let manager = PackManager::new(root.path().join("packs"));
    let installed = manager.install(request(&archive)).await.unwrap();
    assert!(!installed.routines_enabled);
    assert!(installed.solution_content_sha256.is_some());
    let inspection = manager.inspect("tandem.company-brain").await.unwrap();
    assert_eq!(
        inspection.solution.as_ref().unwrap()["activation_required"],
        true
    );
    let source = manager
        .solution_artifacts("tandem.company-brain")
        .await
        .unwrap();
    let context: VerifiedTenantContext = TenantContextAssertionClaims::new_v1(
        "issuer",
        "runtime",
        1000,
        2000,
        "synthetic",
        TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".into()),
            "owner-a",
        ),
        HumanActor::tandem_user("owner-a"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("owner-a", "fixture")),
        vec!["workspace:user".into()],
    )
    .into();
    let install = InstallRequest {
        instance_id: "company-brain".into(),
        customer_config_revision: "a".repeat(64),
        optional_components: ["review-notes".into()].into(),
        preferences: BTreeMap::new(),
        connectors: BTreeMap::new(),
        models: [("economy".into(), "fixture".into())].into(),
    };
    let models = [(
        "fixture".into(),
        ModelBinding {
            provider: "local".into(),
            model: "fixture".into(),
            credential_ref: "secret-ref:local-fixture".into(),
            uses_network: false,
        },
    )]
    .into();
    let readiness: BTreeSet<String> =
        ["governed-memory".into(), "verified-user-context".into()].into();
    let plan = tandem_solutions::resolve(
        &source.blueprint,
        ResolutionInput {
            host_facts_sha256: None,
            request: &install,
            verified_context: &context,
            now_ms: 1500,
            engine_version: "0.7.2",
            deployment_policy: &source.blueprint.constraints,
            available_deployment_requirements: &readiness,
            approved_models: &models,
            artifacts: &source.artifacts,
        },
    )
    .unwrap();
    assert_eq!(plan.install_order, ["central-brain", "review-notes"]);
    let export = manager
        .export(export_request("reusable.zip"))
        .await
        .unwrap();
    let other = PackManager::new(root.path().join("other"));
    other
        .install(request(Path::new(&export.path)))
        .await
        .unwrap();
    let imported = other
        .solution_artifacts("tandem.company-brain")
        .await
        .unwrap();
    assert_eq!(source.blueprint, imported.blueprint);
    assert_eq!(source.artifacts, imported.artifacts);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_pack_invalid_closure_or_customer_files_leave_no_install() {
    for failure in [
        "missing",
        "digest",
        "external",
        "customer",
        "path",
        "entrypoint",
        "secret",
        "large-secret",
        "example-secret",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut entries = fixture();
        let mut blueprint: Value = serde_json::from_str(&entries[1].1).unwrap();
        match failure {
            "missing" => {
                entries.remove(2);
            }
            "digest" => {
                blueprint["components"]["central-brain"]["artifact"]["sha256"] =
                    Value::String("b".repeat(64))
            }
            "external" => {
                blueprint["components"]["central-brain"]["artifact"]["pack_id"] =
                    "other.pack".into()
            }
            "customer" => entries.push((
                "customer.yaml".into(),
                "private: synthetic-customer-value".into(),
            )),
            "path" => {
                blueprint["components"]["central-brain"]["artifact"]["path"] =
                    "../outside.json".into()
            }
            "entrypoint" => {
                entries[0].1 = entries[0]
                    .1
                    .replace("solution: solution.json", "solution: ../outside.json")
            }
            "secret" | "large-secret" | "example-secret" => {
                entries[2].1 = concat!("gh", "p_", "synthetic-fixture").into();
                if failure == "large-secret" {
                    entries[2].1 =
                        " ".repeat(SECRET_SCAN_MAX_FILE_BYTES as usize + 1) + &entries[2].1;
                }
                if failure == "example-secret" {
                    entries[2].0 = "agents/central-brain.example.json".into();
                    blueprint["components"]["central-brain"]["artifact"]["path"] =
                        entries[2].0.clone().into();
                }
                blueprint["components"]["central-brain"]["artifact"]["sha256"] =
                    format!("{:x}", Sha256::digest(entries[2].1.as_bytes())).into();
            }
            _ => unreachable!(),
        }
        entries[1].1 = serde_json::to_string(&blueprint).unwrap();
        let archive = root.path().join("invalid.zip");
        let key = signed(&archive, &entries);
        let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
        let _scan = EnvGuard::set("TANDEM_PACK_SECRET_SCAN_STRICT", "false");
        let manager = PackManager::new(root.path().join("packs"));
        assert!(
            manager.install(request(&archive)).await.is_err(),
            "{failure}"
        );
        assert!(manager.list().await.unwrap().is_empty(), "{failure}");
    }
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_pack_export_rejects_nearby_customer_state_and_revoked_trust() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("solution.zip");
    let key = signed(&archive, &fixture());
    let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
    let manager = PackManager::new(root.path().join("packs"));
    let installed = manager.install(request(&archive)).await.unwrap();
    let customer = Path::new(&installed.install_path).join("customer-private.json");
    fs::write(&customer, "synthetic private state").unwrap();
    assert!(manager
        .solution_artifacts("tandem.company-brain")
        .await
        .is_err());
    assert!(manager.export(export_request("private.zip")).await.is_err());
    assert!(!root.path().join("packs/exports/private.zip").exists());
    fs::remove_file(customer).unwrap();
    let _revoked = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", "");
    assert!(manager
        .solution_artifacts("tandem.company-brain")
        .await
        .is_err());
    assert!(manager.export(export_request("revoked.zip")).await.is_err());
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_pack_resigned_same_version_cannot_replace_installed_content() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("solution.zip");
    let mut entries = fixture();
    let key = signed(&archive, &entries);
    let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
    let manager = PackManager::new(root.path().join("packs"));
    let installed = manager.install(request(&archive)).await.unwrap();
    let mut blueprint: Value = serde_json::from_str(&entries[1].1).unwrap();
    blueprint["ui_features"] = serde_json::json!(["chat", "memory", "changed"]);
    entries[1].1 = serde_json::to_string(&blueprint).unwrap();
    let replacement = root.path().join("replacement.zip");
    signed(&replacement, &entries);
    let unpacked = root.path().join("replacement");
    fs::create_dir(&unpacked).unwrap();
    safe_extract_zip(&replacement, &unpacked).unwrap();
    for relative in ["solution.json", PACK_SIGNATURE_FILE] {
        fs::copy(
            unpacked.join(relative),
            Path::new(&installed.install_path).join(relative),
        )
        .unwrap();
    }
    assert!(matches!(
        verify_pack_signature(Path::new(&installed.install_path)).unwrap(),
        PackSignatureStatus::Verified { .. }
    ));
    assert!(manager
        .solution_artifacts("tandem.company-brain")
        .await
        .unwrap_err()
        .to_string()
        .contains("install receipt"));
    assert!(manager
        .export(export_request("replacement.zip"))
        .await
        .is_err());
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_pack_invalid_upgrade_preserves_prior_pointer_and_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("solution.zip");
    let mut entries = fixture();
    let key = signed(&archive, &entries);
    let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
    let manager = PackManager::new(root.path().join("packs"));
    manager.install(request(&archive)).await.unwrap();
    let original = manager
        .solution_artifacts("tandem.company-brain")
        .await
        .unwrap();
    entries[0].1 = entries[0].1.replace("0.1.0", "0.2.0");
    entries[1].1 = entries[1].1.replace("0.1.0", "0.2.0");
    entries.remove(2);
    let upgrade = root.path().join("upgrade.zip");
    signed(&upgrade, &entries);
    assert!(manager.install(request(&upgrade)).await.is_err());
    assert_eq!(manager.list().await.unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(root.path().join("packs/tandem.company-brain/current"))
            .unwrap()
            .trim(),
        "0.1.0"
    );
    assert_eq!(
        manager
            .solution_artifacts("tandem.company-brain")
            .await
            .unwrap()
            .artifacts,
        original.artifacts
    );
}
