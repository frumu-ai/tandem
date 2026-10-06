use super::*;
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

fn guarded_record(tenant: &MemoryTenantScope, id: &str) -> GlobalMemoryRecord {
    let content = format!("orchard lighthouse {id}");
    GlobalMemoryRecord {
        id: id.into(),
        user_id: "alice".into(),
        source_type: "fact".into(),
        content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
        content,
        run_id: "guarded-run".into(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("guarded-project".into()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(
            serde_json::json!({"owner_org_unit_id":"finance","classification":"internal"}),
        ),
        provenance: Some(serde_json::json!({"tenant_context":tenant})),
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

fn write_scope(tenant: &MemoryTenantScope) -> MemoryWriteScope {
    MemoryWriteScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".into()),
        subject: None,
    }
}
fn read_scope(tenant: &MemoryTenantScope) -> MemoryReadScope {
    MemoryReadScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".into()),
        subject: Some("alice".into()),
        access: MemoryReadAccess::Scoped,
    }
}

async fn read(
    store: &PostgresMemoryStore,
    tenant: &MemoryTenantScope,
    id: &str,
) -> Option<GlobalMemoryRecord> {
    match store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: read_scope(tenant),
            id: id.into(),
        })
        .await
        .unwrap()
    {
        MemoryStoreReadResult::GlobalRecord(record) => record,
        other => panic!("{other:?}"),
    }
}

fn authority(
    allowed: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    calls: Arc<AtomicUsize>,
) -> MemoryCommitAuthority {
    Arc::new(move || {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            entered.notify_one();
        }
        if allowed.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "revoked guarded authority",
            ))
        }
    })
}

async fn witness_real_lock_wait(store: &PostgresMemoryStore, blocker: i32, query_pattern: &str) {
    let observer = store.client().await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let count: i64 = observer
                .query_one(
                    "SELECT COUNT(*) FROM pg_stat_activity WHERE datname=current_database()
                   AND pg_blocking_pids(pid) @> ARRAY[$1]::integer[] AND query LIKE $2",
                    &[&blocker, &query_pattern],
                )
                .await
                .unwrap()
                .get(0);
            if count > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the actual guarded SQL must wait on the independent transaction");
}

#[tokio::test]
async fn postgres_guarded_unique_writer_wait_revocation_rolls_back_and_current_authority_commits() {
    let Some(url) = test_url() else {
        return;
    };
    let store = Arc::new(PostgresMemoryStore::connect(config(url, 4)).await.unwrap());
    let tenant = tenant(&format!("guarded-unique-{}", uuid::Uuid::new_v4()));
    let record = guarded_record(&tenant, &format!("guarded-insert-{}", uuid::Uuid::new_v4()));
    let mut locker = store.client().await.unwrap();
    let blocker: i32 = locker
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    let tx = locker.transaction().await.unwrap();
    let payload = serde_json::to_value(&record).unwrap();
    let held_id = format!("independent-{}", uuid::Uuid::new_v4());
    // A different physical row owns the same real semantic unique key, so the
    // guarded INSERT must wait on this uncommitted transaction.
    tx.execute("INSERT INTO tandem_memory_global_records
        (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,owner_org_unit_id,owner_subject,private,data_class,source_binding_id,
         user_id,source_type,content_hash,run_id,session_id,message_id,tool_name,project_tag,channel_tag,demoted,expires_at_ms,
         created_at_ms,search_content,data,tenant_shared,derived_lineage_digest)
        VALUES ($1,$2,$3,$4,'finance',NULL,false,'internal',NULL,$5,$6,$7,$8,NULL,NULL,NULL,$9,NULL,false,NULL,1000,$10,$11,false,'')",
        &[&held_id,&tenant.org_id,&tenant.workspace_id,&tenant.deployment_id.as_deref().unwrap_or(""),&record.user_id,
          &record.source_type,&record.content_hash,&record.run_id,&record.project_tag,&record.content,&payload]).await.unwrap();
    let allowed = Arc::new(AtomicBool::new(true));
    let entered = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let callback = authority(allowed.clone(), entered.clone(), calls.clone());
    let worker_store = store.clone();
    let worker_record = record.clone();
    let scope = write_scope(&tenant);
    let worker = tokio::spawn(async move {
        worker_store
            .write_with_commit_authority(
                MemoryStoreWriteRequest::GlobalRecord {
                    scope,
                    record: worker_record,
                },
                callback,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), entered.notified())
        .await
        .unwrap();
    witness_real_lock_wait(
        &store,
        blocker,
        "%INSERT INTO tandem_memory_global_records%",
    )
    .await;
    allowed.store(false, Ordering::SeqCst);
    tx.rollback().await.unwrap();
    let error = worker.await.unwrap().unwrap_err();
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
    assert!(calls.load(Ordering::SeqCst) >= 2);
    assert!(
        read(&store, &tenant, &record.id).await.is_none(),
        "the actual waited INSERT must roll back after revocation"
    );
    let result = store
        .write_with_commit_authority(
            MemoryStoreWriteRequest::GlobalRecord {
                scope: write_scope(&tenant),
                record: record.clone(),
            },
            Arc::new(|| Ok(())),
        )
        .await
        .unwrap();
    assert!(matches!(result,MemoryStoreWriteResult::GlobalRecord(result) if result.stored));
    assert_eq!(
        read(&store, &tenant, &record.id)
            .await
            .unwrap()
            .content_hash,
        record.content_hash
    );
}

#[tokio::test]
async fn postgres_guarded_row_writer_wait_and_precommit_denial_preserve_original_context() {
    let Some(url) = test_url() else {
        return;
    };
    let store = Arc::new(PostgresMemoryStore::connect(config(url, 4)).await.unwrap());
    let tenant = tenant(&format!("guarded-row-{}", uuid::Uuid::new_v4()));
    let record = guarded_record(
        &tenant,
        &format!("guarded-context-{}", uuid::Uuid::new_v4()),
    );
    store
        .write(MemoryStoreWriteRequest::GlobalRecord {
            scope: write_scope(&tenant),
            record: record.clone(),
        })
        .await
        .unwrap();
    let mut metadata = record.metadata.clone().unwrap();
    metadata["guarded"] = serde_json::json!(true);
    let mutation = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope: read_scope(&tenant),
        id: record.id.clone(),
        visibility: record.visibility.clone(),
        demoted: false,
        metadata: Some(metadata.clone()),
        provenance: record.provenance.clone(),
    };
    let mut locker = store.client().await.unwrap();
    let blocker: i32 = locker
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    let tx = locker.transaction().await.unwrap();
    tx.query_one(
        "SELECT id FROM tandem_memory_global_records WHERE id=$1 FOR UPDATE",
        &[&record.id],
    )
    .await
    .unwrap();
    let allowed = Arc::new(AtomicBool::new(true));
    let entered = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let callback = authority(allowed.clone(), entered.clone(), calls.clone());
    let worker_store = store.clone();
    let worker_mutation = mutation.clone();
    let worker = tokio::spawn(async move {
        worker_store
            .mutate_with_commit_authority(worker_mutation, callback)
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), entered.notified())
        .await
        .unwrap();
    witness_real_lock_wait(&store, blocker, "%FOR UPDATE%").await;
    allowed.store(false, Ordering::SeqCst);
    tx.rollback().await.unwrap();
    assert_eq!(
        worker.await.unwrap().unwrap_err().kind,
        MemoryStoreErrorKind::ScopeViolation
    );
    let original = read(&store, &tenant, &record.id).await.unwrap();
    assert_eq!(original.metadata, record.metadata);
    let count = Arc::new(AtomicUsize::new(0));
    let callback_count = count.clone();
    let callback: MemoryCommitAuthority = Arc::new(move || {
        if callback_count.fetch_add(1, Ordering::SeqCst) == 2 {
            Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "precommit revoked authority",
            ))
        } else {
            Ok(())
        }
    });
    assert!(store
        .mutate_with_commit_authority(mutation.clone(), callback)
        .await
        .is_err());
    assert_eq!(count.load(Ordering::SeqCst), 3);
    let retained = read(&store, &tenant, &record.id).await.unwrap();
    assert_eq!(retained.metadata, original.metadata);
    assert_eq!(retained.updated_at_ms, original.updated_at_ms);
    assert_eq!(retained.content_hash, original.content_hash);
    assert!(matches!(
        store
            .mutate_with_commit_authority(mutation, Arc::new(|| Ok(())))
            .await
            .unwrap(),
        MemoryStoreMutationResult::Changed(true)
    ));
    assert_eq!(
        read(&store, &tenant, &record.id).await.unwrap().metadata,
        Some(metadata)
    );
}

#[tokio::test]
async fn postgres_guarded_cas_row_wait_rejects_stale_policy_and_current_target_commits() {
    let Some(url) = test_url() else {return;};
    let store = Arc::new(PostgresMemoryStore::connect(config(url,4)).await.unwrap());
    let tenant = tenant(&format!("guarded-cas-{}",uuid::Uuid::new_v4()));
    let original = guarded_record(&tenant,&format!("cas-target-{}",uuid::Uuid::new_v4()));
    store.write(MemoryStoreWriteRequest::GlobalRecord {scope:write_scope(&tenant),record:original.clone()}).await.unwrap();
    let expected = crate::CanonicalMemoryRestriction::from_global_record(&original,&tenant).unwrap().source_reference();
    let stale = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope:read_scope(&tenant),id:original.id.clone(),visibility:original.visibility.clone(),demoted:false,
        metadata:original.metadata.clone(),provenance:original.provenance.clone(),
    };
    let mut locker = store.client().await.unwrap();
    let blocker:i32 = locker.query_one("SELECT pg_backend_pid()",&[]).await.unwrap().get(0);
    let tx = locker.transaction().await.unwrap();
    tx.query_one("SELECT id FROM tandem_memory_global_records WHERE id=$1 FOR UPDATE",&[&original.id]).await.unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let callback = authority(Arc::new(AtomicBool::new(true)),entered.clone(),calls.clone());
    let worker_store = store.clone();
    let worker = tokio::spawn(async move {worker_store.mutate_with_commit_authority_if_unchanged(stale,expected,callback).await});
    tokio::time::timeout(Duration::from_secs(10),entered.notified()).await.unwrap();
    witness_real_lock_wait(&store,blocker,"%FOR UPDATE%").await;
    let mut tightened = original.clone();
    tightened.metadata.as_mut().unwrap()["classification"] = serde_json::json!("restricted");
    tightened.updated_at_ms = 2_000;
    let data = serde_json::to_value(&tightened).unwrap();
    tx.execute("UPDATE tandem_memory_global_records SET data=$2,data_class='restricted' WHERE id=$1",&[&original.id,&data]).await.unwrap();
    tx.commit().await.unwrap();
    let error = worker.await.unwrap().unwrap_err();
    assert_eq!(error.kind,MemoryStoreErrorKind::ScopeViolation);
    assert!(error.message.contains("target changed"),"{error:?}");
    assert_eq!(calls.load(Ordering::SeqCst),2,"current authority does not substitute for target freshness");
    let retained = read(&store,&tenant,&original.id).await.unwrap();
    assert_eq!(retained.metadata,tightened.metadata);
    assert_eq!(retained.updated_at_ms,tightened.updated_at_ms);
    assert_eq!(retained.content_hash,original.content_hash);
    let expected = crate::CanonicalMemoryRestriction::from_global_record(&retained,&tenant).unwrap().source_reference();
    let mut metadata = retained.metadata.clone().unwrap(); metadata["cas_healthy"] = serde_json::json!(true);
    let current = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope:read_scope(&tenant),id:retained.id.clone(),visibility:retained.visibility.clone(),demoted:false,
        metadata:Some(metadata.clone()),provenance:retained.provenance.clone(),
    };
    let mut wrong_id = expected.clone(); wrong_id.memory_id = "other-target".into();
    assert_eq!(store.mutate_with_commit_authority_if_unchanged(current.clone(),wrong_id,Arc::new(||Ok(())))
        .await.unwrap_err().kind,MemoryStoreErrorKind::ScopeViolation);
    let mut foreign = current.clone();
    if let MemoryStoreMutationRequest::UpdateGlobalRecordContext {scope,..} = &mut foreign {scope.tenant.workspace_id="foreign-workspace".into();}
    assert_eq!(store.mutate_with_commit_authority_if_unchanged(foreign,expected.clone(),Arc::new(||Ok(())))
        .await.unwrap_err().kind,MemoryStoreErrorKind::ScopeViolation);
    assert!(matches!(store.mutate_with_commit_authority_if_unchanged(current,expected,Arc::new(||Ok(())))
        .await.unwrap(),MemoryStoreMutationResult::Changed(true)));
    assert_eq!(read(&store,&tenant,&original.id).await.unwrap().metadata,Some(metadata));
}
