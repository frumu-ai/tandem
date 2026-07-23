// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::io::Write;

fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    let file = File::create(path).expect("create zip");
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, body) in entries {
        zip.start_file(*name, opts).expect("start");
        zip.write_all(body.as_bytes()).expect("write");
    }
    zip.finish().expect("finish");
}

fn write_signed_zip(path: &Path, entries: &[(&str, &str)]) -> String {
    let mut ordered = entries
        .iter()
        .map(|(name, body)| ((*name).to_string(), body.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (name, body) in &ordered {
        hasher.update((name.len() as u64).to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update((body.len() as u64).to_be_bytes());
        hasher.update(body);
    }
    let digest: [u8; 32] = hasher.finalize().into();
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let signature = signing_key.sign(&digest);
    let envelope = serde_json::json!({
        "key_id": "test-publisher",
        "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
    })
    .to_string();

    let file = File::create(path).expect("create signed zip");
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, body) in entries {
        zip.start_file(*name, opts).expect("start");
        zip.write_all(body.as_bytes()).expect("write");
    }
    zip.start_file(PACK_SIGNATURE_FILE, opts)
        .expect("start signature");
    zip.write_all(envelope.as_bytes()).expect("write signature");
    zip.finish().expect("finish");

    format!(
        "test-publisher={}",
        base64::engine::general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes())
    )
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_deref() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn detects_root_marker_only() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let ok = root.join("ok.zip");
    write_zip(
        &ok,
        &[
            ("tandempack.yaml", "name: x\nversion: 1.0.0\ntype: skill\n"),
            ("README.md", "# x"),
        ],
    );
    let nested = root.join("nested.zip");
    write_zip(
        &nested,
        &[(
            "sub/tandempack.yaml",
            "name: x\nversion: 1.0.0\ntype: skill\n",
        )],
    );
    assert!(contains_root_marker(&ok).expect("detect"));
    assert!(!contains_root_marker(&nested).expect("detect nested"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn safe_extract_blocks_traversal() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let bad = root.join("bad.zip");
    write_zip(&bad, &[("../escape.txt", "x")]);
    let out = root.join("out");
    std::fs::create_dir_all(&out).expect("mkdir out");
    let err = safe_extract_zip(&bad, &out).expect_err("should fail");
    assert!(err.to_string().contains("unsafe zip entry path"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn safe_extract_rejects_duplicate_entry_paths() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let bad = root.join("duplicate.zip");
    write_zip(
        &bad,
        &[("payload.txt", "first"), ("./payload.txt", "second")],
    );
    let out = root.join("out");
    std::fs::create_dir_all(&out).expect("mkdir out");
    let error = safe_extract_zip(&bad, &out).expect_err("duplicate paths must fail");
    assert!(error.to_string().contains("duplicate entry path"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn safe_extract_blocks_extreme_compression_ratio() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let bad = root.join("bomb.zip");
    let repeated = "A".repeat(300_000);
    write_zip(&bad, &[("payload.txt", repeated.as_str())]);
    let out = root.join("out");
    std::fs::create_dir_all(&out).expect("mkdir out");
    let err = safe_extract_zip(&bad, &out).expect_err("should fail");
    assert!(err.to_string().contains("compression ratio"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn inspect_reports_signature_and_risk_summary() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("inspect.zip");
    let trusted_key = write_signed_zip(
        &pack_zip,
        &[
            (
                "tandempack.yaml",
                "name: inspect-pack\nversion: 1.0.0\ntype: workflow\npack_id: inspect-pack\npublisher:\n  verification: verified\nentrypoints:\n  workflows:\n    - build_feature\ncapabilities:\n  required:\n    - github.create_pull_request\n  optional:\n    - slack.post_message\ncontents:\n  routines:\n    - routines/nightly.yaml\n  workflows:\n    - id: build_feature\n      path: workflows/build_feature.yaml\n  workflow_hooks:\n    - id: build_feature.task_completed.notify\n      path: hooks/notify.yaml\n",
            ),
            ("routines/nightly.yaml", "id: nightly\n"),
        ],
    );
    let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
    let manager = PackManager::new(root.join("packs"));
    let installed = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect("install");
    let inspection = manager.inspect(&installed.pack_id).await.expect("inspect");
    assert_eq!(
        inspection.trust.get("signature").and_then(|v| v.as_str()),
        Some("verified")
    );
    assert_eq!(
        inspection
            .trust
            .get("publisher_verification")
            .and_then(|v| v.as_str()),
        Some("verified")
    );
    assert_eq!(
        inspection
            .trust
            .get("verification_badge")
            .and_then(|v| v.as_str()),
        Some("verified")
    );
    assert_eq!(
        inspection
            .risk
            .get("required_capabilities_count")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        inspection
            .risk
            .get("routines_declared")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        inspection
            .permission_sheet
            .get("required_capabilities")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(1)
    );
    assert_eq!(
        inspection
            .permission_sheet
            .get("routines_declared")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(1)
    );
    assert_eq!(
        inspection
            .risk
            .get("workflows_declared")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        inspection
            .risk
            .get("workflow_hooks_declared")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        inspection
            .permission_sheet
            .get("workflows_declared")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(1)
    );
    assert_eq!(
        inspection
            .permission_sheet
            .get("workflow_hooks_declared")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(1)
    );
    assert_eq!(
        inspection
            .workflow_extensions
            .get("workflow_entrypoints")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(1)
    );
    assert_eq!(
        inspection
            .workflow_extensions
            .get("workflow_count")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        inspection
            .workflow_extensions
            .get("workflow_hook_count")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn inspect_defaults_verification_badge_to_unverified() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("inspect-unverified.zip");
    let trusted_key = write_signed_zip(
        &pack_zip,
        &[(
            "tandempack.yaml",
            "name: inspect-pack-2\nversion: 1.0.0\ntype: workflow\npack_id: inspect-pack-2\n",
        )],
    );
    let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
    let manager = PackManager::new(root.join("packs"));
    let installed = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect("install");
    let inspection = manager.inspect(&installed.pack_id).await.expect("inspect");
    assert_eq!(
        inspection
            .trust
            .get("verification_badge")
            .and_then(|v| v.as_str()),
        Some("unverified")
    );
    assert_eq!(
        inspection.trust.get("signature").and_then(|v| v.as_str()),
        Some("verified")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn install_rejects_unsigned_pack_by_default() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("unsigned.zip");
    write_zip(
        &pack_zip,
        &[(
            "tandempack.yaml",
            "name: unsigned-pack\nversion: 1.0.0\ntype: workflow\n",
        )],
    );
    let manager = PackManager::new(root.join("packs"));
    let error = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect_err("unsigned pack must fail");
    assert!(error.to_string().contains("signature is required"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn invalid_signature_cleans_extracted_staging() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("untrusted-signature.zip");
    let _ = write_signed_zip(
        &pack_zip,
        &[(
            "tandempack.yaml",
            "name: untrusted-signature-pack\nversion: 1.0.0\ntype: workflow\n",
        )],
    );
    let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", "");
    let pack_root = root.join("packs");
    let manager = PackManager::new(pack_root.clone());

    let error = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect_err("untrusted signature must fail");

    assert!(error.to_string().contains("signature key is not trusted"));
    let staged = std::fs::read_dir(pack_root.join(STAGING_DIR))
        .expect("read staging")
        .collect::<Result<Vec<_>, _>>()
        .expect("staging entries");
    assert!(
        staged.is_empty(),
        "rejected pack must not leave staging data"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn current_pointer_recovery_restores_or_discards_fixed_backup() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    let pack_parent = root.join("recover-pack");
    std::fs::create_dir_all(&pack_parent).expect("mkdir");
    let current = pack_parent.join(CURRENT_FILE);
    let backup = pack_parent.join(CURRENT_BACKUP_FILE);

    std::fs::write(&backup, "1.0.0\n").expect("write interrupted backup");
    recover_current_pointer_backup(&pack_parent)
        .await
        .expect("restore interrupted pointer");
    assert_eq!(
        std::fs::read_to_string(&current).expect("current"),
        "1.0.0\n"
    );
    assert!(!backup.exists());

    std::fs::write(&current, "2.0.0\n").expect("write committed pointer");
    std::fs::write(&backup, "1.0.0\n").expect("write stale backup");
    recover_current_pointer_backup(&pack_parent)
        .await
        .expect("discard committed backup");
    assert_eq!(
        std::fs::read_to_string(&current).expect("current"),
        "2.0.0\n"
    );
    assert!(!backup.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn install_rolls_back_files_when_index_commit_fails() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("rollback.zip");
    let trusted_key = write_signed_zip(
        &pack_zip,
        &[(
            "tandempack.yaml",
            "name: rollback-pack\nversion: 1.0.0\ntype: workflow\npack_id: rollback-pack\n",
        )],
    );
    let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
    let pack_root = root.join("packs");
    std::fs::create_dir_all(pack_root.join(INDEX_FILE))
        .expect("make index path unwritable as a file");
    let manager = PackManager::new(pack_root.clone());

    let error = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect_err("index commit failure must abort install");

    assert!(error.to_string().contains("installation rolled back"));
    assert!(
        !pack_root.join("rollback-pack").join("1.0.0").exists(),
        "failed index persistence must not leave an installed pack directory"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn install_rolls_back_index_and_files_when_current_pointer_commit_fails() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("pointer-rollback.zip");
    let trusted_key = write_signed_zip(
        &pack_zip,
        &[(
            "tandempack.yaml",
            "name: pointer-rollback-pack\nversion: 1.0.0\ntype: workflow\npack_id: pointer-rollback-pack\n",
        )],
    );
    let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
    let pack_root = root.join("packs");
    std::fs::create_dir_all(pack_root.join("pointer-rollback-pack").join(CURRENT_FILE))
        .expect("make current pointer path a directory");
    let manager = PackManager::new(pack_root.clone());

    let error = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect_err("current pointer failure must abort install");

    assert!(error.to_string().contains("installation rolled back"));
    assert!(manager.list().await.expect("list").is_empty());
    assert!(
        !pack_root
            .join("pointer-rollback-pack")
            .join("1.0.0")
            .exists(),
        "failed current-pointer persistence must not leave installed files"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn uninstall_rolls_back_index_and_files_when_current_pointer_commit_fails() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let pack_zip = root.join("uninstall-pointer-rollback.zip");
    let trusted_key = write_signed_zip(
        &pack_zip,
        &[(
            "tandempack.yaml",
            "name: uninstall-pointer-pack\nversion: 1.0.0\ntype: workflow\npack_id: uninstall-pointer-pack\n",
        )],
    );
    let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
    let pack_root = root.join("packs");
    let manager = PackManager::new(pack_root.clone());
    let installed = manager
        .install(PackInstallRequest {
            path: Some(pack_zip.to_string_lossy().to_string()),
            url: None,
            expected_sha256: None,
            source: Value::Null,
        })
        .await
        .expect("install");
    let current = pack_root.join("uninstall-pointer-pack").join(CURRENT_FILE);
    std::fs::remove_file(&current).expect("remove current pointer");
    std::fs::create_dir(&current).expect("replace current pointer with directory");

    let error = manager
        .uninstall(PackUninstallRequest {
            pack_id: Some(installed.pack_id.clone()),
            name: None,
            version: None,
        })
        .await
        .expect_err("current pointer failure must abort uninstall");

    assert!(error.to_string().contains("uninstall rolled back"));
    assert_eq!(manager.list().await.expect("list").len(), 1);
    assert!(Path::new(&installed.install_path).exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scan_embedded_secrets_finds_real_and_ignores_examples() {
    let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    let real = root.join("resources").join("token.txt");
    std::fs::create_dir_all(real.parent().expect("parent")).expect("mkdir resources");
    std::fs::write(&real, "token=ghp_example_not_real_but_pattern").expect("write real");
    let example = root.join("secrets.example.env");
    std::fs::write(&example, "API_KEY=sk-live-example").expect("write example");
    let findings = scan_embedded_secrets(&root).expect("scan");
    assert_eq!(findings.len(), 1);
    assert!(findings[0].contains("resources/token.txt"));
    let _ = std::fs::remove_dir_all(root);
}
