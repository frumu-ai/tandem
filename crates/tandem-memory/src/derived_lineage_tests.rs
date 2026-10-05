use super::*;
use crate::db::MemoryDatabase;
use crate::store::*;
use crate::types::{GovernedReadMode, MemoryChunk, MemoryTier};
use serde_json::json;
use tandem_enterprise_contract::{AccessPermission, AssertionMetadata, AuthorityChain, DataBoundary,
    GrantSource, PrincipalRef, RequestPrincipal, ResourceScope, ScopedGrant, StrictTenantContext,
    TenantContext};

fn tenant() -> MemoryTenantScope {
    MemoryTenantScope {org_id: "lineage-org".into(), workspace_id: "lineage-workspace".into(), deployment_id: None}
}

fn row(id: &str, owner: Option<&str>, unit: Option<&str>) -> GlobalMemoryRecord {
    let content = format!("orchard lighthouse {id}");
    GlobalMemoryRecord {
        id: id.into(), user_id: "alice".into(), source_type: "fact".into(),
        content_hash: format!("{:x}", Sha256::digest(content.as_bytes())), content,
        run_id: "lineage-run".into(), session_id: None, message_id: None, tool_name: None,
        project_tag: Some("lineage-project".into()), channel_tag: None, host_tag: None,
        metadata: Some(json!({"owner_subject":owner,"owner_org_unit_id":unit,"tenant_shared":unit.is_none()})),
        provenance: Some(json!({"tenant_context":tenant()})), redaction_status: "passed".into(), redaction_count: 0,
        // This is the real memory_put visibility even for private=false writes.
        visibility: "private".into(), demoted: false, score_boost: 0.0,
        created_at_ms: 1_000, updated_at_ms: 1_000, expires_at_ms: None,
    }
}

fn scope(subject: &str, unit: &str) -> MemoryReadScope {
    MemoryReadScope {tenant:tenant(),org_unit:Some(unit.into()),subject:Some(subject.into()),access:MemoryReadAccess::Scoped}
}

fn strict(subject: &str) -> StrictTenantContext {
    StrictTenantContext::new(
        TenantContext::explicit_user_workspace("lineage-org", "lineage-workspace", None, subject),
        PrincipalRef::human_user(subject),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(subject, "lineage-test")),
        ResourceScope::root(ResourceRef::new("lineage-org", "lineage-workspace", ResourceKind::Workspace, "lineage-workspace")),
        AssertionMetadata::new("test", "runtime", 1_000, 9_000, "lineage-test"),
    ).with_data_boundary(DataBoundary::unrestricted())
}

fn filter(subject: &str, unit: &str) -> MemoryAccessFilter {
    MemoryAccessFilter::strict(strict(subject), 2_000).with_caller_subject(subject)
        .with_caller_org_units([unit.to_string()])
}

fn lineage(source: &GlobalMemoryRecord, owner: Option<&str>, unit: Option<&str>) -> DerivedMemoryLineage {
    let restriction = CanonicalMemoryRestriction::from_global_record(source, &tenant()).unwrap();
    let input = CanonicalInputReference::Memory {source:restriction.source_reference()};
    DerivedMemoryLineage::new(owner.map(str::to_string),unit.map(str::to_string),vec![restriction],vec![input]).unwrap()
}

fn derived(id: &str, lineage: &DerivedMemoryLineage) -> GlobalMemoryRecord {
    let mut record = row(id, lineage.owner_subject.as_deref(), lineage.owner_org_unit_id.as_deref());
    record.metadata = metadata_with_derived_lineage(record.metadata, lineage).unwrap();
    record
}

async fn put(store: &dyn MemoryStore, record: GlobalMemoryRecord) -> crate::types::GlobalMemoryWriteResult {
    let request = MemoryStoreWriteRequest::GlobalRecord {
        scope: MemoryWriteScope {tenant:tenant(),org_unit:owner_org_unit_id_from_metadata(record.metadata.as_ref()),
            subject:owner_subject_from_metadata(record.metadata.as_ref())}, record,
    };
    match store.write(request).await.unwrap() { MemoryStoreWriteResult::GlobalRecord(result) => result, other => panic!("{other:?}") }
}

async fn get(store: &dyn MemoryStore, id: &str, scope: MemoryReadScope) -> Option<GlobalMemoryRecord> {
    match store.read(MemoryStoreReadRequest::GlobalRecord {scope,id:id.into()}).await.unwrap() {
        MemoryStoreReadResult::GlobalRecord(record) => record, other => panic!("{other:?}"),
    }
}

async fn list_and_search_contain(store: &dyn MemoryStore, id: &str) -> Vec<bool> {
    let mut results = Vec::new();
    for request in [
        MemoryStoreQueryRequest::ListGlobalRecords {scope:scope("bob","finance"),user_id:"alice".into(),query:None,
            project_tag:Some("lineage-project".into()),channel_tag:None,limit:50,offset:0},
        MemoryStoreQueryRequest::SearchGlobalRecords {scope:scope("bob","finance"),user_id:"alice".into(),query:"orchard".into(),
            project_tag:Some("lineage-project".into()),limit:50},
    ] {
        results.push(match store.query(request).await.unwrap() {
            MemoryStoreQueryResult::GlobalRecords(rows) => rows.iter().any(|row| row.id == id),
            MemoryStoreQueryResult::GlobalSearchHits(hits) => hits.iter().any(|hit| hit.record.id == id),
            other => panic!("{other:?}"),
        });
    }
    results
}

#[test]
fn private_owner_is_additional_to_source_grants_and_knowledge_scope() {
    let resource = ResourceRef::new("lineage-org", "lineage-workspace", ResourceKind::DocumentCollection, "binding");
    let mut record = row("source", Some("alice"), Some("finance"));
    record.metadata.as_mut().unwrap()["enterprise_source_binding"] = json!({
        "binding_id":"binding", "resource_ref":resource, "data_class":"financial_record"
    });
    let grant = |subject: &str| ScopedGrant::new("grant",PrincipalRef::human_user(subject),resource.clone(),GrantSource::Direct)
        .with_permissions(vec![AccessPermission::Read]).with_data_classes(vec![DataClass::FinancialRecord]);
    let alice = MemoryAccessFilter::strict(strict("alice").with_grants(vec![grant("alice")]),2_000).with_caller_subject("alice");
    let bob = MemoryAccessFilter::strict(strict("bob").with_grants(vec![grant("bob")]),2_000).with_caller_subject("bob");
    assert!(alice.allows_global_record(&record));
    assert!(!bob.allows_global_record(&record), "a source grant cannot erase private ownership");
    let policy = KnowledgeScopePolicy {
        registry_id:"registry".into(),resource_ref:resource,data_class:DataClass::FinancialRecord,
        collection_id:None,source_binding_id:Some("binding".into()),source_object_id:None,
        owner_org_unit_id:None,risk_tier:None,allowed_workflow_phases:vec![],allowed_write_tiers:vec![],
        allowed_promotion_tiers:vec![],retention_expires_at_ms:None,required_trust_label:None,promotion_requires_approval:false,
    };
    record.metadata = crate::metadata_with_knowledge_scope(record.metadata,&policy);
    assert!(alice.allows_global_record(&record));
    assert!(!bob.allows_global_record(&record), "a knowledge registry cannot erase private ownership");
}

#[tokio::test]
#[serial_test::serial]
async fn canonical_department_shared_source_allows_peer_and_denies_foreign_department() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let source = row("shared-source",None,Some("finance"));
    put(&store,source.clone()).await;
    let lineage = lineage(&source,None,Some("finance"));
    let output = derived("derived-shared",&lineage);
    put(&store,output.clone()).await;
    let proof = resolve_derived_lineage(&store,&scope("bob","finance"),&lineage).await.unwrap();
    assert!(filter("bob","finance").with_resolved_derived_lineage(proof.clone()).allows_global_record(&output));
    assert!(!filter("bob","ops").with_resolved_derived_lineage(proof).allows_global_record(&output));
    assert!(get(&store,"derived-shared",scope("bob","finance")).await.is_some());
    assert!(get(&store,"derived-shared",scope("bob","ops")).await.is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn tenant_shared_source_does_not_need_an_invented_department_floor() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let source = row("tenant-shared",None,None);
    put(&store,source.clone()).await;
    let lineage = lineage(&source,None,None);
    let output = derived("tenant-derived",&lineage);
    put(&store,output.clone()).await;
    let proof = resolve_derived_lineage(&store,&scope("bob","ops"),&lineage).await.unwrap();
    assert!(filter("bob","ops").with_resolved_derived_lineage(proof).allows_global_record(&output));
}

#[tokio::test]
#[serial_test::serial]
async fn deleted_source_is_hidden_by_read_list_search_and_cold_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite3");
    {
        let store = MemoryDatabase::new(&path).await.unwrap();
        let source = row("delete-source",None,Some("finance"));
        put(&store,source.clone()).await;
        let lineage = lineage(&source,None,Some("finance"));
        let output = derived("delete-derived",&lineage);
        put(&store,output.clone()).await;
        let chunk = MemoryChunk {id:"delete-derived-chunk".into(),content:output.content.clone(),tier:MemoryTier::Global,
            session_id:None,project_id:Some("lineage-project".into()),source:"derived".into(),source_path:None,source_mtime:None,
            source_size:None,source_hash:None,tenant_scope:tenant(),subject:None,created_at:chrono::Utc::now(),token_count:4,
            metadata:output.metadata};
        store.write(MemoryStoreWriteRequest::Chunk {scope:MemoryWriteScope {tenant:tenant(),org_unit:Some("finance".into()),subject:None},
            chunk,embedding:vec![1.0;crate::types::DEFAULT_EMBEDDING_DIMENSION]}).await.unwrap();
        assert!(get(&store,"delete-derived",scope("bob","finance")).await.is_some());
        assert_eq!(list_and_search_contain(&store,"delete-derived").await,vec![true,true],
            "prove both query surfaces expose the authorized row before deletion");
        assert!(matches!(store.read(MemoryStoreReadRequest::Chunks {scope:scope("bob","finance"),
            selector:MemoryChunkSelector::global(),limit:None}).await.unwrap(),MemoryStoreReadResult::Chunks(rows) if rows.len()==1));
        store.mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {scope:scope("alice","finance"),id:source.id}).await.unwrap();
        assert!(get(&store,"delete-derived",scope("bob","finance")).await.is_none());
    }
    let reopened = MemoryDatabase::new(&path).await.unwrap();
    assert!(get(&reopened,"delete-derived",scope("bob","finance")).await.is_none());
    assert_eq!(list_and_search_contain(&reopened,"delete-derived").await,vec![false,false],
        "a persisted derivative cannot survive source deletion through another read surface");
    assert!(matches!(reopened.read(MemoryStoreReadRequest::Chunks {scope:scope("bob","finance"),
        selector:MemoryChunkSelector::global(),limit:None}).await.unwrap(),MemoryStoreReadResult::Chunks(rows) if rows.is_empty()));
    assert!(matches!(reopened.query(MemoryStoreQueryRequest::SimilarChunks {scope:scope("bob","finance"),
        selector:MemoryChunkSelector::global(),query_embedding:vec![1.0;crate::types::DEFAULT_EMBEDDING_DIMENSION],limit:10})
        .await.unwrap(),MemoryStoreQueryResult::SimilarChunks(rows) if rows.is_empty()));
}

#[tokio::test]
#[serial_test::serial]
async fn tombstoned_connector_object_invalidates_derivative_without_deleting_memory() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let mut source = row("lifecycle-source",None,Some("finance"));
    let resource = ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::DocumentCollection,"lifecycle-binding");
    source.metadata.as_mut().unwrap()["enterprise_source_binding"] = json!({"binding_id":"lifecycle-binding",
        "source_object_id":"lifecycle-object","resource_ref":resource,"data_class":"financial_record"});
    put(&store,source.clone()).await;
    let lifecycle = crate::types::SourceObjectLifecycleRecord {
        source_object_id:"lifecycle-object".into(),tenant_scope:tenant(),source_binding_id:"lifecycle-binding".into(),
        connector_id:"fixture-connector".into(),state:crate::types::SourceObjectLifecycleState::Active,tier:MemoryTier::Global,
        session_id:None,project_id:Some("lineage-project".into()),import_namespace:"fixture".into(),indexed_path:"fixture.txt".into(),
        native_object_id:"native-object".into(),resource_ref:serde_json::to_value(resource).unwrap(),data_class:"financial_record".into(),
        content_hash:Some(source.content_hash.clone()),source_hash:None,first_seen_at_ms:1_000,last_seen_at_ms:1_000,
        tombstoned_at_ms:None,metadata:None,
    };
    store.write(MemoryStoreWriteRequest::SourceObjectLifecycle {scope:MemoryWriteScope::tenant(tenant()),record:lifecycle}).await.unwrap();
    let lineage = lineage(&source,None,None);
    put(&store,derived("lifecycle-derived",&lineage)).await;
    assert!(get(&store,"lifecycle-derived",scope("bob","ops")).await.is_some());
    store.mutate(MemoryStoreMutationRequest::TombstoneSourceObjectLifecycle {scope:MemoryReadScope::tenant(tenant()),
        source_binding_id:"lifecycle-binding".into(),native_object_id:"native-object".into(),tombstoned_at_ms:3_000}).await.unwrap();
    assert!(get(&store,&source.id,scope("alice","finance")).await.is_some(),"the canonical memory still exists");
    assert!(get(&store,"lifecycle-derived",scope("bob","ops")).await.is_none(),"connector lifecycle is an inherited restriction");
    assert!(resolve_derived_lineage(&store,&scope("bob","ops"),&lineage).await.is_err());
}

#[tokio::test]
#[serial_test::serial]
async fn manager_resolver_seam_preserves_private_chunk_positive_and_denial() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn MemoryStore> = Arc::new(MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap());
    let lineage = DerivedMemoryLineage::new(Some("alice".into()),Some("finance".into()),vec![],
        vec![CanonicalInputReference::SessionMessage {session_id:"native-session".into(),message_id:"native-message".into(),
            body_digest:"a".repeat(64)}]).unwrap();
    let chunk = MemoryChunk {
        id:"native-private-chunk".into(),content:"orchard lighthouse".into(),tier:MemoryTier::Session,
        session_id:Some("native-session".into()),project_id:Some("lineage-project".into()),source:"derived".into(),
        source_path:None,source_mtime:None,source_size:None,source_hash:None,tenant_scope:tenant(),subject:Some("alice".into()),
        created_at:chrono::Utc::now(),token_count:3,
        metadata:metadata_with_derived_lineage(Some(json!({"owner_org_unit_id":"finance","owner_subject":"alice"})),&lineage).unwrap(),
    };
    let embeddings = crate::embeddings::EmbeddingService::deterministic_for_tests(crate::types::DEFAULT_EMBEDDING_DIMENSION);
    let embedding = embeddings.embed(&chunk.content).await.unwrap();
    store.write(MemoryStoreWriteRequest::Chunk {scope:MemoryWriteScope {tenant:tenant(),org_unit:Some("finance".into()),
        subject:Some("alice".into())},chunk:chunk.clone(),embedding}).await.unwrap();
    let make_manager = || crate::MemoryManager::new_with_store(store.clone(),
        crate::embeddings::EmbeddingService::deterministic_for_tests(crate::types::DEFAULT_EMBEDDING_DIMENSION)).unwrap();
    let access = filter("alice","finance");
    let unresolved = make_manager().search_for_tenant_with_access_filter("orchard lighthouse",Some(MemoryTier::Session),
        Some("lineage-project"),Some("native-session"),&tenant(),Some(5),Some(&access)).await.unwrap();
    assert!(unresolved.is_empty(),"memory cannot resolve native sessions without the embedding host");
    let permit = Arc::new(AtomicBool::new(true));
    let invocations = Arc::new(AtomicUsize::new(0));
    let current_permit = permit.clone();
    let calls = invocations.clone();
    let expected = lineage.clone();
    // The callback is the session-repository integration seam. Its canonical
    // validation is host-owned; this test proves ranking invokes that seam and
    // retains a valid private positive instead of discarding every derivative.
    let resolver: DerivedMemoryAccessResolver = Arc::new(move |store,scope,lineage,filter| {
        let permit = current_permit.clone(); let calls = calls.clone(); let expected = expected.clone();
        Box::pin(async move {
            calls.fetch_add(1,Ordering::SeqCst);
            assert_eq!(lineage,expected); assert_eq!(scope.subject.as_deref(),Some("alice"));
            if !permit.load(Ordering::SeqCst) { return None; }
            resolve_derived_lineage(store.as_ref(),&scope,&lineage).await.ok()
                .map(|proof| filter.with_resolved_derived_lineage(proof))
        })
    });
    let manager = make_manager().with_derived_memory_access_resolver(resolver);
    let allowed = manager.search_for_tenant_with_access_filter("orchard lighthouse",Some(MemoryTier::Session),
        Some("lineage-project"),Some("native-session"),&tenant(),Some(5),Some(&access)).await.unwrap();
    assert_eq!(allowed.len(),1); assert_eq!(allowed[0].chunk.id,chunk.id);
    let (context,_) = manager.retrieve_context_with_meta_for_tenant_with_access_filter("orchard lighthouse",
        Some("lineage-project"),Some("native-session"),&tenant(),Some(100),Some(&access)).await.unwrap();
    assert!(context.current_session.iter().any(|row| row.id == chunk.id));
    let before = invocations.load(Ordering::SeqCst);
    permit.store(false,Ordering::SeqCst);
    let (context,_) = manager.retrieve_context_with_meta_for_tenant_with_access_filter("orchard lighthouse",
        Some("lineage-project"),Some("native-session"),&tenant(),Some(100),Some(&access)).await.unwrap();
    assert!(context.current_session.is_empty() && context.relevant_history.is_empty() && context.project_facts.is_empty());
    assert!(invocations.load(Ordering::SeqCst) > before);
    let imported = crate::types::StoreMessageRequest {content:"forged import".into(),tier:MemoryTier::Session,
        session_id:Some("native-session".into()),project_id:Some("lineage-project".into()),source:"file".into(),
        source_path:Some("fixture.txt".into()),source_mtime:None,source_size:None,source_hash:None,
        tenant_scope:tenant(),subject:Some("alice".into()),metadata:chunk.metadata};
    assert!(manager.store_message(imported).await.unwrap_err().to_string().contains("reserved_metadata"));
}

#[tokio::test]
#[serial_test::serial]
async fn source_privacy_tightening_invalidates_the_old_derivative() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let source = row("tighten-source",None,Some("finance"));
    put(&store,source.clone()).await;
    let lineage = lineage(&source,None,Some("finance"));
    put(&store,derived("tighten-derived",&lineage)).await;
    let mut metadata = source.metadata.clone().unwrap();
    metadata["owner_subject"] = json!("alice");
    store.mutate(MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope:scope("alice","finance"),id:source.id.clone(),visibility:source.visibility.clone(),demoted:false,
        metadata:Some(metadata),provenance:source.provenance.clone(),
    }).await.unwrap();
    assert!(get(&store,"tighten-derived",scope("alice","finance")).await.is_none(),
        "even its owner must rederive after source restrictions change");
    assert!(get(&store,"tighten-derived",scope("bob","finance")).await.is_none());
    assert!(resolve_derived_lineage(&store,&scope("alice","finance"),&lineage).await.is_err());
}

#[tokio::test]
#[serial_test::serial]
async fn identical_shared_private_and_distinct_lineage_writes_do_not_dedupe() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let source_a = row("dedupe-source-a",None,Some("finance"));
    let source_b = row("dedupe-source-b",None,Some("finance"));
    put(&store,source_a.clone()).await; put(&store,source_b.clone()).await;
    let shared_lineage = lineage(&source_a,None,Some("finance"));
    let private_lineage = lineage(&source_a,Some("alice"),Some("finance"));
    let other_lineage = lineage(&source_b,Some("alice"),Some("finance"));
    let shared = derived("dedupe-shared",&shared_lineage);
    let mut private = derived("dedupe-private",&private_lineage);
    let mut other = derived("dedupe-other",&other_lineage);
    private.content = shared.content.clone(); private.content_hash = shared.content_hash.clone();
    other.content = shared.content.clone(); other.content_hash = shared.content_hash.clone();
    assert!(put(&store,shared.clone()).await.stored);
    assert!(put(&store,private.clone()).await.stored,"private write must not reuse a shared ID");
    assert!(put(&store,other.clone()).await.stored,"different source restrictions need a distinct ID");
    let mut repeat = private.clone(); repeat.id = "dedupe-repeat".into();
    let result = put(&store,repeat).await;
    assert!(result.deduped); assert_eq!(result.id,private.id);
    let unchanged = get(&store,&shared.id,scope("bob","finance")).await.unwrap();
    assert_eq!(unchanged.metadata,shared.metadata);
    assert_eq!(unchanged.content,shared.content);
    assert!(get(&store,&private.id,scope("bob","finance")).await.is_none());
}

#[test]
fn unresolved_or_malformed_lineage_cannot_disclose_records_or_copied_chunks() {
    let source = row("unresolved-source",None,Some("finance"));
    let lineage = lineage(&source,None,Some("finance"));
    let output = derived("unresolved-derived",&lineage);
    let filter = filter("bob","finance");
    assert_eq!(filter.decision_for_global_record(&output).reason.as_deref(),Some("derived_lineage_unresolved"));
    let chunk = MemoryChunk {
        id:"copied".into(),content:output.content.clone(),tier:MemoryTier::Global,session_id:None,
        project_id:Some("lineage-project".into()),source:"derived".into(),source_path:None,source_mtime:None,
        source_size:None,source_hash:None,tenant_scope:tenant(),subject:None,created_at:chrono::Utc::now(),token_count:5,
        metadata:output.metadata.clone(),
    };
    assert!(!filter.allows_chunk(&chunk));
    let mut malformed = output.clone(); malformed.metadata.as_mut().unwrap()[DERIVED_MEMORY_LINEAGE_METADATA_KEY] = json!({"schema_version":99});
    assert!(!filter.allows_global_record(&malformed));
    assert!(reject_reserved_derived_metadata(output.metadata.as_ref()).is_err());
    assert_eq!(filter.mode,GovernedReadMode::GovernedStrict);
}

#[test]
fn native_inputs_and_private_sources_cannot_be_relabelled_shared() {
    let source = row("private-source",Some("alice"),Some("finance"));
    let restriction = CanonicalMemoryRestriction::from_global_record(&source,&tenant()).unwrap();
    let input = CanonicalInputReference::Memory {source:restriction.source_reference()};
    assert!(DerivedMemoryLineage::new(None,Some("finance".into()),vec![restriction],vec![input]).is_err());
    assert!(DerivedMemoryLineage::new(None,Some("finance".into()),vec![],vec![CanonicalInputReference::SessionMessage {
        session_id:"session".into(),message_id:"message".into(),body_digest:"a".repeat(64),
    }]).is_err());
}

#[test]
fn nested_class_union_retains_specific_sources_and_native_internal_floor() {
    let mut financial = row("class-financial",None,Some("finance"));
    financial.metadata.as_mut().unwrap()["classification"] = json!("financial_record");
    let mut code = row("class-code",None,Some("finance"));
    code.metadata.as_mut().unwrap()["classification"] = json!("source_code");
    let sources = [financial,code].iter().map(|row| CanonicalMemoryRestriction::from_global_record(row,&tenant()).unwrap())
        .collect::<Vec<_>>();
    let inputs = sources.iter().map(|source| CanonicalInputReference::Memory {source:source.source_reference()}).collect();
    let first = DerivedMemoryLineage::new(None,Some("finance".into()),sources,inputs).unwrap();
    let intermediate = derived("class-intermediate",&first);
    let nested = lineage(&intermediate,None,Some("finance"));
    assert_eq!(nested.source_data_classes().unwrap(),vec![DataClass::Internal,DataClass::SourceCode,DataClass::FinancialRecord]);
    assert_eq!(nested.output_data_class(),DataClass::FinancialRecord,
        "the intermediate Internal label cannot replace its original FinancialRecord contribution");
    let native = DerivedMemoryLineage::new(Some("alice".into()),None,vec![],vec![CanonicalInputReference::SessionMessage {
        session_id:"native-session".into(),message_id:"native-message".into(),body_digest:"a".repeat(64),
    }]).unwrap();
    assert_eq!(native.source_data_classes().unwrap(),vec![DataClass::Internal]);
    assert_eq!(native.output_data_class(),DataClass::Internal);
}

#[test]
fn output_policy_cannot_erase_inherited_write_promotion_or_retention_limits() {
    let mut source = row("scope-source",None,Some("finance"));
    let policy = KnowledgeScopePolicy {
        registry_id:"scope-registry".into(),
        resource_ref:ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::SourceBinding,"scope-binding")
            .with_project_id("lineage-project"),
        data_class:DataClass::FinancialRecord,collection_id:None,source_binding_id:Some("scope-binding".into()),
        source_object_id:None,owner_org_unit_id:None,risk_tier:None,allowed_workflow_phases:vec![],
        allowed_write_tiers:vec![GovernedMemoryTier::Session],allowed_promotion_tiers:vec![GovernedMemoryTier::Project],
        retention_expires_at_ms:Some(5_000),required_trust_label:None,promotion_requires_approval:true,
    };
    source.metadata = crate::metadata_with_knowledge_scope(source.metadata,&policy);
    let inherited = lineage(&source,None,Some("finance"));
    // A second-generation row must retain the original policy, even when its
    // own registry allows a wider destination.
    let intermediate = derived("scope-intermediate",&inherited);
    let nested = lineage(&intermediate,None,Some("finance"));
    let mut output_policy = policy.clone();
    output_policy.allowed_write_tiers = vec![GovernedMemoryTier::Session,GovernedMemoryTier::Project];
    output_policy.allowed_promotion_tiers = vec![GovernedMemoryTier::Project,GovernedMemoryTier::Team];
    output_policy.retention_expires_at_ms = None;
    output_policy.promotion_requires_approval = false;
    let metadata = crate::metadata_with_knowledge_scope(
        metadata_with_derived_lineage(None,&nested).unwrap(),&output_policy).unwrap();
    let mut partition = MemoryPartition {org_id:"lineage-org".into(),workspace_id:"lineage-workspace".into(),
        project_id:"lineage-project".into(),tier:GovernedMemoryTier::Session};
    assert!(crate::memory_write_scope_decision(&partition,Some(&metadata),2_000).unwrap().allowed);
    partition.tier = GovernedMemoryTier::Project;
    let denied = crate::memory_write_scope_decision(&partition,Some(&metadata),2_000).unwrap();
    assert!(!denied.allowed);
    assert_eq!(denied.reason_code,"knowledge_write_tier_denied_by_scope");
    let unreviewed = PromotionReview {required:false,reviewer_id:None,approval_id:None};
    let reviewed = PromotionReview {required:true,reviewer_id:Some("reviewer".into()),approval_id:Some("approval".into())};
    let denied = crate::memory_promotion_scope_decision(&partition,GovernedMemoryTier::Project,&unreviewed,Some(&metadata),2_000).unwrap();
    assert!(!denied.allowed);
    assert_eq!(denied.reason_code,"knowledge_promotion_approval_required");
    assert!(crate::memory_promotion_scope_decision(&partition,GovernedMemoryTier::Project,&reviewed,Some(&metadata),2_000).unwrap().allowed);
    assert!(!crate::memory_promotion_scope_decision(&partition,GovernedMemoryTier::Team,&reviewed,Some(&metadata),2_000).unwrap().allowed);
    assert!(!crate::memory_promotion_scope_decision(&partition,GovernedMemoryTier::Project,&reviewed,Some(&metadata),5_000).unwrap().allowed);
    partition.tier = GovernedMemoryTier::Session;
    assert!(!crate::memory_write_scope_decision(&partition,Some(&metadata),5_000).unwrap().allowed);
    partition.project_id = "foreign-project".into();
    assert!(!crate::memory_write_scope_decision(&partition,Some(&metadata),2_000).unwrap().allowed);
    let malformed = json!({DERIVED_MEMORY_LINEAGE_METADATA_KEY:{"schema_version":99}});
    assert!(crate::memory_write_scope_decision(&partition,Some(&malformed),2_000).is_err());
}

#[tokio::test]
#[serial_test::serial]
async fn current_source_grant_is_required_after_proof_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let mut source = row("grant-source",None,Some("finance"));
    let resource = ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::DocumentCollection,"grant-binding");
    source.metadata.as_mut().unwrap()["enterprise_source_binding"] = json!({
        "binding_id":"grant-binding","resource_ref":resource,"data_class":"financial_record"
    });
    put(&store,source.clone()).await;
    let lineage = lineage(&source,None,None);
    assert_eq!(lineage.output_data_class(),DataClass::FinancialRecord);
    let mut output = derived("grant-derived",&lineage);
    output.metadata.as_mut().unwrap()["classification"] = serde_json::to_value(lineage.output_data_class()).unwrap();
    put(&store,output.clone()).await;
    let proof = resolve_derived_lineage(&store,&scope("bob","ops"),&lineage).await.unwrap();
    let grant = ScopedGrant::new("current-grant",PrincipalRef::human_user("bob"),resource,GrantSource::Direct)
        .with_permissions(vec![AccessPermission::Read]).with_data_classes(vec![DataClass::FinancialRecord]);
    let current = MemoryAccessFilter::strict(strict("bob").with_grants(vec![grant])
        .with_data_boundary(DataBoundary::allow(vec![DataClass::FinancialRecord])),2_000)
        .with_caller_subject("bob").with_caller_org_units(["ops".to_string()]).with_resolved_derived_lineage(proof.clone());
    assert!(current.allows_global_record(&output),
        "a current source grant may authorize Bob outside the source's metadata department");
    let fetched = get(&store,&output.id,scope("bob","ops")).await.expect("actual stored FinancialRecord derivative");
    assert_eq!(data_class_from_metadata(fetched.metadata.as_ref()),Some(DataClass::FinancialRecord));
    assert!(current.allows_global_record(&fetched),
        "an actual FinancialRecord grant/read boundary must not need an invented Restricted class");
    let mut relabelled = fetched.clone(); relabelled.metadata.as_mut().unwrap()["classification"] = json!("restricted");
    assert!(!current.allows_global_record(&relabelled));
    assert!(!filter("bob","ops").with_resolved_derived_lineage(proof).allows_global_record(&output),
        "canonical existence proof cannot replace a revoked grant");
}

#[tokio::test]
#[serial_test::serial]
async fn knowledge_registry_grant_without_connector_binding_remains_a_shared_positive() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap();
    let mut source = row("registry-source",None,Some("finance"));
    let resource = ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::KnowledgeSpace,"approved-space")
        .with_project_id("lineage-project");
    let policy = KnowledgeScopePolicy {registry_id:"approved-registry".into(),resource_ref:resource.clone(),data_class:DataClass::Confidential,
        collection_id:None,source_binding_id:None,source_object_id:None,owner_org_unit_id:None,risk_tier:None,
        allowed_workflow_phases:vec![],allowed_write_tiers:vec![GovernedMemoryTier::Session],allowed_promotion_tiers:vec![],
        retention_expires_at_ms:None,required_trust_label:None,promotion_requires_approval:false};
    source.metadata = crate::metadata_with_knowledge_scope(source.metadata,&policy);
    put(&store,source.clone()).await;
    let lineage = lineage(&source,None,None);
    let mut output = derived("registry-derived",&lineage);
    output.metadata.as_mut().unwrap()["classification"] = json!("confidential");
    put(&store,output.clone()).await;
    let proof = resolve_derived_lineage(&store,&scope("bob","ops"),&lineage).await.unwrap();
    let grant = ScopedGrant::new("registry-grant",PrincipalRef::human_user("bob"),resource,GrantSource::Direct)
        .with_permissions(vec![AccessPermission::Read]).with_data_classes(vec![DataClass::Confidential]);
    let access = MemoryAccessFilter::strict(strict("bob").with_grants(vec![grant])
        .with_data_boundary(DataBoundary::allow(vec![DataClass::Confidential])),2_000)
        .with_caller_subject("bob").with_caller_org_units(["ops".to_string()]).with_resolved_derived_lineage(proof);
    let fetched = get(&store,&output.id,scope("bob","ops")).await.unwrap();
    assert!(access.allows_global_record(&fetched),"a valid knowledge-space grant does not require an invented connector binding");
}
