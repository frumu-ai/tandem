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
    hosted_provider_with_fingerprint(0x5A)
}

fn hosted_provider_with_fingerprint(fingerprint: u8) -> crate::crypto::MemoryCryptoProvider {
    let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL).unwrap();
    let broker = MemoryDecryptBroker::new(config).unwrap();
    let kms = XorFixtureKms { fingerprint };
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

/// A connector-sourced chunk whose governed data class lives under
/// `enterprise_source_binding.data_class` with NO top-level `classification`.
fn source_bound_finance_chunk() -> MemoryChunk {
    let mut chunk = finance_chunk();
    chunk.id = "hosted-source-bound-1".to_string();
    chunk.metadata = Some(serde_json::json!({
        "enterprise_source_binding": {
            "binding_id": "notion-finance-db",
            "data_class": "financial_record",
        },
    }));
    chunk
}

#[tokio::test]
async fn hosted_global_chunk_is_returned_by_vector_search() {
    // TAN-668 review: the global-tier search SELECT must project crypto_envelope,
    // or hosted global rows are handed to the legacy decrypt path and dropped.
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());

    let mut global = finance_chunk();
    global.id = "hosted-global-1".to_string();
    global.tier = MemoryTier::Global;
    global.session_id = None;
    let vector = vec![0.1f32; DEFAULT_EMBEDDING_DIMENSION];
    db.store_chunk(&global, &vector).await.unwrap();

    let finance = principal("acme", vec![DataClass::FinancialRecord]);
    let results = with_decrypt_principal(
        finance,
        db.search_similar_for_tenant(
            &vector,
            MemoryTier::Global,
            None,
            None,
            &acme_finance_scope(),
            10,
            None,
            None,
        ),
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1, "hosted global chunk must survive search");
    assert!(results[0].0.content.contains("120k"));
}

#[tokio::test]
async fn source_binding_data_class_drives_the_key_scope() {
    // TAN-668 review: a connector row stamps its class under the source binding,
    // not a top-level `classification`. The key scope must seal under that class
    // (FinancialRecord), so a matching source principal decrypts and an
    // Internal-only principal without the source grant is denied.
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_memory.db");
    let db = MemoryDatabase::new(&path)
        .await
        .unwrap()
        .with_crypto_provider(hosted_provider());

    // Sealing succeeds only because the key scope's data class now matches the
    // binding's — the envelope validator rejects a mismatch, so an Internal
    // default would fail this write closed.
    db.store_chunk(
        &source_bound_finance_chunk(),
        &[0.1f32; DEFAULT_EMBEDDING_DIMENSION],
    )
    .await
    .unwrap();

    // A Finance principal that also holds the source-binding grant decrypts it.
    let source_reader = MemoryDecryptPrincipal::retrieval_gateway(
        "kb-mcp-retrieval-gateway",
        MemoryTenantScope {
            org_id: "acme".to_string(),
            workspace_id: "hq".to_string(),
            deployment_id: Some("prod".to_string()),
        },
        vec![DataClass::FinancialRecord],
        vec!["notion-finance-db".to_string()],
    );
    let chunks = with_decrypt_principal(source_reader, db.get_session_chunks("session-hosted"))
        .await
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.contains("120k"));

    // An Internal-only principal (the class the old derivation defaulted to) is
    // denied — proving the scope did not collapse to Internal.
    let internal_reader = principal("acme", vec![DataClass::Internal]);
    assert!(
        with_decrypt_principal(internal_reader, db.get_session_chunks("session-hosted"))
            .await
            .is_err(),
        "Internal principal must not decrypt a FinancialRecord source-bound row"
    );
}

fn hosted_global_record(content: &str, metadata_note: &str, provenance_note: &str) -> GlobalMemoryRecord {
    let now = Utc::now().timestamp_millis() as u64;
    GlobalMemoryRecord {
        id: format!("hosted-record-{}", uuid::Uuid::new_v4()),
        user_id: "alice".to_string(),
        source_type: "note".to_string(),
        content: content.to_string(),
        content_hash: format!("hash-{}", uuid::Uuid::new_v4()),
        run_id: "hosted-run".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: None,
        channel_tag: None,
        host_tag: None,
        metadata: Some(serde_json::json!({
            "classification": "internal",
            "owner_subject": "alice",
            "note": metadata_note,
        })),
        provenance: Some(serde_json::json!({
            "tenant_context": {
                "org_id": "acme",
                "workspace_id": "hq",
                "deployment_id": "prod"
            },
            "note": provenance_note,
        })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: None,
    }
}

fn assert_bytes_absent(path: &std::path::Path, needles: &[&str]) {
    if !path.exists() {
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    for needle in needles {
        assert!(
            !bytes.windows(needle.len()).any(|window| window == needle.as_bytes()),
            "{} contains plaintext canary {needle}",
            path.display(),
        );
    }
}

#[tokio::test]
async fn hosted_global_record_cold_decrypt_search_and_sqlite_artifacts_are_sealed() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hosted_global.sqlite");
    let backup = temp.path().join("backup.sqlite");
    let content = format!("recovery lantern {}", uuid::Uuid::new_v4());
    let metadata_note = format!("metadata lantern {}", uuid::Uuid::new_v4());
    let provenance_note = format!("provenance lantern {}", uuid::Uuid::new_v4());
    let record = hosted_global_record(&content, &metadata_note, &provenance_note);
    let db = MemoryDatabase::new(&path).await.unwrap().with_crypto_provider(hosted_provider());
    db.put_global_memory_record(&record).await.unwrap();

    let reader = principal("acme", vec![DataClass::Internal])
        .with_owner_subjects(vec!["alice".to_string()]);
    let hits = with_decrypt_principal(
        reader.clone(),
        db.search_global_memory_for_tenant_scoped(
            "acme", "hq", Some("prod"), Some("alice"), "alice", "lantern", 10,
            None, None, None, None,
        ),
    ).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record.content, content);

    // A different subject is filtered before decrypt; a wrong key cannot
    // reconstruct the DEK even when its principal has the correct scope.
    let bob = principal("acme", vec![DataClass::Internal])
        .with_owner_subjects(vec!["bob".to_string()]);
    let bob_hits = with_decrypt_principal(
        bob,
        db.search_global_memory_for_tenant_scoped(
            "acme", "hq", Some("prod"), Some("bob"), "bob", "lantern", 10,
            None, None, None, None,
        ),
    ).await.unwrap();
    assert!(bob_hits.is_empty());

    {
        let conn = db.conn.lock().await;
        let (stored, metadata, provenance, fts): (String, String, String, String) = conn.query_row(
            "SELECT m.content, m.metadata, m.provenance, f.content
             FROM memory_records m JOIN memory_records_fts f ON f.id = m.id WHERE m.id = ?1",
            params![record.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert!(stored.starts_with("tce1:"));
        assert!(metadata.starts_with("tce1:"));
        assert!(provenance.starts_with("tce1:"));
        assert_eq!(fts, stored);
        conn.execute("VACUUM INTO ?1", params![backup.to_str().unwrap()]).unwrap();
    }
    let needles = [&*content, &*metadata_note, &*provenance_note];
    assert_bytes_absent(&path, &needles);
    let wal = path.with_extension("sqlite-wal");
    assert!(wal.exists(), "WAL must exist while the writer is open");
    assert_bytes_absent(&wal, &needles);
    assert_bytes_absent(&backup, &needles);
    drop(db);

    let cold = MemoryDatabase::new(&path).await.unwrap().with_crypto_provider(hosted_provider());
    let loaded = with_decrypt_principal(
        reader.clone(),
        cold.get_global_memory_for_tenant_scoped(
            &record.id, "acme", "hq", Some("prod"), None, Some("alice"),
        ),
    ).await.unwrap().unwrap();
    assert_eq!(loaded.content, content);
    assert_eq!(loaded.metadata.unwrap()["note"], metadata_note);
    assert_eq!(loaded.provenance.unwrap()["note"], provenance_note);
    drop(cold);

    let wrong = MemoryDatabase::new(&path).await.unwrap()
        .with_crypto_provider(hosted_provider_with_fingerprint(0xA5));
    assert!(with_decrypt_principal(
        reader,
        wrong.get_global_memory_for_tenant_scoped(
            &record.id, "acme", "hq", Some("prod"), None, Some("alice"),
        ),
    ).await.is_err());
}

#[tokio::test]
async fn hosted_global_record_rejects_plaintext_legacy_and_scope_change() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("legacy.sqlite");
    let record = hosted_global_record("secret hosted content", "meta secret", "provenance secret");
    let local = MemoryDatabase::new(&path).await.unwrap();
    local.put_global_memory_record(&record).await.unwrap();
    drop(local);
    let hosted = MemoryDatabase::new(&path).await.unwrap().with_crypto_provider(hosted_provider());
    assert!(hosted.reject_legacy_global_records_for_hosted().await.is_err());
    let reader = principal("acme", vec![DataClass::Internal])
        .with_owner_subjects(vec!["alice".to_string()]);
    assert!(with_decrypt_principal(reader, hosted.get_global_memory_for_tenant_scoped(
        &record.id, "acme", "hq", Some("prod"), None, Some("alice"),
    )).await.is_err());

    let clean = MemoryDatabase::new(&temp.path().join("clean.sqlite")).await.unwrap()
        .with_crypto_provider(hosted_provider());
    clean.put_global_memory_record(&record).await.unwrap();
    let mut changed = record.metadata.clone().unwrap();
    changed["owner_subject"] = serde_json::json!("bob");
    assert!(clean.update_global_memory_context_for_tenant_scoped(
        &record.id, "acme", "hq", Some("prod"), None, Some("alice"),
        "private", false, Some(&changed), record.provenance.as_ref(),
    ).await.is_err());
    assert!(clean.search_global_memory("alice", "secret", 10, None, None, None).await.is_err());
}

#[tokio::test]
async fn hosted_atomic_global_write_and_context_update_remain_sealed() {
    use crate::store::{
        MemoryReadAccess, MemoryReadScope, MemoryStoreBatchOperation,
        MemoryStoreMutationRequest, MemoryStoreWriteRequest, MemoryWriteScope,
    };

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("atomic.sqlite");
    let db = MemoryDatabase::new(&path).await.unwrap().with_crypto_provider(hosted_provider());
    let content = format!("atomic lantern {}", uuid::Uuid::new_v4());
    let record = hosted_global_record(&content, "initial note", "initial origin");
    let tenant = acme_finance_scope();
    let metadata = serde_json::json!({
        "classification": "internal", "owner_subject": "alice",
        "note": format!("updated metadata {}", uuid::Uuid::new_v4()),
    });
    let provenance = serde_json::json!({
        "tenant_context": {"org_id": "acme", "workspace_id": "hq", "deployment_id": "prod"},
        "note": format!("updated provenance {}", uuid::Uuid::new_v4()),
    });
    db.execute_atomic_store_batch(vec![
        MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
            scope: MemoryWriteScope { tenant: tenant.clone(), org_unit: None, subject: Some("alice".to_string()) },
            record: record.clone(),
        }),
        MemoryStoreBatchOperation::Mutation(MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: MemoryReadScope {
                tenant,
                org_unit: None,
                subject: Some("alice".to_string()),
                access: MemoryReadAccess::Scoped,
            },
            id: record.id.clone(),
            visibility: "private".to_string(),
            demoted: false,
            metadata: Some(metadata.clone()),
            provenance: Some(provenance.clone()),
        }),
    ]).await.unwrap();
    let reader = principal("acme", vec![DataClass::Internal])
        .with_owner_subjects(vec!["alice".to_string()]);
    let loaded = with_decrypt_principal(reader,
        db.get_global_memory_for_tenant_scoped(&record.id, "acme", "hq", Some("prod"), None, Some("alice")),
    ).await.unwrap().unwrap();
    assert_eq!(loaded.content, content);
    assert_eq!(loaded.metadata, Some(metadata.clone()));
    assert_eq!(loaded.provenance, Some(provenance.clone()));
    let needles = [content.as_str(), metadata["note"].as_str().unwrap(), provenance["note"].as_str().unwrap()];
    assert_bytes_absent(&path.with_extension("sqlite-wal"), &needles);
}

#[tokio::test]
async fn local_key_global_search_still_matches_decrypted_content() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("local_key.sqlite");
    let db = MemoryDatabase::new(&path).await.unwrap()
        .with_crypto_provider(crate::crypto::MemoryCryptoProvider::local_key([7u8; 32]));
    let content = format!("local encrypted lantern {}", uuid::Uuid::new_v4());
    let record = hosted_global_record(&content, "local metadata", "local provenance");
    db.put_global_memory_record(&record).await.unwrap();
    let hits = db.search_global_memory("alice", "lantern", 10, None, None, None).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record.content, content);
    let scoped_hits = db.search_global_memory_for_tenant(
        "acme", "hq", Some("prod"), "alice", "lantern", 10, None, None, None,
    ).await.unwrap();
    assert_eq!(scoped_hits.len(), 1);
    let listed = db.list_global_memory("alice", Some("lantern"), None, None, 10, 0).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_bytes_absent(&path.with_extension("sqlite-wal"), &[&content]);
}
