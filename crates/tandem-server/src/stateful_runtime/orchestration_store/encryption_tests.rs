use serial_test::serial;
use tandem_memory::MemoryCryptoProvider;
use tandem_types::TenantContext;

use super::protected_records;

fn tenant(org: &str, actor: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, "workspace-a", Some("prod".to_string()), actor)
}

#[tokio::test]
async fn local_plaintext_round_trips_and_reads_legacy_json() {
    crate::encrypted_file_store::with_test_crypto_provider(
        MemoryCryptoProvider::plaintext(),
        None,
        async {
            let tenant = tenant("org-a", "user-a");
            let value = serde_json::json!({"secret": "value"});
            let stored = protected_records::encode(&tenant, "goal", "goal-1", &value).unwrap();
            assert!(!crate::encrypted_file_store::is_encrypted_payload(&stored));
            assert_eq!(
                protected_records::decode::<serde_json::Value>(&tenant, "goal", "goal-1", &stored,)
                    .unwrap(),
                value
            );
            assert_eq!(
                protected_records::decode::<serde_json::Value>(
                    &tenant,
                    "goal",
                    "goal-1",
                    r#"{"secret":"legacy"}"#,
                )
                .unwrap(),
                serde_json::json!({"secret": "legacy"})
            );
        },
    )
    .await;
}

#[tokio::test]
async fn protected_records_bind_tenant_scope_kind_and_id() {
    crate::encrypted_file_store::with_test_crypto_provider(
        MemoryCryptoProvider::local_key([0x5a; 32]),
        None,
        async {
            let tenant_a = tenant("org-a", "user-a");
            let value = serde_json::json!({"secret": "value"});
            let stored = protected_records::encode(&tenant_a, "run", "run-1", &value).unwrap();
            assert_eq!(
                protected_records::decode::<serde_json::Value>(
                    &tenant("org-a", "user-b"),
                    "run",
                    "run-1",
                    &stored,
                )
                .unwrap(),
                value
            );
            assert!(protected_records::decode::<serde_json::Value>(
                &tenant("org-b", "user-a"),
                "run",
                "run-1",
                &stored,
            )
            .is_err());
            assert!(protected_records::decode::<serde_json::Value>(
                &tenant_a, "goal", "run-1", &stored,
            )
            .is_err());
            assert!(protected_records::decode::<serde_json::Value>(
                &tenant_a, "run", "run-2", &stored,
            )
            .is_err());
        },
    )
    .await;
}

#[tokio::test]
async fn randomized_ciphertext_uses_stable_tenant_scoped_digest() {
    crate::encrypted_file_store::with_test_crypto_provider(
        MemoryCryptoProvider::local_key([0x33; 32]),
        None,
        async {
            let tenant_a = tenant("org-a", "user-a");
            let tenant_b = tenant("org-b", "user-a");
            let value = serde_json::json!({"status": "settled"});
            let first = protected_records::encode(&tenant_a, "wait", "wait-1", &value).unwrap();
            let second = protected_records::encode(&tenant_a, "wait", "wait-1", &value).unwrap();
            assert_ne!(first, second);
            assert_eq!(
                protected_records::digest(&tenant_a, "wait", &value).unwrap(),
                protected_records::digest(&tenant_a, "wait", &value).unwrap()
            );
            assert_ne!(
                protected_records::digest(&tenant_a, "projection", &value).unwrap(),
                protected_records::digest(&tenant_b, "projection", &value).unwrap()
            );
            assert_eq!(
                protected_records::digest(&tenant_a, "projection", &value).unwrap(),
                protected_records::digest(&tenant("org-a", "user-b"), "projection", &value,)
                    .unwrap()
            );
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn hosted_required_without_kms_fails_closed() {
    let names = [
        "TANDEM_MEMORY_ENCRYPTION_REQUIRED",
        "TANDEM_MEMORY_DECRYPT_PROVIDER",
        "TANDEM_MEMORY_LOCAL_KEY_FILE",
        "TANDEM_MEMORY_GOOGLE_KMS_ENCRYPT_COMMAND",
        "TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND",
        "TANDEM_MEMORY_KEK_ID",
        "TANDEM_MEMORY_KEK_VERSION",
    ];
    let previous = names.map(|name| std::env::var(name).ok());
    std::env::set_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED", "true");
    std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "google_cloud_kms");
    for name in &names[2..] {
        std::env::remove_var(name);
    }

    let provider = MemoryCryptoProvider::from_mode(tandem_memory::MemoryCryptoMode::HostedKms {
        provider: "google_cloud_kms".to_string(),
    });
    let error = crate::encrypted_file_store::with_test_crypto_provider(provider, None, async {
        protected_records::encode(
            &tenant("org-a", "user-a"),
            "goal",
            "goal-1",
            &serde_json::json!({"secret": "must-not-land"}),
        )
        .expect_err("hosted mode must not fall back to plaintext")
    })
    .await;

    for (name, value) in names.into_iter().zip(previous) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    assert!(format!("{error:?}").contains("refusing to store plaintext"));
}
