use super::*;
use crate::store::*;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};

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
