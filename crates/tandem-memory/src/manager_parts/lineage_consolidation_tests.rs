use super::*;
use async_trait::async_trait;
use sha2::{Digest,Sha256};
use std::sync::atomic::{AtomicUsize,Ordering};
use tandem_data_boundary::{DataBoundaryTenantRef,ProviderEgressAuthority,SensitiveDataClass};
use tandem_enterprise_contract::{AssertionMetadata,AuthorityChain,DataBoundary,DataClass,PrincipalRef,
    RequestPrincipal,ResourceKind,ResourceRef,ResourceScope,StrictTenantContext,TenantContext};
use tandem_providers::{AppConfig,Provider};
use tandem_types::ProviderInfo;
use crate::provider_egress::test_environment::{env_lock,EnvRestore};
use crate::{CanonicalInputReference,CanonicalMemoryRestriction,DerivedMemoryLineage};
use crate::types::GlobalMemoryRecord;

struct SummaryProvider {
    calls:Arc<AtomicUsize>,
    delete_source:Option<(Arc<dyn MemoryStore>,String)>,
}

#[async_trait]
impl Provider for SummaryProvider {
    fn info(&self)->ProviderInfo { ProviderInfo {id:"capture".into(),name:"Synthetic summary".into(),models:Vec::new()} }
    async fn complete(&self,_prompt:&str,_model_override:Option<&str>)->anyhow::Result<String> {
        self.calls.fetch_add(1,Ordering::SeqCst);
        if let Some((store,id)) = &self.delete_source {
            store.mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {scope:MemoryReadScope::tenant(tenant()),id:id.clone()}).await?;
        }
        Ok("orchard lighthouse summary".into())
    }
}

fn tenant()->MemoryTenantScope { MemoryTenantScope {org_id:"lineage-org".into(),workspace_id:"lineage-workspace".into(),deployment_id:None} }

fn read_scope()->MemoryReadScope { MemoryReadScope {tenant:tenant(),org_unit:Some("finance".into()),subject:Some("alice".into()),
    access:crate::MemoryReadAccess::Scoped} }

fn filter()->crate::types::MemoryAccessFilter {
    let now = Utc::now().timestamp_millis().max(0) as u64;
    let strict = StrictTenantContext::new(TenantContext::explicit_user_workspace("lineage-org","lineage-workspace",None,"alice"),
        PrincipalRef::human_user("alice"),AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice","consolidation-test")),
        ResourceScope::root(ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::Workspace,"lineage-workspace")),
        AssertionMetadata::new("test","runtime",now,now+60_000,"consolidation-test"))
        .with_data_boundary(DataBoundary::unrestricted());
    crate::types::MemoryAccessFilter::strict(strict,now).with_caller_subject("alice").with_caller_org_units(["finance".to_string()])
}

fn request()->ScopedMemoryConsolidationRequest { ScopedMemoryConsolidationRequest {tenant_scope:tenant(),org_unit:Some("finance".into()),
    subject:Some("alice".into()),project_id:"lineage-project".into(),session_id:"lineage-session".into()} }

fn config()->MemoryConsolidationConfig { MemoryConsolidationConfig {enabled:true,..Default::default()} }

fn egress(classes:Arc<std::sync::Mutex<Vec<SensitiveDataClass>>>)->MemoryProviderEgressContext {
    MemoryProviderEgressContext::new(ProviderEgressAuthority::new(DataBoundaryTenantRef {organization_id:Some("lineage-org".into()),
        workspace_id:Some("lineage-workspace".into()),deployment_id:None}).with_run_id("lineage-run").with_session_id("lineage-session"))
        .with_audit_sink(Arc::new(move |event| {
            *classes.lock().unwrap() = event.semantic_data_classes;
            Box::pin(async {Ok(())})
        }))
}

fn source(id:&str,class:DataClass)->GlobalMemoryRecord {
    let content = format!("orchard lighthouse {id}");
    GlobalMemoryRecord {id:id.into(),user_id:"alice".into(),source_type:"fact".into(),content_hash:format!("{:x}",Sha256::digest(content.as_bytes())),
        content,run_id:"lineage-run".into(),session_id:None,message_id:None,tool_name:None,project_tag:Some("lineage-project".into()),
        channel_tag:None,host_tag:None,metadata:Some(serde_json::json!({"owner_org_unit_id":"finance","classification":class})),
        provenance:Some(serde_json::json!({"tenant_context":tenant()})),redaction_status:"passed".into(),redaction_count:0,
        visibility:"private".into(),demoted:false,score_boost:0.0,created_at_ms:1_000,updated_at_ms:1_000,expires_at_ms:None}
}

async fn put_source(store:&dyn MemoryStore,record:GlobalMemoryRecord) {
    store.write(MemoryStoreWriteRequest::GlobalRecord {scope:MemoryWriteScope {tenant:tenant(),org_unit:Some("finance".into()),subject:None},
        record}).await.unwrap();
}

async fn seed(store:&dyn MemoryStore,records:&[GlobalMemoryRecord],native:bool)->DerivedMemoryLineage {
    let mut sources = Vec::new();
    for record in records {put_source(store,record.clone()).await; sources.push(CanonicalMemoryRestriction::from_global_record(record,&tenant()).unwrap());}
    let mut inputs = sources.iter().map(|source| CanonicalInputReference::Memory {source:source.source_reference()}).collect::<Vec<_>>();
    if native { inputs.push(CanonicalInputReference::SessionMessage {session_id:"lineage-session".into(),message_id:"native-message".into(),
        body_digest:"a".repeat(64)}); }
    let lineage = DerivedMemoryLineage::new(Some("alice".into()),Some("finance".into()),sources,inputs).unwrap();
    let chunk = MemoryChunk {id:"contributing-chunk".into(),content:"orchard lighthouse contributor".into(),tier:MemoryTier::Session,
        session_id:Some("lineage-session".into()),project_id:Some("lineage-project".into()),source:"derived".into(),source_path:None,
        source_mtime:None,source_size:None,source_hash:None,tenant_scope:tenant(),subject:Some("alice".into()),created_at:Utc::now(),token_count:4,
        metadata:crate::metadata_with_derived_lineage(Some(serde_json::json!({"owner_subject":"alice","owner_org_unit_id":"finance"})),&lineage).unwrap()};
    store.write(MemoryStoreWriteRequest::Chunk {scope:MemoryWriteScope {tenant:tenant(),org_unit:Some("finance".into()),subject:Some("alice".into())},
        chunk,embedding:vec![1.0;crate::types::DEFAULT_EMBEDDING_DIMENSION]}).await.unwrap();
    lineage
}

fn manager(store:Arc<dyn MemoryStore>)->MemoryManager {
    MemoryManager::new_with_store(store,EmbeddingService::deterministic_for_tests(crate::types::DEFAULT_EMBEDDING_DIMENSION)).unwrap()
}

async fn registry(calls:Arc<AtomicUsize>,delete_source:Option<(Arc<dyn MemoryStore>,String)>)->ProviderRegistry {
    let providers = ProviderRegistry::new(AppConfig::default());
    providers.replace_for_test(vec![Arc::new(SummaryProvider {calls,delete_source})],Some("capture".into())).await;
    providers
}

async fn chunks(store:&dyn MemoryStore,selector:MemoryChunkSelector)->Vec<MemoryChunk> {
    match store.read(MemoryStoreReadRequest::Chunks {scope:read_scope(),selector,limit:None}).await.unwrap() {
        MemoryStoreReadResult::Chunks(rows)=>rows,other=>panic!("{other:?}"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_preserves_lineage_classes_and_hides_summary_after_source_deletion() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[("TANDEM_DATA_BOUNDARY_MODE","enforce"),("TANDEM_DATA_BOUNDARY_STRICT","1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES","capture=local")]);
    let dir = tempfile::tempdir().unwrap();
    let store:Arc<dyn MemoryStore> = Arc::new(MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap());
    let financial = source("financial-source",DataClass::FinancialRecord);
    let code = source("code-source",DataClass::SourceCode);
    let lineage = seed(store.as_ref(),&[financial.clone(),code],false).await;
    let manager = manager(store.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let providers = registry(calls.clone(),None).await;
    let classes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let egress = egress(classes.clone());
    assert!(manager.consolidate_scoped_session(&request(),&providers,&config(),&egress).await.is_err(),
        "the ordinary wrapper cannot invent derivative authority");
    assert_eq!(calls.load(Ordering::SeqCst),0);
    let result = manager.consolidate_scoped_session_with_access_filter(&request(),&providers,&config(),&egress,Some(&filter())).await.unwrap();
    assert_eq!(result.as_deref(),Some("orchard lighthouse summary"));
    assert_eq!(calls.load(Ordering::SeqCst),1);
    let summaries = chunks(store.as_ref(),MemoryChunkSelector::project("lineage-project")).await;
    assert_eq!(summaries.len(),1);
    let retained = DerivedMemoryLineage::from_metadata(summaries[0].metadata.as_ref()).unwrap().unwrap();
    assert_eq!(retained,lineage);
    assert_eq!(crate::types::data_class_from_metadata(summaries[0].metadata.as_ref()),Some(DataClass::FinancialRecord));
    let observed = classes.lock().unwrap();
    assert!(observed.contains(&SensitiveDataClass::Financial)); assert!(observed.contains(&SensitiveDataClass::SourceCode));
    drop(observed);
    assert!(chunks(store.as_ref(),MemoryChunkSelector::session("lineage-session")).await.is_empty());
    store.mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {scope:read_scope(),id:financial.id}).await.unwrap();
    assert!(chunks(store.as_ref(),MemoryChunkSelector::project("lineage-project")).await.is_empty(),
        "the durable summary inherits canonical source deletion");
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_denied_grant_or_missing_native_resolver_never_dispatches_provider() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[("TANDEM_DATA_BOUNDARY_MODE","enforce"),("TANDEM_DATA_BOUNDARY_STRICT","1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES","capture=local")]);
    for native in [false,true] {
        let dir = tempfile::tempdir().unwrap();
        let store:Arc<dyn MemoryStore> = Arc::new(MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap());
        let mut record = source("restricted-source",DataClass::FinancialRecord);
        if !native { record.metadata.as_mut().unwrap()["enterprise_source_binding"] = serde_json::json!({"binding_id":"restricted-binding",
            "resource_ref":ResourceRef::new("lineage-org","lineage-workspace",ResourceKind::DocumentCollection,"restricted-binding"),
            "data_class":"financial_record"}); }
        seed(store.as_ref(),&[record],native).await;
        let manager = manager(store.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = registry(calls.clone(),None).await;
        let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
        let error = manager.consolidate_scoped_session_with_access_filter(&request(),&providers,&config(),&egress,Some(&filter())).await.unwrap_err();
        assert!(error.to_string().contains(if native {"authority unavailable"} else {"lineage denied"}),"{error}");
        assert_eq!(calls.load(Ordering::SeqCst),0);
        assert_eq!(chunks(store.as_ref(),MemoryChunkSelector::session("lineage-session")).await.len(),1);
        assert!(chunks(store.as_ref(),MemoryChunkSelector::project("lineage-project")).await.is_empty());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn consolidation_rechecks_source_after_provider_before_summary_write() {
    let _lock = env_lock();
    let _env = EnvRestore::set(&[("TANDEM_DATA_BOUNDARY_MODE","enforce"),("TANDEM_DATA_BOUNDARY_STRICT","1"),
        ("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES","capture=local")]);
    let dir = tempfile::tempdir().unwrap();
    let store:Arc<dyn MemoryStore> = Arc::new(MemoryDatabase::new(&dir.path().join("memory.sqlite3")).await.unwrap());
    let record = source("provider-revoked-source",DataClass::FinancialRecord);
    seed(store.as_ref(),&[record.clone()],false).await;
    let manager = manager(store.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let providers = registry(calls.clone(),Some((store.clone(),record.id.clone()))).await;
    let egress = egress(Arc::new(std::sync::Mutex::new(Vec::new())));
    let error = manager.consolidate_scoped_session_with_access_filter(&request(),&providers,&config(),&egress,Some(&filter())).await.unwrap_err();
    assert!(error.to_string().contains("authority unavailable"),"{error}");
    assert_eq!(calls.load(Ordering::SeqCst),1,"the actual provider reached the source-deletion boundary");
    assert!(chunks(store.as_ref(),MemoryChunkSelector::project("lineage-project")).await.is_empty());
    put_source(store.as_ref(),record).await;
    assert_eq!(chunks(store.as_ref(),MemoryChunkSelector::session("lineage-session")).await.len(),1,
        "restoring canonical source proves the contributor was not removed by a failed consolidation");
}
