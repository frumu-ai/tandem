use super::*;
use crate::store::*;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};

type CasBusyWitness = (std::sync::mpsc::SyncSender<()>, std::sync::mpsc::Receiver<()>);
static CAS_BUSY_WITNESS: std::sync::Mutex<Option<CasBusyWitness>> = std::sync::Mutex::new(None);

fn witness_cas_sqlite_writer(attempt: i32) -> bool {
    if attempt != 0 { return false; }
    let gate = CAS_BUSY_WITNESS.lock().unwrap();
    let Some((reached, released)) = gate.as_ref() else { return false; };
    reached.try_send(()).is_ok()
        && released.recv_timeout(std::time::Duration::from_secs(10)).is_ok()
}

fn tenant() -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: "commit-org".into(),
        workspace_id: "commit-workspace".into(),
        deployment_id: None,
    }
}
fn read_scope() -> MemoryReadScope {
    MemoryReadScope {
        tenant: tenant(),
        org_unit: Some("finance".into()),
        subject: Some("alice".into()),
        access: MemoryReadAccess::Scoped,
    }
}
fn record(id: &str) -> GlobalMemoryRecord {
    let content = format!("orchard lighthouse {id}");
    GlobalMemoryRecord {
        id: id.into(),
        user_id: "alice".into(),
        source_type: "fact".into(),
        content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
        content,
        run_id: "commit-run".into(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("commit-project".into()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(
            serde_json::json!({"owner_subject":"alice","owner_org_unit_id":"finance","classification":"internal"}),
        ),
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
fn write(record: GlobalMemoryRecord) -> MemoryStoreWriteRequest {
    MemoryStoreWriteRequest::GlobalRecord {
        scope: MemoryWriteScope {
            tenant: tenant(),
            org_unit: Some("finance".into()),
            subject: Some("alice".into()),
        },
        record,
    }
}
async fn get(store: &dyn MemoryStore, id: &str) -> Option<GlobalMemoryRecord> {
    match store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: read_scope(),
            id: id.into(),
        })
        .await
        .unwrap()
    {
        MemoryStoreReadResult::GlobalRecord(row) => row,
        other => panic!("{other:?}"),
    }
}
fn deny_second(calls: Arc<AtomicUsize>) -> MemoryCommitAuthority {
    Arc::new(move || {
        if calls.fetch_add(1, Ordering::SeqCst) == 1 {
            Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "expired commit authority",
            ))
        } else {
            Ok(())
        }
    })
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_sqlite_insert_and_context_update_rollback_on_precommit_denial() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
        .await
        .unwrap();
    let source = record("guarded-row");
    let calls = Arc::new(AtomicUsize::new(0));
    let error = store
        .write_with_commit_authority(write(source.clone()), deny_second(calls.clone()))
        .await
        .unwrap_err();
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        get(&store, &source.id).await.is_none(),
        "the actual SQLite INSERT must roll back"
    );
    let healthy = store
        .write_with_commit_authority(write(source.clone()), Arc::new(|| Ok(())))
        .await
        .unwrap();
    assert!(matches!(healthy,MemoryStoreWriteResult::GlobalRecord(result) if result.stored));
    let original = get(&store, &source.id).await.unwrap();
    let mut changed = original.metadata.clone().unwrap();
    changed["guarded"] = serde_json::json!(true);
    let calls = Arc::new(AtomicUsize::new(0));
    let mutation = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope: read_scope(),
        id: source.id.clone(),
        visibility: source.visibility.clone(),
        demoted: false,
        metadata: Some(changed.clone()),
        provenance: source.provenance.clone(),
    };
    let error = store
        .mutate_with_commit_authority(mutation.clone(), deny_second(calls.clone()))
        .await
        .unwrap_err();
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let retained = get(&store, &source.id).await.unwrap();
    assert_eq!(retained.metadata, original.metadata);
    assert_eq!(retained.content_hash, original.content_hash);
    assert_eq!(retained.updated_at_ms, original.updated_at_ms);
    assert!(matches!(
        store
            .mutate_with_commit_authority(mutation, Arc::new(|| Ok(())))
            .await
            .unwrap(),
        MemoryStoreMutationResult::Changed(true)
    ));
    assert_eq!(
        get(&store, &source.id).await.unwrap().metadata,
        Some(changed)
    );
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_sqlite_context_update_preserves_lineage_dedupe_and_private_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryDatabase::new(&dir.path().join("memory.sqlite3"))
        .await
        .unwrap();
    let source_a = record("source-a");
    let source_b = record("source-b");
    store.write(write(source_a.clone())).await.unwrap();
    store.write(write(source_b.clone())).await.unwrap();
    let make_lineage = |source: &GlobalMemoryRecord| {
        let restriction =
            crate::CanonicalMemoryRestriction::from_global_record(source, &tenant()).unwrap();
        crate::DerivedMemoryLineage::new(
            Some("alice".into()),
            Some("finance".into()),
            vec![restriction.clone()],
            vec![crate::CanonicalInputReference::Memory {
                source: restriction.source_reference(),
            }],
        )
        .unwrap()
    };
    let mut output = record("derived-row");
    output.metadata =
        crate::metadata_with_derived_lineage(output.metadata, &make_lineage(&source_a)).unwrap();
    store.write(write(output.clone())).await.unwrap();
    let next_metadata =
        crate::metadata_with_derived_lineage(output.metadata.clone(), &make_lineage(&source_b))
            .unwrap();
    store
        .mutate_with_commit_authority(
            MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                scope: read_scope(),
                id: output.id.clone(),
                visibility: output.visibility.clone(),
                demoted: false,
                metadata: next_metadata.clone(),
                provenance: output.provenance.clone(),
            },
            Arc::new(|| Ok(())),
        )
        .await
        .unwrap();
    let retained = get(&store, &output.id).await.unwrap();
    assert_eq!(retained.content_hash, output.content_hash);
    assert_eq!(retained.metadata, next_metadata);
    let mut repeated = retained;
    repeated.id = "repeat-id".into();
    let result = store
        .write_with_commit_authority(write(repeated), Arc::new(|| Ok(())))
        .await
        .unwrap();
    assert!(
        matches!(result,MemoryStoreWriteResult::GlobalRecord(result) if result.deduped && result.id==output.id),
        "context mutation must update the real lineage dedupe column"
    );
    let mut bob = read_scope();
    bob.subject = Some("bob".into());
    assert!(matches!(
        store
            .read(MemoryStoreReadRequest::GlobalRecord {
                scope: bob,
                id: output.id
            })
            .await
            .unwrap(),
        MemoryStoreReadResult::GlobalRecord(None)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn guarded_sqlite_cas_writer_wait_rejects_stale_policy_and_current_target_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite3");
    let store = Arc::new(MemoryDatabase::new(&path).await.unwrap());
    let original = record("cas-target");
    store.write(write(original.clone())).await.unwrap();
    let expected = crate::CanonicalMemoryRestriction::from_global_record(&original,&tenant()).unwrap().source_reference();
    let stale = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope: read_scope(), id: original.id.clone(), visibility: original.visibility.clone(),
        demoted: false, metadata: original.metadata.clone(), provenance: original.provenance.clone(),
    };
    let locker = rusqlite::Connection::open(&path).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (reached, waiting) = std::sync::mpsc::sync_channel(1);
    let (release, released) = std::sync::mpsc::sync_channel(1);
    *CAS_BUSY_WITNESS.lock().unwrap() = Some((reached,released));
    store.conn.lock().await.busy_handler(Some(witness_cas_sqlite_writer)).unwrap();
    let worker_store = store.clone();
    let worker = tokio::spawn(async move {
        worker_store.mutate_with_commit_authority_if_unchanged(stale,expected,Arc::new(||Ok(()))).await
    });
    waiting.recv_timeout(std::time::Duration::from_secs(10)).expect("actual BEGIN IMMEDIATE must wait for the independent writer");
    let mut tightened = original.metadata.clone().unwrap();
    tightened["classification"] = serde_json::json!("restricted");
    assert!(store.update_global_memory_context_on_connection(&locker,&original.id,&tenant().org_id,&tenant().workspace_id,
        None,Some("finance"),Some("alice"),&original.visibility,false,Some(&tightened),original.provenance.as_ref(),false).unwrap());
    locker.execute_batch("COMMIT").unwrap();
    release.send(()).unwrap();
    let error = worker.await.unwrap().unwrap_err();
    *CAS_BUSY_WITNESS.lock().unwrap() = None;
    store.conn.lock().await.busy_handler(None).unwrap();
    assert_eq!(error.kind,MemoryStoreErrorKind::ScopeViolation);
    assert!(error.message.contains("target changed"),"{error:?}");
    let retained = get(store.as_ref(),&original.id).await.unwrap();
    assert_eq!(retained.metadata,Some(tightened.clone()),"stale queued metadata cannot erase the newly committed restriction");
    assert_eq!(retained.content_hash,original.content_hash);
    let expected = crate::CanonicalMemoryRestriction::from_global_record(&retained,&tenant()).unwrap().source_reference();
    let mut current_metadata = tightened; current_metadata["cas_healthy"] = serde_json::json!(true);
    let current = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
        scope: read_scope(), id: retained.id.clone(), visibility: retained.visibility.clone(), demoted: false,
        metadata: Some(current_metadata.clone()), provenance: retained.provenance.clone(),
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
    assert_eq!(get(store.as_ref(),&original.id).await.unwrap().metadata,Some(current_metadata));
}
