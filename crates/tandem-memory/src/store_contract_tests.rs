use tempfile::TempDir;

use crate::db::MemoryDatabase;
use crate::types::{
    GlobalMemoryRecord, LayerType, MemoryChunk, MemoryError, MemoryTenantScope, MemoryTier,
    NodeType, DEFAULT_EMBEDDING_DIMENSION,
};

use super::*;

async fn test_store() -> (MemoryDatabase, TempDir) {
    let temp_dir = TempDir::new().expect("create temporary store directory");
    let database = MemoryDatabase::new(&temp_dir.path().join("contract.db"))
        .await
        .expect("create contract database");
    (database, temp_dir)
}

fn global_record(id: &str, subject: Option<&str>) -> GlobalMemoryRecord {
    GlobalMemoryRecord {
        id: id.to_string(),
        user_id: "legacy-user".to_string(),
        source_type: "contract_test".to_string(),
        content: format!("visibility contract record {id}"),
        content_hash: format!("hash-{id}"),
        run_id: "run-contract".to_string(),
        session_id: None,
        message_id: Some(id.to_string()),
        tool_name: None,
        project_tag: None,
        channel_tag: None,
        host_tag: None,
        metadata: subject.map(|subject| serde_json::json!({ "owner_subject": subject })),
        provenance: None,
        redaction_status: "none".to_string(),
        redaction_count: 0,
        visibility: "shared".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: 1,
        updated_at_ms: 1,
        expires_at_ms: None,
    }
}

fn tenant(org_id: &str) -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: org_id.to_string(),
        workspace_id: "contract-workspace".to_string(),
        deployment_id: Some("contract-deployment".to_string()),
    }
}

fn chunk(
    id: &str,
    tier: MemoryTier,
    session_id: Option<&str>,
    project_id: Option<&str>,
    tenant_scope: MemoryTenantScope,
) -> MemoryChunk {
    MemoryChunk {
        id: id.to_string(),
        content: format!("contract chunk {id}"),
        tier,
        session_id: session_id.map(ToString::to_string),
        project_id: project_id.map(ToString::to_string),
        source: "contract_test".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope,
        subject: None,
        created_at: chrono::Utc::now(),
        token_count: 3,
        metadata: None,
    }
}

fn embedding(x: f32, y: f32) -> Vec<f32> {
    let mut embedding = vec![0.0; DEFAULT_EMBEDDING_DIMENSION];
    embedding[0] = x;
    embedding[1] = y;
    embedding
}

fn assert_scope_violation(error: MemoryStoreError) {
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
    assert!(!error.retryable);
}

async fn create_context_node(
    store: &dyn MemoryStore,
    tenant_scope: MemoryTenantScope,
    uri: &str,
    parent_uri: Option<&str>,
    node_type: NodeType,
) -> String {
    let result = store
        .write(MemoryStoreWriteRequest::ContextNode {
            scope: MemoryWriteScope::tenant(tenant_scope),
            uri: uri.to_string(),
            parent_uri: parent_uri.map(ToString::to_string),
            node_type,
            metadata: None,
        })
        .await
        .expect("create context node through contract");
    let MemoryStoreWriteResult::ContextNodeCreated(id) = result else {
        panic!("expected context node id");
    };
    id
}

#[test]
fn memory_store_is_object_safe() {
    fn accepts_trait_object(_: &dyn MemoryStore) {}
    let _: fn(&dyn MemoryStore) = accepts_trait_object;
}

#[test]
fn contract_errors_do_not_expose_driver_types() {
    let scope_error = MemoryStoreError::from(MemoryError::TenantScopeViolation(
        "wrong tenant".to_string(),
    ));
    assert_eq!(scope_error.kind, MemoryStoreErrorKind::ScopeViolation);
    assert!(!scope_error.retryable);

    let invalid = MemoryStoreError::from(MemoryError::InvalidConfig("bad limit".to_string()));
    assert_eq!(invalid.kind, MemoryStoreErrorKind::InvalidRequest);
}

#[test]
fn chunk_selectors_encode_tier_requirements() {
    let session = MemoryChunkSelector::session("session-a");
    assert_eq!(session.tier, crate::types::MemoryTier::Session);
    assert_eq!(session.session_id.as_deref(), Some("session-a"));
    assert!(session.project_id.is_none());

    let project = MemoryChunkSelector::project("project-a");
    assert_eq!(project.tier, crate::types::MemoryTier::Project);
    assert_eq!(project.project_id.as_deref(), Some("project-a"));
}

#[tokio::test]
async fn sqlite_adapter_reports_only_migration_features_it_exposes() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;

    let available = store
        .migration_capabilities(MemoryMigrationCapabilityRequest {
            require_version_introspection: false,
            require_transactional_apply: false,
            require_online_apply: false,
            require_dry_run: false,
        })
        .await
        .expect("read migration capabilities");
    assert_eq!(available.backend, MemoryBackendKind::Sqlite);
    assert_eq!(available.apply_mode, MemoryMigrationApplyMode::OnOpen);
    assert!(available.requirements_satisfied);

    let transactional = store
        .migration_capabilities(MemoryMigrationCapabilityRequest {
            require_version_introspection: false,
            require_transactional_apply: true,
            require_online_apply: false,
            require_dry_run: false,
        })
        .await
        .expect("read migration capabilities");
    assert!(!transactional.transactional_apply);
    assert!(!transactional.requirements_satisfied);
}

#[tokio::test]
async fn sqlite_adapter_preflights_unsupported_atomic_operations() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let error = store
        .batch(MemoryStoreBatchRequest {
            mode: MemoryStoreBatchMode::Atomic,
            operations: vec![
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: MemoryWriteScope::tenant(tenant.clone()),
                    record: global_record("atomic-preflight", None),
                }),
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::Chunk {
                    scope: MemoryWriteScope::tenant(tenant.clone()),
                    chunk: chunk(
                        "unsupported-chunk",
                        MemoryTier::Global,
                        None,
                        None,
                        tenant.clone(),
                    ),
                    embedding: embedding(1.0, 0.0),
                }),
            ],
        })
        .await
        .expect_err("unsupported atomic operations must fail preflight");
    assert_eq!(error.kind, MemoryStoreErrorKind::Unsupported);

    let read = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::tenant(tenant),
            id: "atomic-preflight".to_string(),
        })
        .await
        .expect("read record after failed preflight");
    assert!(matches!(read, MemoryStoreReadResult::GlobalRecord(None)));
}

#[tokio::test]
async fn sqlite_adapter_atomic_batch_commits_global_record_crud() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let scope = MemoryWriteScope::tenant(tenant.clone());
    let read_scope = MemoryReadScope::tenant(tenant.clone());

    let result = store
        .batch(MemoryStoreBatchRequest {
            mode: MemoryStoreBatchMode::Atomic,
            operations: vec![
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: scope.clone(),
                    record: global_record("atomic-kept", None),
                }),
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope,
                    record: global_record("atomic-deleted", None),
                }),
                MemoryStoreBatchOperation::Mutation(
                    MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                        scope: read_scope.clone(),
                        id: "atomic-kept".to_string(),
                        visibility: "team".to_string(),
                        demoted: true,
                        metadata: None,
                        provenance: None,
                    },
                ),
                MemoryStoreBatchOperation::Mutation(
                    MemoryStoreMutationRequest::DeleteGlobalRecord {
                        scope: read_scope.clone(),
                        id: "atomic-deleted".to_string(),
                    },
                ),
            ],
        })
        .await
        .expect("commit supported atomic CRUD batch");
    assert!(result.completed);
    assert_eq!(result.items.len(), 4);
    assert!(result.items.iter().all(|item| item.result.is_ok()));

    let kept = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: read_scope.clone(),
            id: "atomic-kept".to_string(),
        })
        .await
        .expect("read committed updated record");
    let MemoryStoreReadResult::GlobalRecord(Some(kept)) = kept else {
        panic!("expected committed global record");
    };
    assert_eq!(kept.visibility, "team");
    assert!(kept.demoted);

    let deleted = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: read_scope,
            id: "atomic-deleted".to_string(),
        })
        .await
        .expect("read deleted record");
    assert!(matches!(deleted, MemoryStoreReadResult::GlobalRecord(None)));
}

#[tokio::test]
async fn sqlite_adapter_atomic_batch_rolls_back_after_forced_failure() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let scope = MemoryWriteScope::tenant(tenant.clone());
    let first = global_record("atomic-rollback", None);
    let mut conflicting = first.clone();
    conflicting.content = "different content with the same primary key".to_string();
    conflicting.content_hash = "different-content-hash".to_string();
    conflicting.message_id = Some("different-message".to_string());

    let error = store
        .batch(MemoryStoreBatchRequest {
            mode: MemoryStoreBatchMode::Atomic,
            operations: vec![
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: scope.clone(),
                    record: first,
                }),
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope,
                    record: conflicting,
                }),
            ],
        })
        .await
        .expect_err("primary-key failure must abort the atomic batch");
    assert_eq!(error.kind, MemoryStoreErrorKind::Internal);

    let read = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::tenant(tenant),
            id: "atomic-rollback".to_string(),
        })
        .await
        .expect("read record after atomic rollback");
    assert!(matches!(read, MemoryStoreReadResult::GlobalRecord(None)));
}

#[tokio::test]
async fn sqlite_adapter_strict_mode_rejects_local_global_record_operations() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let write_scope = MemoryWriteScope::tenant(tenant.clone());
    let read_scope = MemoryReadScope::tenant(tenant.clone());

    for id in ["strict-read", "strict-update", "strict-delete"] {
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: write_scope.clone(),
                record: global_record(id, None),
            })
            .await
            .expect("seed local global record before strict mode");
    }

    database.set_strict_tenant_enforcement(true);

    let error = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: read_scope.clone(),
            id: "strict-read".to_string(),
        })
        .await
        .expect_err("strict mode must reject local global-record point reads");
    assert_scope_violation(error);

    let error = store
        .query(MemoryStoreQueryRequest::SearchGlobalRecords {
            scope: read_scope.clone(),
            user_id: "legacy-user".to_string(),
            query: "visibility".to_string(),
            limit: 10,
            project_tag: None,
        })
        .await
        .expect_err("strict mode must reject local global-record searches");
    assert_scope_violation(error);

    let error = store
        .query(MemoryStoreQueryRequest::ListGlobalRecords {
            scope: read_scope.clone(),
            user_id: "legacy-user".to_string(),
            query: None,
            project_tag: None,
            channel_tag: None,
            limit: 10,
            offset: 0,
        })
        .await
        .expect_err("strict mode must reject local global-record lists");
    assert_scope_violation(error);

    let error = store
        .write(MemoryStoreWriteRequest::GlobalRecord {
            scope: write_scope,
            record: global_record("strict-write", None),
        })
        .await
        .expect_err("strict mode must reject local global-record writes");
    assert_scope_violation(error);

    let error = store
        .mutate(MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: read_scope.clone(),
            id: "strict-update".to_string(),
            visibility: "team".to_string(),
            demoted: true,
            metadata: None,
            provenance: None,
        })
        .await
        .expect_err("strict mode must reject local global-record updates");
    assert_scope_violation(error);

    let error = store
        .mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope: read_scope,
            id: "strict-delete".to_string(),
        })
        .await
        .expect_err("strict mode must reject local global-record deletes");
    assert_scope_violation(error);

    database.set_strict_tenant_enforcement(false);

    let update = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::tenant(tenant.clone()),
            id: "strict-update".to_string(),
        })
        .await
        .expect("inspect rejected update");
    let MemoryStoreReadResult::GlobalRecord(Some(update)) = update else {
        panic!("expected unchanged update fixture");
    };
    assert_eq!(update.visibility, "shared");
    assert!(!update.demoted);

    for id in ["strict-delete", "strict-read"] {
        let result = store
            .read(MemoryStoreReadRequest::GlobalRecord {
                scope: MemoryReadScope::tenant(tenant.clone()),
                id: id.to_string(),
            })
            .await
            .expect("inspect retained fixture");
        assert!(matches!(
            result,
            MemoryStoreReadResult::GlobalRecord(Some(_))
        ));
    }

    let write = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::tenant(tenant),
            id: "strict-write".to_string(),
        })
        .await
        .expect("inspect rejected write");
    assert!(matches!(write, MemoryStoreReadResult::GlobalRecord(None)));
}

#[tokio::test]
async fn sqlite_adapter_strict_mode_rejects_local_atomic_batches() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let write_scope = MemoryWriteScope::tenant(tenant.clone());
    let read_scope = MemoryReadScope::tenant(tenant.clone());

    for id in ["strict-atomic-update", "strict-atomic-delete"] {
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: write_scope.clone(),
                record: global_record(id, None),
            })
            .await
            .expect("seed local atomic fixture before strict mode");
    }

    database.set_strict_tenant_enforcement(true);

    let operations = [
        MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
            scope: write_scope,
            record: global_record("strict-atomic-write", None),
        }),
        MemoryStoreBatchOperation::Mutation(
            MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                scope: read_scope.clone(),
                id: "strict-atomic-update".to_string(),
                visibility: "team".to_string(),
                demoted: true,
                metadata: None,
                provenance: None,
            },
        ),
        MemoryStoreBatchOperation::Mutation(MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope: read_scope,
            id: "strict-atomic-delete".to_string(),
        }),
    ];

    for operation in operations {
        let error = store
            .batch(MemoryStoreBatchRequest {
                mode: MemoryStoreBatchMode::Atomic,
                operations: vec![operation],
            })
            .await
            .expect_err("strict mode must reject a local-scope atomic batch");
        assert_scope_violation(error);
    }

    database.set_strict_tenant_enforcement(false);

    for id in ["strict-atomic-update", "strict-atomic-delete"] {
        let result = store
            .read(MemoryStoreReadRequest::GlobalRecord {
                scope: MemoryReadScope::tenant(tenant.clone()),
                id: id.to_string(),
            })
            .await
            .expect("inspect atomic fixture after rejected batch");
        let MemoryStoreReadResult::GlobalRecord(Some(record)) = result else {
            panic!("expected retained atomic fixture");
        };
        assert_eq!(record.visibility, "shared");
        assert!(!record.demoted);
    }

    let write = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::tenant(tenant),
            id: "strict-atomic-write".to_string(),
        })
        .await
        .expect("inspect rejected atomic write");
    assert!(matches!(write, MemoryStoreReadResult::GlobalRecord(None)));
}

#[tokio::test]
async fn import_index_round_trips_through_typed_contract() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let selector = MemoryChunkSelector::project("project-a");

    store
        .write(MemoryStoreWriteRequest::ImportIndexEntry {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            selector: selector.clone(),
            path: "src/main.rs".to_string(),
            entry: MemoryImportIndexEntry {
                modified_at: 42,
                size: 128,
                hash: "abc123".to_string(),
            },
        })
        .await
        .expect("write import index entry");

    let result = store
        .read(MemoryStoreReadRequest::ImportIndexEntry {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: selector.clone(),
            path: "src/main.rs".to_string(),
        })
        .await
        .expect("read import index entry");
    let MemoryStoreReadResult::ImportIndexEntry(Some(entry)) = result else {
        panic!("expected import index entry");
    };
    assert_eq!(entry.modified_at, 42);
    assert_eq!(entry.size, 128);
    assert_eq!(entry.hash, "abc123");

    let paths = store
        .query(MemoryStoreQueryRequest::ImportIndexPaths {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: selector.clone(),
        })
        .await
        .expect("list import index paths");
    let MemoryStoreQueryResult::Paths(paths) = paths else {
        panic!("expected path list");
    };
    assert_eq!(paths, vec!["src/main.rs"]);

    store
        .mutate(MemoryStoreMutationRequest::DeleteImportIndexEntry {
            scope: MemoryReadScope::tenant(tenant),
            selector,
            path: "src/main.rs".to_string(),
        })
        .await
        .expect("delete import index entry");
}

#[tokio::test]
async fn global_query_forwards_shared_and_private_subject_scope() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();

    for (id, subject) in [("shared", None), ("private-a", Some("subject-a"))] {
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope {
                    tenant: tenant.clone(),
                    org_unit: None,
                    subject: subject.map(ToString::to_string),
                },
                record: global_record(id, subject),
            })
            .await
            .expect("write global contract record");
    }

    for (subject, expected) in [
        (None, vec!["shared"]),
        (Some("subject-a"), vec!["private-a", "shared"]),
        (Some("subject-b"), vec!["shared"]),
    ] {
        let mut scope = MemoryReadScope::tenant(tenant.clone());
        scope.subject = subject.map(ToString::to_string);
        let result = store
            .query(MemoryStoreQueryRequest::SearchGlobalRecords {
                scope,
                user_id: "legacy-user".to_string(),
                query: "visibility".to_string(),
                limit: 10,
                project_tag: None,
            })
            .await
            .expect("search scoped global records");
        let MemoryStoreQueryResult::GlobalSearchHits(hits) = result else {
            panic!("expected global search hits");
        };
        let mut ids = hits
            .into_iter()
            .map(|hit| hit.record.id)
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, expected);
    }
}

#[tokio::test]
async fn chunk_list_applies_private_and_department_scope_before_limit() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = tenant("chunk-list-scope");

    let fixtures = [
        ("shared-finance", None, Some("finance"), false),
        ("private-owner", Some("subject-a"), Some("finance"), false),
        ("private-peer", Some("subject-b"), Some("finance"), false),
        ("shared-legal", None, Some("legal"), false),
        ("tenant-shared-legal", None, Some("legal"), true),
    ];

    for (index, (id, subject, org_unit, tenant_shared)) in fixtures.into_iter().enumerate() {
        let mut value = chunk(id, MemoryTier::Global, None, None, tenant.clone());
        value.subject = subject.map(ToString::to_string);
        value.created_at = chrono::Utc::now() + chrono::Duration::seconds(index as i64);
        value.metadata = Some(serde_json::json!({
            "owner_org_unit_id": org_unit,
            "tenant_shared": tenant_shared,
        }));
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope {
                    tenant: tenant.clone(),
                    org_unit: org_unit.map(ToString::to_string),
                    subject: subject.map(ToString::to_string),
                },
                chunk: value,
                embedding: embedding(1.0, 0.0),
            })
            .await
            .expect("write scoped chunk fixture");
    }

    let cases = [
        (
            None,
            None,
            vec!["tenant-shared-legal", "shared-legal", "shared-finance"],
        ),
        (
            Some("subject-a"),
            Some("finance"),
            vec!["tenant-shared-legal", "private-owner", "shared-finance"],
        ),
        (
            Some("subject-b"),
            Some("finance"),
            vec!["tenant-shared-legal", "private-peer", "shared-finance"],
        ),
    ];

    for (subject, org_unit, expected) in cases {
        let result = store
            .read(MemoryStoreReadRequest::Chunks {
                scope: MemoryReadScope {
                    tenant: tenant.clone(),
                    org_unit: org_unit.map(ToString::to_string),
                    subject: subject.map(ToString::to_string),
                    access: MemoryReadAccess::Scoped,
                },
                selector: MemoryChunkSelector::global(),
                limit: Some(10),
            })
            .await
            .expect("read scoped chunks");
        let MemoryStoreReadResult::Chunks(chunks) = result else {
            panic!("expected chunks");
        };
        assert_eq!(
            chunks.into_iter().map(|chunk| chunk.id).collect::<Vec<_>>(),
            expected
        );
    }

    let limited = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant),
            selector: MemoryChunkSelector::global(),
            limit: Some(1),
        })
        .await
        .expect("read shared chunks with a limit");
    let MemoryStoreReadResult::Chunks(limited) = limited else {
        panic!("expected chunks");
    };
    assert_eq!(limited[0].id, "tenant-shared-legal");
}

#[tokio::test]
async fn chunk_delete_enforces_owner_scope_in_the_mutation() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = tenant("chunk-delete-scope");

    for (id, subject) in [
        ("delete-shared", None),
        ("delete-owner-a", Some("subject-a")),
        ("delete-owner-b", Some("subject-b")),
    ] {
        let mut value = chunk(id, MemoryTier::Global, None, None, tenant.clone());
        value.subject = subject.map(ToString::to_string);
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope {
                    tenant: tenant.clone(),
                    org_unit: None,
                    subject: subject.map(ToString::to_string),
                },
                chunk: value,
                embedding: embedding(1.0, 0.0),
            })
            .await
            .expect("write delete fixture");
    }

    let delete = |subject: Option<&str>, id: &str, unrestricted: bool| {
        let mut scope = if unrestricted {
            MemoryReadScope::trusted_unrestricted(tenant.clone())
        } else {
            MemoryReadScope::tenant(tenant.clone())
        };
        scope.subject = subject.map(ToString::to_string);
        MemoryStoreMutationRequest::DeleteChunk {
            scope,
            selector: MemoryChunkSelector::global(),
            chunk_id: id.to_string(),
        }
    };

    let peer_attempt = store
        .mutate(delete(Some("subject-b"), "delete-owner-a", false))
        .await
        .expect("peer delete is a scoped miss");
    assert!(matches!(
        peer_attempt,
        MemoryStoreMutationResult::Affected(0)
    ));

    let owner_delete = store
        .mutate(delete(Some("subject-a"), "delete-owner-a", false))
        .await
        .expect("owner deletes private chunk");
    assert!(matches!(
        owner_delete,
        MemoryStoreMutationResult::Affected(1)
    ));

    let shared_delete = store
        .mutate(delete(Some("subject-b"), "delete-shared", false))
        .await
        .expect("peer deletes shared chunk");
    assert!(matches!(
        shared_delete,
        MemoryStoreMutationResult::Affected(1)
    ));

    let shared_only_attempt = store
        .mutate(delete(None, "delete-owner-b", false))
        .await
        .expect("shared-only delete is a scoped miss");
    assert!(matches!(
        shared_only_attempt,
        MemoryStoreMutationResult::Affected(0)
    ));

    let maintenance_delete = store
        .mutate(delete(None, "delete-owner-b", true))
        .await
        .expect("trusted maintenance deletes private chunk");
    assert!(matches!(
        maintenance_delete,
        MemoryStoreMutationResult::Affected(1)
    ));
}

#[tokio::test]
async fn global_point_operations_enforce_department_and_subject_scope() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant = MemoryTenantScope::local();
    let metadata = serde_json::json!({
        "owner_org_unit_id": "finance",
        "owner_subject": "subject-a"
    });

    for id in ["read-a", "update-a", "delete-a"] {
        let mut record = global_record(id, Some("subject-a"));
        record.metadata = Some(metadata.clone());
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope {
                    tenant: tenant.clone(),
                    org_unit: Some("finance".to_string()),
                    subject: Some("subject-a".to_string()),
                },
                record,
            })
            .await
            .expect("write private department record");
    }

    let peer_scope = MemoryReadScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("subject-b".to_string()),
        access: MemoryReadAccess::Scoped,
    };
    let owner_scope = MemoryReadScope {
        tenant,
        org_unit: Some("finance".to_string()),
        subject: Some("subject-a".to_string()),
        access: MemoryReadAccess::Scoped,
    };

    let peer_read = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: peer_scope.clone(),
            id: "read-a".to_string(),
        })
        .await
        .expect("peer read is a scoped miss");
    assert!(matches!(
        peer_read,
        MemoryStoreReadResult::GlobalRecord(None)
    ));

    let owner_read = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: owner_scope.clone(),
            id: "read-a".to_string(),
        })
        .await
        .expect("owner point read");
    assert!(matches!(
        owner_read,
        MemoryStoreReadResult::GlobalRecord(Some(_))
    ));

    let peer_update = store
        .mutate(MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: peer_scope.clone(),
            id: "update-a".to_string(),
            visibility: "shared".to_string(),
            demoted: false,
            metadata: Some(metadata.clone()),
            provenance: None,
        })
        .await
        .expect("peer update is a scoped miss");
    assert!(matches!(
        peer_update,
        MemoryStoreMutationResult::Changed(false)
    ));

    let owner_update = store
        .mutate(MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: owner_scope.clone(),
            id: "update-a".to_string(),
            visibility: "shared".to_string(),
            demoted: false,
            metadata: Some(metadata),
            provenance: None,
        })
        .await
        .expect("owner update");
    assert!(matches!(
        owner_update,
        MemoryStoreMutationResult::Changed(true)
    ));

    let peer_delete = store
        .mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope: peer_scope,
            id: "delete-a".to_string(),
        })
        .await
        .expect("peer delete is a scoped miss");
    assert!(matches!(
        peer_delete,
        MemoryStoreMutationResult::Changed(false)
    ));

    let owner_delete = store
        .mutate(MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope: owner_scope,
            id: "delete-a".to_string(),
        })
        .await
        .expect("owner delete");
    assert!(matches!(
        owner_delete,
        MemoryStoreMutationResult::Changed(true)
    ));
}

#[tokio::test]
async fn context_nodes_and_layers_round_trip_without_cross_tenant_visibility() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant_a = tenant("context-a");
    let tenant_b = tenant("context-b");
    let root_uri = "memory://contract";
    let child_uri = "memory://contract/child.txt";

    create_context_node(store, tenant_a.clone(), root_uri, None, NodeType::Directory).await;
    let child_id = create_context_node(
        store,
        tenant_a.clone(),
        child_uri,
        Some(root_uri),
        NodeType::File,
    )
    .await;

    let point = store
        .read(MemoryStoreReadRequest::ContextNode {
            scope: MemoryReadScope::tenant(tenant_a.clone()),
            uri: child_uri.to_string(),
        })
        .await
        .expect("get context node");
    assert!(matches!(point, MemoryStoreReadResult::ContextNode(Some(_))));

    let foreign_point = store
        .read(MemoryStoreReadRequest::ContextNode {
            scope: MemoryReadScope::tenant(tenant_b.clone()),
            uri: child_uri.to_string(),
        })
        .await
        .expect("foreign context node is a scoped miss");
    assert!(matches!(
        foreign_point,
        MemoryStoreReadResult::ContextNode(None)
    ));

    let children = store
        .query(MemoryStoreQueryRequest::ContextNodes {
            scope: MemoryReadScope::tenant(tenant_a.clone()),
            parent_uri: root_uri.to_string(),
        })
        .await
        .expect("list context directory");
    let MemoryStoreQueryResult::ContextNodes(children) = children else {
        panic!("expected context node list");
    };
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, child_id);

    let tree = store
        .query(MemoryStoreQueryRequest::ContextTree {
            scope: MemoryReadScope::tenant(tenant_a.clone()),
            parent_uri: root_uri.to_string(),
            max_depth: 2,
        })
        .await
        .expect("read context tree");
    let MemoryStoreQueryResult::ContextTree(tree) = tree else {
        panic!("expected context tree");
    };
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].node.id, child_id);

    let created = store
        .write(MemoryStoreWriteRequest::ContextLayer {
            scope: MemoryWriteScope::tenant(tenant_a.clone()),
            node_id: child_id.clone(),
            layer_type: LayerType::L2,
            content: "layer detail".to_string(),
            token_count: 2,
            source_chunk_id: None,
        })
        .await
        .expect("create context layer");
    assert!(matches!(
        created,
        MemoryStoreWriteResult::ContextLayerCreated(_)
    ));

    let layer = store
        .read(MemoryStoreReadRequest::ContextLayer {
            scope: MemoryReadScope::tenant(tenant_a),
            node_id: child_id.clone(),
            layer_type: LayerType::L2,
        })
        .await
        .expect("get context layer");
    let MemoryStoreReadResult::ContextLayer(Some(layer)) = layer else {
        panic!("expected context layer");
    };
    assert_eq!(layer.content, "layer detail");

    let foreign_layer = store
        .read(MemoryStoreReadRequest::ContextLayer {
            scope: MemoryReadScope::tenant(tenant_b),
            node_id: child_id,
            layer_type: LayerType::L2,
        })
        .await
        .expect("foreign context layer is a scoped miss");
    assert!(matches!(
        foreign_layer,
        MemoryStoreReadResult::ContextLayer(None)
    ));
}

#[tokio::test]
async fn context_operations_fail_closed_on_unimplemented_narrowing() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let mut scope = MemoryReadScope::tenant(tenant("context-narrowed"));
    scope.subject = Some("subject-a".to_string());

    let error = store
        .read(MemoryStoreReadRequest::ContextNode {
            scope,
            uri: "memory://contract".to_string(),
        })
        .await
        .expect_err("context subject narrowing must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
}

#[tokio::test]
async fn session_vector_search_can_span_sessions_but_not_tenants() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant_a = tenant("session-search-a");
    let tenant_b = tenant("session-search-b");

    for (id, session_id, tenant_scope) in [
        ("session-a1", "session-1", tenant_a.clone()),
        ("session-a2", "session-2", tenant_a.clone()),
        ("session-b1", "session-1", tenant_b),
    ] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                chunk: chunk(
                    id,
                    MemoryTier::Session,
                    Some(session_id),
                    None,
                    tenant_scope,
                ),
                embedding: embedding(1.0, 0.0),
            })
            .await
            .expect("store session chunk");
    }

    let result = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant_a),
            selector: MemoryChunkSelector::all_sessions(),
            query_embedding: embedding(1.0, 0.0),
            limit: 10,
        })
        .await
        .expect("search all visible sessions");
    let MemoryStoreQueryResult::SimilarChunks(hits) = result else {
        panic!("expected similar chunks");
    };
    let mut ids = hits
        .into_iter()
        .map(|(chunk, _)| chunk.id)
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["session-a1", "session-a2"]);
}

#[tokio::test]
async fn cleanup_log_writes_and_project_cap_eviction_are_tenant_scoped() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant_a = tenant("maintenance-a");
    let tenant_b = tenant("maintenance-b");

    for (id, tenant_scope) in [
        ("project-a1", tenant_a.clone()),
        ("project-a2", tenant_a.clone()),
        ("project-a3", tenant_a.clone()),
        ("project-b1", tenant_b.clone()),
    ] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                chunk: chunk(
                    id,
                    MemoryTier::Project,
                    None,
                    Some("shared-project"),
                    tenant_scope,
                ),
                embedding: embedding(0.0, 1.0),
            })
            .await
            .expect("store project chunk");
    }

    let evicted = store
        .mutate(MemoryStoreMutationRequest::EnforceProjectChunkCap {
            scope: MemoryReadScope::tenant(tenant_a.clone()),
            project_id: "shared-project".to_string(),
            max_chunks: 1,
        })
        .await
        .expect("enforce project chunk cap");
    assert!(matches!(evicted, MemoryStoreMutationResult::Affected(2)));

    store
        .write(MemoryStoreWriteRequest::CleanupLog {
            scope: MemoryWriteScope::tenant(tenant_a.clone()),
            entry: MemoryCleanupLogWrite {
                cleanup_type: "contract-cap".to_string(),
                tier: MemoryTier::Project,
                project_id: Some("shared-project".to_string()),
                session_id: None,
                chunks_deleted: 2,
                bytes_reclaimed: 0,
            },
        })
        .await
        .expect("append cleanup log");

    let log = store
        .query(MemoryStoreQueryRequest::CleanupLog {
            scope: MemoryReadScope::tenant(tenant_a.clone()),
            limit: 10,
        })
        .await
        .expect("read tenant cleanup log");
    let MemoryStoreQueryResult::CleanupLog(log) = log else {
        panic!("expected cleanup log");
    };
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].cleanup_type, "contract-cap");

    let tenant_a_chunks = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant_a),
            selector: MemoryChunkSelector::project("shared-project"),
            limit: None,
        })
        .await
        .expect("read capped tenant chunks");
    let MemoryStoreReadResult::Chunks(tenant_a_chunks) = tenant_a_chunks else {
        panic!("expected project chunks");
    };
    assert_eq!(tenant_a_chunks.len(), 1);

    let tenant_b_chunks = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant_b.clone()),
            selector: MemoryChunkSelector::project("shared-project"),
            limit: None,
        })
        .await
        .expect("read other tenant chunks");
    let MemoryStoreReadResult::Chunks(tenant_b_chunks) = tenant_b_chunks else {
        panic!("expected project chunks");
    };
    assert_eq!(tenant_b_chunks.len(), 1);

    let other_log = store
        .query(MemoryStoreQueryRequest::CleanupLog {
            scope: MemoryReadScope::tenant(tenant_b),
            limit: 10,
        })
        .await
        .expect("read other tenant cleanup log");
    let MemoryStoreQueryResult::CleanupLog(other_log) = other_log else {
        panic!("expected cleanup log");
    };
    assert!(other_log.is_empty());
}

#[tokio::test]
async fn vacuum_and_backend_reset_safety_are_explicit_contract_operations() {
    let (database, _temp_dir) = test_store().await;
    let store: &dyn MemoryStore = &database;
    let tenant_scope = tenant("backend-recovery");
    create_context_node(
        store,
        tenant_scope.clone(),
        "memory://survives-unconfirmed-reset",
        None,
        NodeType::Directory,
    )
    .await;

    let vacuumed = store
        .mutate(MemoryStoreMutationRequest::Vacuum)
        .await
        .expect("vacuum backend");
    assert!(matches!(vacuumed, MemoryStoreMutationResult::Completed));

    let error = store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: false,
        })
        .await
        .expect_err("unconfirmed reset must be rejected");
    assert_eq!(error.kind, MemoryStoreErrorKind::InvalidRequest);

    let reset = store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("confirmed reset clears the backend");
    assert!(reset.changed);

    let after_reset = store
        .read(MemoryStoreReadRequest::ContextNode {
            scope: MemoryReadScope::tenant(tenant_scope),
            uri: "memory://survives-unconfirmed-reset".to_string(),
        })
        .await
        .expect("read backend after reset");
    assert!(matches!(
        after_reset,
        MemoryStoreReadResult::ContextNode(None)
    ));
}
