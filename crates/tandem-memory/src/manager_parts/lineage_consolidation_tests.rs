use super::*;
use crate::provider_egress::test_environment::{env_lock, EnvRestore};
use crate::types::GlobalMemoryRecord;
use crate::{CanonicalInputReference, CanonicalMemoryRestriction, DerivedMemoryLineage};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use tandem_data_boundary::{DataBoundaryTenantRef, ProviderEgressAuthority, SensitiveDataClass};
use tandem_enterprise_contract::{
    AssertionMetadata, AuthorityChain, DataBoundary, DataClass, PrincipalRef, RequestPrincipal,
    ResourceKind, ResourceRef, ResourceScope, StrictTenantContext, TenantContext,
};
use tandem_providers::{AppConfig, Provider};
use tandem_types::ProviderInfo;

struct SummaryProvider {
    calls: Arc<AtomicUsize>,
    delete_source: Option<(Arc<dyn MemoryStore>, String)>,
}

#[async_trait]
impl Provider for SummaryProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "capture".into(),
            name: "Synthetic summary".into(),
            models: Vec::new(),
        }
    }
    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((store, id)) = &self.delete_source {
            store
                .mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {
                    scope: MemoryReadScope::tenant(tenant()),
                    id: id.clone(),
                })
                .await?;
        }
        Ok("orchard lighthouse summary".into())
    }
}

fn tenant() -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: "lineage-org".into(),
        workspace_id: "lineage-workspace".into(),
        deployment_id: None,
    }
}

fn read_scope() -> MemoryReadScope {
    MemoryReadScope {
        tenant: tenant(),
        org_unit: Some("finance".into()),
        subject: Some("alice".into()),
        access: crate::MemoryReadAccess::Scoped,
    }
}

fn filter() -> crate::types::MemoryAccessFilter {
    filter_with_data_classes(&[])
}

fn filter_with_data_classes(classes: &[DataClass]) -> crate::types::MemoryAccessFilter {
    let now = Utc::now().timestamp_millis().max(0) as u64;
    // Unrestricted projections without Read grants deliberately fall back to
    // the governed default. Supply only the fixture's actual authorized classes;
    // this class boundary does not grant access to source/knowledge resources.
    let mut allowed = vec![DataClass::Internal];
    for class in classes {
        if !allowed.contains(class) {
            allowed.push(*class);
        }
    }
    let strict = StrictTenantContext::new(
        TenantContext::explicit_user_workspace("lineage-org", "lineage-workspace", None, "alice"),
        PrincipalRef::human_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(
            "alice",
            "consolidation-test",
        )),
        ResourceScope::root(ResourceRef::new(
            "lineage-org",
            "lineage-workspace",
            ResourceKind::Workspace,
            "lineage-workspace",
        )),
        AssertionMetadata::new("test", "runtime", now, now + 60_000, "consolidation-test"),
    )
    .with_data_boundary(DataBoundary::allow(allowed));
    crate::types::MemoryAccessFilter::strict(strict, now)
        .with_caller_subject("alice")
        .with_caller_org_units(["finance".to_string()])
}

fn request() -> ScopedMemoryConsolidationRequest {
    ScopedMemoryConsolidationRequest {
        tenant_scope: tenant(),
        org_unit: Some("finance".into()),
        subject: Some("alice".into()),
        project_id: "lineage-project".into(),
        session_id: "lineage-session".into(),
    }
}

fn config() -> MemoryConsolidationConfig {
    MemoryConsolidationConfig {
        enabled: true,
        ..Default::default()
    }
}

fn egress(classes: Arc<std::sync::Mutex<Vec<SensitiveDataClass>>>) -> MemoryProviderEgressContext {
    MemoryProviderEgressContext::new(
        ProviderEgressAuthority::new(DataBoundaryTenantRef {
            organization_id: Some("lineage-org".into()),
            workspace_id: Some("lineage-workspace".into()),
            deployment_id: None,
        })
        .with_run_id("lineage-run")
        .with_session_id("lineage-session"),
    )
    .with_audit_sink(Arc::new(move |event| {
        *classes.lock().unwrap() = event.semantic_data_classes;
        Box::pin(async { Ok(()) })
    }))
}

fn source(id: &str, class: DataClass) -> GlobalMemoryRecord {
    let content = format!("orchard lighthouse {id}");
    GlobalMemoryRecord {
        id: id.into(),
        user_id: "alice".into(),
        source_type: "fact".into(),
        content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
        content,
        run_id: "lineage-run".into(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("lineage-project".into()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(serde_json::json!({"owner_org_unit_id":"finance","classification":class})),
        provenance: Some(serde_json::json!({"tenant_context":tenant()})),
        redaction_status: "passed".into(),
        redaction_count: 0,
        visibility: "private".into(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
        expires_at_ms: None,
    }
}

async fn put_source(store: &dyn MemoryStore, record: GlobalMemoryRecord) {
    store
        .write(MemoryStoreWriteRequest::GlobalRecord {
            scope: MemoryWriteScope {
                tenant: tenant(),
                org_unit: Some("finance".into()),
                subject: None,
            },
            record,
        })
        .await
        .unwrap();
}

async fn seed(
    store: &dyn MemoryStore,
    records: &[GlobalMemoryRecord],
    native: bool,
) -> DerivedMemoryLineage {
    let mut sources = Vec::new();
    for record in records {
        put_source(store, record.clone()).await;
        sources.push(CanonicalMemoryRestriction::from_global_record(record, &tenant()).unwrap());
    }
    let mut inputs = sources
        .iter()
        .map(|source| CanonicalInputReference::Memory {
            source: source.source_reference(),
        })
        .collect::<Vec<_>>();
    if native {
        inputs.push(CanonicalInputReference::SessionMessage {
            session_id: "lineage-session".into(),
            message_id: "native-message".into(),
            body_digest: "a".repeat(64),
        });
    }
    let lineage = DerivedMemoryLineage::new(
        Some("alice".into()),
        Some("finance".into()),
        sources,
        inputs,
    )
    .unwrap();
    let chunk = MemoryChunk {
        id: "contributing-chunk".into(),
        content: "orchard lighthouse contributor".into(),
        tier: MemoryTier::Session,
        session_id: Some("lineage-session".into()),
        project_id: Some("lineage-project".into()),
        source: "derived".into(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: tenant(),
        subject: Some("alice".into()),
        created_at: Utc::now(),
        token_count: 4,
        metadata: crate::metadata_with_derived_lineage(
            Some(serde_json::json!({"owner_subject":"alice","owner_org_unit_id":"finance"})),
            &lineage,
        )
        .unwrap(),
    };
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope {
                tenant: tenant(),
                org_unit: Some("finance".into()),
                subject: Some("alice".into()),
            },
            chunk,
            embedding: vec![1.0; crate::types::DEFAULT_EMBEDDING_DIMENSION],
        })
        .await
        .unwrap();
    lineage
}

fn manager(store: Arc<dyn MemoryStore>) -> MemoryManager {
    MemoryManager::new_with_store(
        store,
        EmbeddingService::deterministic_for_tests(crate::types::DEFAULT_EMBEDDING_DIMENSION),
    )
    .unwrap()
}

async fn registry(
    calls: Arc<AtomicUsize>,
    delete_source: Option<(Arc<dyn MemoryStore>, String)>,
) -> ProviderRegistry {
    let providers = ProviderRegistry::new(AppConfig::default());
    providers
        .replace_for_test(
            vec![Arc::new(SummaryProvider {
                calls,
                delete_source,
            })],
            Some("capture".into()),
        )
        .await;
    providers
}

async fn chunks(store: &dyn MemoryStore, selector: MemoryChunkSelector) -> Vec<MemoryChunk> {
    match store
        .read(MemoryStoreReadRequest::Chunks {
            scope: read_scope(),
            selector,
            limit: None,
        })
        .await
        .unwrap()
    {
        MemoryStoreReadResult::Chunks(rows) => rows,
        other => panic!("{other:?}"),
    }
}

fn ordinary_chunk(id: &str, extra: serde_json::Value) -> MemoryChunk {
    let mut metadata = serde_json::json!({"owner_subject":"alice","owner_org_unit_id":"finance"});
    metadata
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    MemoryChunk {
        id: id.into(),
        content: format!("orchard lighthouse ordinary {id}"),
        tier: MemoryTier::Session,
        session_id: Some("lineage-session".into()),
        project_id: Some("lineage-project".into()),
        source: "message".into(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: tenant(),
        subject: Some("alice".into()),
        created_at: Utc::now(),
        token_count: 5,
        metadata: Some(metadata),
    }
}

async fn put_ordinary(store: &dyn MemoryStore, chunk: MemoryChunk) {
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope {
                tenant: tenant(),
                org_unit: Some("finance".into()),
                subject: Some("alice".into()),
            },
            chunk,
            embedding: vec![1.0; crate::types::DEFAULT_EMBEDDING_DIMENSION],
        })
        .await
        .unwrap();
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_preserves_lineage_classes_and_hides_summary_after_source_deletion() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn MemoryStore> = Arc::new(
        MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
            .await
            .unwrap(),
    );
    let financial = source("financial-source", DataClass::FinancialRecord);
    let code = source("code-source", DataClass::SourceCode);
    let lineage = seed(store.as_ref(), &[financial.clone(), code], false).await;
    let access = filter_with_data_classes(&[DataClass::FinancialRecord, DataClass::SourceCode]);
    let proof = crate::resolve_derived_lineage(store.as_ref(), &read_scope(), &lineage)
        .await
        .unwrap();
    assert!(
        access
            .clone()
            .with_resolved_derived_lineage(proof)
            .decision_for_derived_lineage(&lineage)
            .allowed,
        "the healthy fixture must authorize its actual contributing classes"
    );
    let manager = manager(store.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let providers = registry(calls.clone(), None).await;
    let classes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let egress = egress(classes.clone());
    assert!(
        manager
            .consolidate_scoped_session(&request(), &providers, &config(), &egress)
            .await
            .is_err(),
        "the ordinary wrapper cannot invent derivative authority"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let result = manager
        .consolidate_scoped_session_with_access_filter(
            &request(),
            &providers,
            &config(),
            &egress,
            Some(&access),
        )
        .await
        .unwrap();
    assert_eq!(result.as_deref(), Some("orchard lighthouse summary"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let summaries = chunks(
        store.as_ref(),
        MemoryChunkSelector::project("lineage-project"),
    )
    .await;
    assert_eq!(summaries.len(), 1);
    let retained = DerivedMemoryLineage::from_metadata(summaries[0].metadata.as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(retained, lineage);
    assert_eq!(
        crate::types::data_class_from_metadata(summaries[0].metadata.as_ref()),
        Some(DataClass::FinancialRecord)
    );
    let observed = classes.lock().unwrap();
    assert!(observed.contains(&SensitiveDataClass::Financial));
    assert!(observed.contains(&SensitiveDataClass::SourceCode));
    drop(observed);
    assert!(chunks(
        store.as_ref(),
        MemoryChunkSelector::session("lineage-session")
    )
    .await
    .is_empty());
    store
        .mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope: read_scope(),
            id: financial.id,
        })
        .await
        .unwrap();
    assert!(
        chunks(
            store.as_ref(),
            MemoryChunkSelector::project("lineage-project")
        )
        .await
        .is_empty(),
        "the durable summary inherits canonical source deletion"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_denied_grant_or_missing_native_resolver_never_dispatches_provider() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    for native in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn MemoryStore> = Arc::new(
            MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
                .await
                .unwrap(),
        );
        let mut record = source("restricted-source", DataClass::FinancialRecord);
        if !native {
            record.metadata.as_mut().unwrap()["enterprise_source_binding"] = serde_json::json!({"binding_id":"restricted-binding",
            "resource_ref":ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::DocumentCollection,"restricted-binding"),
            "data_class":"financial_record"});
        }
        seed(store.as_ref(), &[record], native).await;
        let access = filter_with_data_classes(&[DataClass::FinancialRecord]);
        let manager = manager(store.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = registry(calls.clone(), None).await;
        let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
        let error = manager
            .consolidate_scoped_session_with_access_filter(
                &request(),
                &providers,
                &config(),
                &egress,
                Some(&access),
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains(if native {
                "authority unavailable"
            } else {
                "no_matching_allow_grant"
            }),
            "{error}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            chunks(
                store.as_ref(),
                MemoryChunkSelector::session("lineage-session")
            )
            .await
            .len(),
            1
        );
        assert!(chunks(
            store.as_ref(),
            MemoryChunkSelector::project("lineage-project")
        )
        .await
        .is_empty());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_rechecks_source_after_provider_before_summary_write() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn MemoryStore> = Arc::new(
        MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
            .await
            .unwrap(),
    );
    let record = source("provider-revoked-source", DataClass::FinancialRecord);
    seed(store.as_ref(), &[record.clone()], false).await;
    let access = filter_with_data_classes(&[DataClass::FinancialRecord]);
    let manager = manager(store.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let providers = registry(calls.clone(), Some((store.clone(), record.id.clone()))).await;
    let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
    let error = manager
        .consolidate_scoped_session_with_access_filter(
            &request(),
            &providers,
            &config(),
            &egress,
            Some(&access),
        )
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("authority unavailable"),
        "{error}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the actual provider reached the source-deletion boundary"
    );
    assert!(chunks(
        store.as_ref(),
        MemoryChunkSelector::project("lineage-project")
    )
    .await
    .is_empty());
    put_source(store.as_ref(), record).await;
    assert_eq!(chunks(store.as_ref(),MemoryChunkSelector::session("lineage-session")).await.len(),1,
        "restoring canonical source proves the contributor was not removed by a failed consolidation");
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_mixed_denied_ordinary_source_never_dispatches_or_replaces() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    for knowledge in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn MemoryStore> = Arc::new(
            MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
                .await
                .unwrap(),
        );
        let lineage = seed(
            store.as_ref(),
            &[source("allowed-source", DataClass::Internal)],
            false,
        )
        .await;
        let proof = crate::resolve_derived_lineage(store.as_ref(), &read_scope(), &lineage)
            .await
            .unwrap();
        let access = filter_with_data_classes(&[DataClass::FinancialRecord, DataClass::Confidential])
            .with_resolved_derived_lineage(proof);
        let derived = chunks(
            store.as_ref(),
            MemoryChunkSelector::session("lineage-session"),
        )
        .await;
        assert!(
            access.allows_chunk(&derived[0]),
            "the real derived contributor is independently authorized"
        );
        let resource = ResourceRef::new(
            "lineage-org",
            "lineage-workspace",
            ResourceKind::DocumentCollection,
            "denied-binding",
        );
        let mut denied = ordinary_chunk(
            "denied-ordinary",
            serde_json::json!({
                "enterprise_source_binding": {"binding_id":"denied-binding", "resource_ref":resource, "data_class":"financial_record"}
            }),
        );
        if knowledge {
            denied.metadata = crate::metadata_with_knowledge_scope(
                denied.metadata,
                &crate::KnowledgeScopePolicy {
                    registry_id: "denied-registry".into(),
                    resource_ref: ResourceRef::new(
                        "lineage-org",
                        "lineage-workspace",
                        ResourceKind::KnowledgeSpace,
                        "denied-space",
                    )
                    .with_project_id("lineage-project"),
                    data_class: DataClass::Confidential,
                    collection_id: None,
                    source_binding_id: None,
                    source_object_id: None,
                    owner_org_unit_id: None,
                    risk_tier: None,
                    allowed_workflow_phases: Vec::new(),
                    allowed_write_tiers: vec![crate::GovernedMemoryTier::Session],
                    allowed_promotion_tiers: Vec::new(),
                    retention_expires_at_ms: None,
                    required_trust_label: None,
                    promotion_requires_approval: false,
                },
            );
        }
        let denial = access.decision_for_chunk(&denied);
        assert!(
            !denial.allowed,
            "same private owner cannot replace the absent source/knowledge grant"
        );
        assert_eq!(
            denial.reason.as_deref(),
            Some("no_matching_allow_grant"),
            "the negative must reach the missing resource grant, not a class-boundary denial"
        );
        put_ordinary(store.as_ref(), denied).await;
        let manager = manager(store.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = registry(calls.clone(), None).await;
        let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
        let error = manager
            .consolidate_scoped_session_with_access_filter(
                &request(),
                &providers,
                &config(),
                &egress,
                Some(&access),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("contributor denied"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            chunks(
                store.as_ref(),
                MemoryChunkSelector::session("lineage-session")
            )
            .await
            .len(),
            2
        );
        assert!(chunks(
            store.as_ref(),
            MemoryChunkSelector::project("lineage-project")
        )
        .await
        .is_empty());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_governed_unrepresentable_ordinary_and_mixed_inputs_fail_closed() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    for kind in ["mixed", "classified", "retained", "source_path"] {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn MemoryStore> = Arc::new(
            MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
                .await
                .unwrap(),
        );
        if kind == "mixed" {
            seed(
                store.as_ref(),
                &[source("allowed-source", DataClass::Internal)],
                false,
            )
            .await;
        }
        let extra = match kind {
            "classified" => serde_json::json!({"classification":"financial_record"}),
            "retained" => {
                serde_json::json!({"retention_expires_at_ms":Utc::now().timestamp_millis().max(0) as u64 + 60_000})
            }
            _ => serde_json::json!({}),
        };
        let mut ordinary = ordinary_chunk("allowed-ordinary", extra);
        if kind == "source_path" {
            ordinary.source_path = Some("governed/source.txt".into());
        }
        let access = if kind == "classified" {
            filter_with_data_classes(&[DataClass::FinancialRecord])
        } else {
            filter()
        };
        assert!(
            access.allows_chunk(&ordinary),
            "fixture must reach the unrepresentable-disposition gate, not an unrelated read denial"
        );
        put_ordinary(store.as_ref(), ordinary).await;
        let manager = manager(store.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = registry(calls.clone(), None).await;
        let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
        let error = manager
            .consolidate_scoped_session_with_access_filter(
                &request(),
                &providers,
                &config(),
                &egress,
                Some(&access),
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains(if kind == "mixed" {
                "mixed contributors lack canonical lineage"
            } else {
                "ordinary restrictions lack canonical lineage"
            }),
            "{error}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            chunks(
                store.as_ref(),
                MemoryChunkSelector::session("lineage-session")
            )
            .await
            .len(),
            if kind == "mixed" { 2 } else { 1 }
        );
        assert!(chunks(
            store.as_ref(),
            MemoryChunkSelector::project("lineage-project")
        )
        .await
        .is_empty());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_plain_ordinary_current_and_legacy_paths_remain_private_positives() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    for governed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn MemoryStore> = Arc::new(
            MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
                .await
                .unwrap(),
        );
        put_ordinary(
            store.as_ref(),
            ordinary_chunk("ordinary-default", serde_json::json!({})),
        )
        .await;
        put_ordinary(
            store.as_ref(),
            ordinary_chunk(
                "ordinary-internal",
                serde_json::json!({"classification":"internal"}),
            ),
        )
        .await;
        let manager = manager(store.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = registry(calls.clone(), None).await;
        let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
        let access = filter();
        let result = if governed {
            manager
                .consolidate_scoped_session_with_access_filter(
                    &request(),
                    &providers,
                    &config(),
                    &egress,
                    Some(&access),
                )
                .await
        } else {
            manager
                .consolidate_scoped_session(&request(), &providers, &config(), &egress)
                .await
        }
        .unwrap();
        assert_eq!(result.as_deref(), Some("orchard lighthouse summary"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let summaries = chunks(
            store.as_ref(),
            MemoryChunkSelector::project("lineage-project"),
        )
        .await;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].subject.as_deref(), Some("alice"));
        assert_eq!(
            crate::types::owner_subject_from_metadata(summaries[0].metadata.as_ref()).as_deref(),
            Some("alice")
        );
        assert_eq!(
            summaries[0].metadata.as_ref().unwrap()["consolidation_provenance"]["source_count"],
            serde_json::json!(2)
        );
        assert!(
            DerivedMemoryLineage::from_metadata(summaries[0].metadata.as_ref())
                .unwrap()
                .is_none()
        );
        assert!(chunks(
            store.as_ref(),
            MemoryChunkSelector::session("lineage-session")
        )
        .await
        .is_empty());
        let mut foreign_scope = read_scope();
        foreign_scope.subject = Some("bob".into());
        match store
            .read(MemoryStoreReadRequest::Chunks {
                scope: foreign_scope,
                selector: MemoryChunkSelector::project("lineage-project"),
                limit: None,
            })
            .await
            .unwrap()
        {
            MemoryStoreReadResult::Chunks(rows) => assert!(
                rows.is_empty(),
                "ordinary consolidation preserves the actual private owner"
            ),
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_legacy_wrapper_rejects_restricted_ordinary_before_provider() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "capture=local"),
    ]);
    for kind in [
        "financial_record",
        "source_binding",
        "knowledge_scope",
        "retention",
        "source_path",
        "mixed",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn MemoryStore> = Arc::new(
            MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
                .await
                .unwrap(),
        );
        if kind == "mixed" {
            seed(
                store.as_ref(),
                &[source("allowed-source", DataClass::Internal)],
                false,
            )
            .await;
        }
        let extra = match kind {
            "financial_record" => serde_json::json!({"classification":"financial_record"}),
            "source_binding" => serde_json::json!({"enterprise_source_binding": {
                "binding_id":"restricted-binding", "data_class":"financial_record",
                "resource_ref":ResourceRef::new("lineage-org", "lineage-workspace", ResourceKind::DocumentCollection, "restricted-binding"),
            }}),
            "retention" => {
                serde_json::json!({"retention_expires_at_ms":Utc::now().timestamp_millis().max(0) as u64 + 60_000})
            }
            _ => serde_json::json!({}),
        };
        let mut ordinary = ordinary_chunk("restricted-ordinary", extra);
        if kind == "knowledge_scope" {
            ordinary.metadata = crate::metadata_with_knowledge_scope(
                ordinary.metadata,
                &crate::KnowledgeScopePolicy {
                    registry_id: "restricted-registry".into(),
                    resource_ref: ResourceRef::new(
                        "lineage-org",
                        "lineage-workspace",
                        ResourceKind::KnowledgeSpace,
                        "restricted-space",
                    )
                    .with_project_id("lineage-project"),
                    data_class: DataClass::Confidential,
                    collection_id: None,
                    source_binding_id: None,
                    source_object_id: None,
                    owner_org_unit_id: None,
                    risk_tier: None,
                    allowed_workflow_phases: Vec::new(),
                    allowed_write_tiers: vec![crate::GovernedMemoryTier::Session],
                    allowed_promotion_tiers: Vec::new(),
                    retention_expires_at_ms: None,
                    required_trust_label: None,
                    promotion_requires_approval: false,
                },
            );
        }
        if kind == "source_path" {
            ordinary.source_path = Some("governed/source.txt".into());
        }
        put_ordinary(store.as_ref(), ordinary).await;
        let manager = manager(store.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = registry(calls.clone(), None).await;
        let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
        // Exercise the actual compatibility API, with no access filter at all.
        let error = manager
            .consolidate_scoped_session(&request(), &providers, &config(), &egress)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains(if kind == "mixed" {
                "mixed contributors lack canonical lineage"
            } else {
                "ordinary restrictions lack canonical lineage"
            }),
            "{kind}: {error}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{kind}");
        assert_eq!(
            chunks(
                store.as_ref(),
                MemoryChunkSelector::session("lineage-session")
            )
            .await
            .len(),
            if kind == "mixed" { 2 } else { 1 },
            "{kind}"
        );
        assert!(
            chunks(
                store.as_ref(),
                MemoryChunkSelector::project("lineage-project")
            )
            .await
            .is_empty(),
            "{kind}"
        );
    }
}
