// Hosted-KMS end-to-end memory encryption tests (TAN-668): a chunk sealed under a
// per-scope DEK on write, stored as ciphertext + envelope, and decrypted on read
// only when the caller's decrypt principal is authorized for that scope.

use crate::decrypt_broker::{MemoryDecryptBroker, MemoryDecryptBrokerConfig, MemoryDecryptPrincipal};
use crate::decrypt_context::with_decrypt_principal;
use crate::dek_cache::MemoryDekCache;
use crate::envelope_crypto::HostedMemoryEnvelopeCrypto;
use crate::kms_providers::{
    GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
    GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
};
use tandem_enterprise_contract::DataClass;

const RUNTIME_PRINCIPAL: &str = "runtime-memory-decryptor";
const PROVIDER_ID: &str = "google_cloud_kms";
const KEK_ID: &str = "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance";

/// A reversible in-process KMS for tests: wrap and unwrap are the same keyed XOR
/// involution, so a DEK round-trips without a subprocess. Asserts the scope AAD
/// is bound on both sides.
#[derive(Clone)]
struct XorFixtureKms {
    fingerprint: u8,
}

impl GoogleCloudKmsEncryptClient for XorFixtureKms {
    fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
        assert!(!request.additional_authenticated_data.is_empty());
        Ok(request
            .plaintext
            .iter()
            .map(|byte| byte ^ self.fingerprint)
            .collect())
    }
}

impl GoogleCloudKmsDecryptClient for XorFixtureKms {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        assert!(!request.additional_authenticated_data.is_empty());
        Ok(request
            .ciphertext
            .iter()
            .map(|byte| byte ^ self.fingerprint)
            .collect())
    }
}

fn hosted_provider() -> crate::crypto::MemoryCryptoProvider {
    let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL).unwrap();
    let broker = MemoryDecryptBroker::new(config).unwrap();
    let kms = XorFixtureKms { fingerprint: 0x5A };
    let wrap = GoogleCloudKmsDekWrapProvider::new(kms.clone(), RUNTIME_PRINCIPAL).unwrap();
    let unwrap = GoogleCloudKmsDekUnwrapProvider::new(kms, RUNTIME_PRINCIPAL).unwrap();
    let hosted = HostedMemoryEnvelopeCrypto::new(
        broker,
        Box::new(wrap),
        Box::new(unwrap),
        MemoryDekCache::new(64),
        PROVIDER_ID,
        RUNTIME_PRINCIPAL,
        KEK_ID,
        "1",
        0,
    );
    crate::crypto::MemoryCryptoProvider::hosted(hosted)
}

fn acme_finance_scope() -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: "acme".to_string(),
        workspace_id: "hq".to_string(),
        deployment_id: Some("prod".to_string()),
    }
}

fn principal(org: &str, classes: Vec<DataClass>) -> MemoryDecryptPrincipal {
    MemoryDecryptPrincipal::retrieval_gateway(
        "kb-mcp-retrieval-gateway",
        MemoryTenantScope {
            org_id: org.to_string(),
            workspace_id: "hq".to_string(),
            deployment_id: Some("prod".to_string()),
        },
        classes,
        Vec::new(),
    )
}

fn finance_chunk() -> MemoryChunk {
    MemoryChunk {
        id: "hosted-finance-1".to_string(),
        content: "Invoice INV-2043: ACME owes $120k, net-30, unpaid".to_string(),
        tier: MemoryTier::Session,
        session_id: Some("session-hosted".to_string()),
        project_id: None,
        source: "user_message".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: acme_finance_scope(),
        subject: None,
        created_at: Utc::now(),
        token_count: 8,
        metadata: Some(serde_json::json!({
            "classification": "financial_record",
            "owner_org_unit_id": "department/finance",
        })),
    }
}

#[tokio::test]
async fn hosted_chunk_round_trips_and_is_ciphertext_at_rest() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());

    db.store_chunk(&finance_chunk(), &[0.1f32; DEFAULT_EMBEDDING_DIMENSION])
        .await
        .unwrap();

    // A raw DB dump exposes only ciphertext + a wrapped-DEK envelope, never plaintext.
    {
        let conn = db.conn.lock().await;
        let (content, envelope, metadata): (String, Option<String>, String) = conn
            .query_row(
                "SELECT content, crypto_envelope, metadata FROM session_memory_chunks WHERE id = ?1",
                params!["hosted-finance-1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!(content.starts_with("tce1:"), "content is ciphertext");
        assert!(!content.contains("120k"));
        assert!(metadata.starts_with("tce1:"), "metadata is ciphertext");
        let envelope = envelope.expect("hosted rows carry a crypto envelope");
        assert!(envelope.contains("wrapped_dek"));
        assert!(!envelope.contains("120k"));
    }

    // An authorized Finance principal for ACME decrypts transparently.
    let finance = principal("acme", vec![DataClass::FinancialRecord]);
    let chunks = with_decrypt_principal(finance, db.get_session_chunks("session-hosted"))
        .await
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.contains("120k"));
    assert_eq!(
        chunks[0]
            .metadata
            .as_ref()
            .and_then(|m| m.get("owner_org_unit_id"))
            .and_then(|v| v.as_str()),
        Some("department/finance"),
    );
}

#[tokio::test]
async fn hosted_read_without_a_principal_fails_closed() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());
    db.store_chunk(&finance_chunk(), &[0.1f32; DEFAULT_EMBEDDING_DIMENSION])
        .await
        .unwrap();

    // No decrypt principal scoped → hosted-sealed row cannot be read (fail closed).
    assert!(db.get_session_chunks("session-hosted").await.is_err());
}

#[tokio::test]
async fn cross_tenant_principal_cannot_read_another_tenants_memory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());
    db.store_chunk(&finance_chunk(), &[0.1f32; DEFAULT_EMBEDDING_DIMENSION])
        .await
        .unwrap();

    // A principal for a different tenant is denied at the broker — a raw dump of
    // ACME's rows cannot be decrypted with another tenant's authorization.
    let other_tenant = principal("hooli", vec![DataClass::FinancialRecord]);
    let result = with_decrypt_principal(other_tenant, db.get_session_chunks("session-hosted")).await;
    assert!(result.is_err(), "cross-tenant read must be denied");
}

#[tokio::test]
async fn wrong_data_class_principal_is_denied() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());
    db.store_chunk(&finance_chunk(), &[0.1f32; DEFAULT_EMBEDDING_DIMENSION])
        .await
        .unwrap();

    // Right tenant, but no grant for the row's FinancialRecord class → denied.
    let under_scoped = principal("acme", vec![DataClass::Internal]);
    let result = with_decrypt_principal(under_scoped, db.get_session_chunks("session-hosted")).await;
    assert!(result.is_err(), "data-class denial must hold");
}

#[tokio::test]
async fn local_mode_leaves_crypto_envelope_null_and_reads_back() {
    // Backward-compat: a local/plaintext DB stores NULL crypto_envelope and reads
    // its rows without any principal — single-tenant behavior is unchanged.
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("local_memory.db");
    let db = MemoryDatabase::new(&path).await.unwrap();
    db.store_chunk(&finance_chunk(), &[0.1f32; DEFAULT_EMBEDDING_DIMENSION])
        .await
        .unwrap();

    {
        let conn = db.conn.lock().await;
        let envelope: Option<String> = conn
            .query_row(
                "SELECT crypto_envelope FROM session_memory_chunks WHERE id = ?1",
                params!["hosted-finance-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert!(envelope.is_none(), "local rows carry no crypto envelope");
    }

    let chunks = db.get_session_chunks("session-hosted").await.unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.contains("120k"));
}

#[tokio::test]
async fn hosted_layer_seals_content_and_reads_back_under_principal() {
    // Layers (L0/L1/L2 summaries) seal under the tenant's Internal scope and
    // read back only under an authorized decrypt principal, exactly like chunks.
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());
    let tenant = acme_finance_scope();

    let node_id = db
        .create_node(
            "memory://acme/hq/summary.md",
            None,
            crate::types::NodeType::File,
            None,
            &tenant,
        )
        .await
        .unwrap();
    db.create_layer(
        &node_id,
        crate::types::LayerType::L2,
        "Summary: ACME owes $120k on invoice INV-2043",
        12,
        None,
        &tenant,
    )
    .await
    .unwrap();

    // Raw column is ciphertext with an envelope.
    {
        let conn = db.conn.lock().await;
        let (content, envelope): (String, Option<String>) = conn
            .query_row(
                "SELECT content, crypto_envelope FROM memory_layers WHERE node_id = ?1",
                params![node_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(content.starts_with("tce1:"), "layer content is ciphertext");
        assert!(!content.contains("120k"));
        assert!(envelope.is_some(), "hosted layers carry a crypto envelope");
    }

    // No principal → fail closed.
    assert!(db
        .get_layer(&node_id, crate::types::LayerType::L2, &tenant)
        .await
        .is_err());

    // Internal-class principal for ACME reads it back (layers seal Internal).
    let reader = principal("acme", vec![DataClass::Internal]);
    let layer = with_decrypt_principal(
        reader,
        db.get_layer(&node_id, crate::types::LayerType::L2, &tenant),
    )
    .await
    .unwrap()
    .expect("layer present");
    assert!(layer.content.contains("120k"));
}
