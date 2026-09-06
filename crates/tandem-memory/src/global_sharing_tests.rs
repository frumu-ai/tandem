//! The same persisted global-record visibility contract runs on both backends.
use crate::store::*;
use crate::types::{GlobalMemoryRecord, MemoryTenantScope};
use serde_json::json;
use std::collections::BTreeSet;

fn record(
    id: &str,
    tenant: &MemoryTenantScope,
    owner: Option<&str>,
    unit: Option<&str>,
    shared: bool,
) -> GlobalMemoryRecord {
    GlobalMemoryRecord {
        id: format!("{}/{id}", tenant.org_id),
        user_id: "collector".into(),
        source_type: "note".into(),
        content: format!("lantern {id}"),
        content_hash: id.into(),
        run_id: "sharing".into(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("sharing".into()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(
            json!({"owner_subject": owner, "owner_org_unit_id": unit, "tenant_shared": shared}),
        ),
        provenance: Some(json!({"tenant_context": tenant})),
        redaction_status: "passed".into(),
        redaction_count: 0,
        visibility: "private".into(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: 1,
        updated_at_ms: 1,
        expires_at_ms: None,
    }
}

fn write(record: GlobalMemoryRecord, tenant: &MemoryTenantScope) -> MemoryStoreWriteRequest {
    MemoryStoreWriteRequest::GlobalRecord {
        scope: MemoryWriteScope {
            tenant: tenant.clone(),
            org_unit: crate::types::owner_org_unit_id_from_metadata(record.metadata.as_ref()),
            subject: crate::types::owner_subject_from_metadata(record.metadata.as_ref()),
        },
        record,
    }
}

fn scope(tenant: &MemoryTenantScope, user: Option<&str>, unit: &str) -> MemoryReadScope {
    MemoryReadScope {
        tenant: tenant.clone(),
        org_unit: Some(unit.into()),
        subject: user.map(str::to_string),
        access: MemoryReadAccess::Scoped,
    }
}

async fn assert_visible(
    store: &dyn MemoryStore,
    tenant: &MemoryTenantScope,
    user: Option<&str>,
    unit: &str,
    expected: &[&str],
) {
    let expected: BTreeSet<_> = expected.iter().map(|id| id.to_string()).collect();
    let scope = scope(tenant, user, unit);
    let requests = [
        MemoryStoreQueryRequest::SearchGlobalRecords {
            scope: scope.clone(),
            user_id: user.unwrap_or_default().into(),
            query: "lantern".into(),
            limit: 100,
            project_tag: Some("sharing".into()),
        },
        MemoryStoreQueryRequest::ListGlobalRecords {
            scope,
            user_id: user.unwrap_or_default().into(),
            query: None,
            project_tag: Some("sharing".into()),
            channel_tag: None,
            limit: 100,
            offset: 0,
        },
    ];
    for request in requests {
        let ids = match store.query(request).await.unwrap() {
            MemoryStoreQueryResult::GlobalSearchHits(hits) => hits
                .into_iter()
                .map(|hit| hit.record.id)
                .collect::<BTreeSet<_>>(),
            MemoryStoreQueryResult::GlobalRecords(rows) => {
                rows.into_iter().map(|row| row.id).collect()
            }
            other => panic!("unexpected query result: {other:?}"),
        };
        let ids: BTreeSet<_> = ids
            .into_iter()
            .map(|id| id.rsplit('/').next().unwrap().to_string())
            .collect();
        assert_eq!(ids, expected, "subject={user:?}, department={unit}");
    }
}

pub(crate) async fn assert_after_reopen(store: &dyn MemoryStore, tenant: &MemoryTenantScope) {
    assert_visible(
        store,
        tenant,
        Some("bob"),
        "ops",
        &["bob-personal", "tenant-shared"],
    )
    .await;
    assert_visible(
        store,
        tenant,
        Some("alice"),
        "ops",
        &["alice-personal", "tenant-shared"],
    )
    .await;
    assert_visible(store, tenant, None, "ops", &["tenant-shared"]).await;
    assert_visible(
        store,
        tenant,
        Some("bob"),
        "eng",
        &[
            "bob-personal",
            "dept-private",
            "dept-shared",
            "tenant-shared",
        ],
    )
    .await;
}

pub(crate) async fn exercise(store: &dyn MemoryStore, tenant: &MemoryTenantScope) {
    for (id, owner) in [("bob-personal", "bob"), ("alice-personal", "alice")] {
        assert!(
            matches!(store.write(write(record(id, tenant, Some(owner), None, true), tenant)).await.unwrap(), MemoryStoreWriteResult::GlobalRecord(result) if result.stored)
        );
    }
    let rows = [
        record("dept-private", tenant, Some("bob"), Some("eng"), true),
        record("dept-shared", tenant, None, Some("eng"), true),
        record("tenant-shared", tenant, None, None, true),
        record("unlabelled", tenant, None, None, false),
    ];
    let batch = store
        .batch(MemoryStoreBatchRequest {
            mode: MemoryStoreBatchMode::Atomic,
            operations: rows
                .into_iter()
                .map(|row| MemoryStoreBatchOperation::Write(write(row, tenant)))
                .collect(),
        })
        .await
        .unwrap();
    assert!(batch.completed);
    assert_after_reopen(store, tenant).await;
    let mut foreign = tenant.clone();
    foreign.org_id.push_str("-foreign");
    assert_visible(store, &foreign, Some("bob"), "eng", &[]).await;

    for (user, found) in [("bob", true), ("alice", false)] {
        let result = store
            .read(MemoryStoreReadRequest::GlobalRecord {
                scope: scope(tenant, Some(user), "ops"),
                id: format!("{}/bob-personal", tenant.org_id),
            })
            .await
            .unwrap();
        assert!(
            matches!(result, MemoryStoreReadResult::GlobalRecord(row) if row.is_some() == found)
        );
    }
    // Updates and deletes must retain private-owner restrictions even though
    // the caller can see other tenant-shared data.
    for atomic in [false, true] {
        let target = record("bob-personal", tenant, Some("bob"), None, true);
        let mutation = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: scope(tenant, Some("alice"), "ops"),
            id: target.id.clone(),
            visibility: "shared".into(),
            demoted: false,
            metadata: Some(json!({"tenant_shared": true})),
            provenance: target.provenance.clone(),
        };
        if atomic {
            let result = store
                .batch(MemoryStoreBatchRequest {
                    mode: MemoryStoreBatchMode::Atomic,
                    operations: vec![MemoryStoreBatchOperation::Mutation(mutation)],
                })
                .await
                .unwrap();
            assert!(matches!(
                &result.items[0].result,
                Ok(MemoryStoreBatchValue::Mutation(
                    MemoryStoreMutationResult::Changed(false)
                ))
            ));
        } else {
            assert!(matches!(
                store.mutate(mutation).await.unwrap(),
                MemoryStoreMutationResult::Changed(false)
            ));
        }
        let mutation = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: scope(tenant, Some("bob"), "ops"),
            id: target.id.clone(),
            visibility: "private".into(),
            demoted: false,
            metadata: target.metadata,
            provenance: target.provenance,
        };
        if atomic {
            let result = store
                .batch(MemoryStoreBatchRequest {
                    mode: MemoryStoreBatchMode::Atomic,
                    operations: vec![MemoryStoreBatchOperation::Mutation(mutation)],
                })
                .await
                .unwrap();
            assert!(matches!(
                &result.items[0].result,
                Ok(MemoryStoreBatchValue::Mutation(
                    MemoryStoreMutationResult::Changed(true)
                ))
            ));
        } else {
            assert!(matches!(
                store.mutate(mutation).await.unwrap(),
                MemoryStoreMutationResult::Changed(true)
            ));
        }
        assert!(matches!(
            store
                .mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {
                    scope: scope(tenant, Some("alice"), "ops"),
                    id: format!("{}/bob-personal", tenant.org_id)
                })
                .await
                .unwrap(),
            MemoryStoreMutationResult::Changed(false)
        ));
    }
    assert_after_reopen(store, tenant).await;

    // Identical content with a different sharing boundary is a distinct record.
    // Both normal and atomic insertion must use the new dedupe dimension.
    for atomic in [false, true] {
        let prefix = if atomic { "atomic" } else { "ordinary" };
        let mut first = record(
            &format!("{prefix}-unshared"),
            tenant,
            Some("bob"),
            None,
            false,
        );
        first.project_tag = Some("dedupe".into());
        let mut second = first.clone();
        second.id = format!("{}/{prefix}-shared", tenant.org_id);
        second.metadata.as_mut().unwrap()["tenant_shared"] = json!(true);
        if atomic {
            let result = store
                .batch(MemoryStoreBatchRequest {
                    mode: MemoryStoreBatchMode::Atomic,
                    operations: vec![
                        MemoryStoreBatchOperation::Write(write(first, tenant)),
                        MemoryStoreBatchOperation::Write(write(second, tenant)),
                    ],
                })
                .await
                .unwrap();
            assert!(result.completed);
            for item in result.items {
                assert!(
                    matches!(item.result, Ok(MemoryStoreBatchValue::Write(MemoryStoreWriteResult::GlobalRecord(row))) if row.stored && !row.deduped)
                );
            }
        } else {
            for value in [first, second] {
                assert!(
                    matches!(store.write(write(value, tenant)).await.unwrap(), MemoryStoreWriteResult::GlobalRecord(row) if row.stored && !row.deduped)
                );
            }
        }
        let mut updated = record(
            &format!("{prefix}-updated"),
            tenant,
            Some("bob"),
            None,
            false,
        );
        updated.project_tag = Some("updates".into());
        store.write(write(updated.clone(), tenant)).await.unwrap();
        updated.metadata.as_mut().unwrap()["tenant_shared"] = json!(true);
        let mut author = scope(tenant, Some("bob"), "ops");
        author.org_unit = None;
        let update = MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: author,
            id: updated.id.clone(),
            visibility: "private".into(),
            demoted: false,
            metadata: updated.metadata,
            provenance: updated.provenance,
        };
        if atomic {
            let result = store
                .batch(MemoryStoreBatchRequest {
                    mode: MemoryStoreBatchMode::Atomic,
                    operations: vec![MemoryStoreBatchOperation::Mutation(update)],
                })
                .await
                .unwrap();
            assert!(matches!(
                &result.items[0].result,
                Ok(MemoryStoreBatchValue::Mutation(
                    MemoryStoreMutationResult::Changed(true)
                ))
            ));
        } else {
            assert!(matches!(
                store.mutate(update).await.unwrap(),
                MemoryStoreMutationResult::Changed(true)
            ));
        }
        assert!(matches!(
            store
                .read(MemoryStoreReadRequest::GlobalRecord {
                    scope: scope(tenant, Some("bob"), "ops"),
                    id: updated.id.clone()
                })
                .await
                .unwrap(),
            MemoryStoreReadResult::GlobalRecord(Some(_))
        ));
        let delete = MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope: scope(tenant, Some("bob"), "ops"),
            id: updated.id,
        };
        if atomic {
            let result = store
                .batch(MemoryStoreBatchRequest {
                    mode: MemoryStoreBatchMode::Atomic,
                    operations: vec![MemoryStoreBatchOperation::Mutation(delete)],
                })
                .await
                .unwrap();
            assert!(matches!(
                &result.items[0].result,
                Ok(MemoryStoreBatchValue::Mutation(
                    MemoryStoreMutationResult::Changed(true)
                ))
            ));
        } else {
            assert!(matches!(
                store.mutate(delete).await.unwrap(),
                MemoryStoreMutationResult::Changed(true)
            ));
        }
    }
}

#[tokio::test]
async fn sqlite_global_sharing_survives_department_change_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("memory.sqlite");
    let tenant = MemoryTenantScope {
        org_id: "sharing-org".into(),
        workspace_id: "workspace".into(),
        deployment_id: Some("deployment".into()),
    };
    let db = crate::db::MemoryDatabase::new(&path).await.unwrap();
    exercise(&db, &tenant).await;
    // The LIKE fallback must enforce the same scope when FTS is unavailable.
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("DROP TABLE memory_records_fts", [])
        .unwrap();
    assert_after_reopen(&db, &tenant).await;
    drop(db);
    let reopened = crate::db::MemoryDatabase::new(&path).await.unwrap();
    assert_after_reopen(&reopened, &tenant).await;
}
