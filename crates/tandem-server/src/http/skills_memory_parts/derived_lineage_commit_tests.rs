// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tandem_memory::{MemoryCommitAuthority, MemoryStore, MemoryStoreErrorKind};
use tandem_types::{AuthorityChain, HumanActor, TenantContextAssertionClaims};
use tokio::sync::oneshot;

struct CommitFixture {
    state: AppState,
    directory: tempfile::TempDir,
    store: Arc<dyn MemoryStore>,
}

impl CommitFixture {
    async fn new() -> Self {
        Self::new_with_policy_remaining(None).await
    }

    async fn new_with_policy_remaining(remaining_ms: Option<u64>) -> Self {
        let state = crate::test_support::test_state().await;
        let directory = tempfile::tempdir().expect("policy directory");
        let path = directory.path().join("policy.json");
        let generated = remaining_ms.map_or_else(crate::now_ms, |remaining| {
            crate::now_ms() - tandem_enterprise_contract::hosted_policy::MAX_POLICY_AGE_MS + remaining
        });
        write_policy_at(&path, 4, generated);
        state.enterprise.hosted_policy.configure_test_source(
            "org-memory-commit", "dep-memory-commit", path,
        );
        state.reload_hosted_policy().await.expect("actual policy reload");
        let store = Arc::new(
            tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
                .await.expect("native memory database"),
        );
        Self { state, directory, store }
    }

    fn identity(&self, lifetime_ms: u64) -> VerifiedTenantContext {
        let now = crate::now_ms();
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web", "tandem-runtime", now, now + lifetime_ms,
            format!("memory-commit-{}", Uuid::new_v4()),
            TenantContext::explicit_user_workspace(
                "org-memory-commit", "ws-memory-commit",
                Some("dep-memory-commit".to_owned()), "alice",
            ),
            HumanActor::tandem_user("alice"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
            vec!["hosted:role:member".to_owned()],
        );
        claims.policy_version = Some(4);
        claims.capabilities = vec!["hosted.use".to_owned()];
        let mut verified = VerifiedTenantContext::from(claims);
        self.state.enterprise.hosted_policy.project(&mut verified)
            .expect("project original assertion").expect("installed hosted snapshot");
        verified
    }

    fn writer(&self) -> rusqlite::Connection {
        let writer = rusqlite::Connection::open(&self.state.memory_db_path)
            .expect("separate native SQLite connection");
        writer.execute_batch("BEGIN IMMEDIATE").expect("hold actual SQLite writer");
        writer
    }
}

fn write_policy(path: &std::path::Path, version: u64) {
    write_policy_at(path, version, crate::now_ms());
}

fn write_policy_at(path: &std::path::Path, version: u64, generated_at_ms: u64) {
    let bundle = json!({
        "schema_version": 1,
        "policy_version": version,
        "organization_id": "org-memory-commit",
        "deployment_id": "dep-memory-commit",
        "generated_at": chrono::DateTime::from_timestamp_millis(generated_at_ms as i64).unwrap(),
        "users": [{
            "id": "alice", "email": null, "username": null, "role": "member",
            "capabilities": ["hosted.use"], "is_active": true, "email_verified": true
        }],
        "org_units": [],
        "org_unit_memberships": [],
        "deployment_grants": []
    });
    std::fs::write(path, serde_json::to_vec(&bundle).unwrap()).expect("policy file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("private policy file");
    }
}

fn record(tenant: &TenantContext, id: &str) -> GlobalMemoryRecord {
    let now = crate::now_ms();
    let content = "A synthetic private memory fact for the native commit boundary.".to_owned();
    GlobalMemoryRecord {
        id: id.to_owned(), user_id: "alice".to_owned(), source_type: "fact".to_owned(),
        content_hash: hash_text(&content), content, run_id: format!("run-{id}"),
        session_id: None, message_id: None, tool_name: None,
        project_tag: Some("memory-commit-boundary".to_owned()), channel_tag: None, host_tag: None,
        metadata: Some(json!({"owner_subject":"alice", "classification":"internal"})),
        provenance: Some(json!({"tenant_context": tenant})),
        redaction_status: "passed".to_owned(), redaction_count: 0, visibility: "private".to_owned(),
        demoted: false, score_boost: 0.0, created_at_ms: now, updated_at_ms: now, expires_at_ms: None,
    }
}

fn observed_authority(
    state: &AppState, original: &VerifiedTenantContext,
) -> (MemoryCommitAuthority, Arc<AtomicUsize>, Arc<Mutex<Option<String>>>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let failure = Arc::new(Mutex::new(None));
    let current = derived_memory_commit_authority(state, &original.tenant_context, Some(original));
    let observed_calls = calls.clone();
    let observed_failure = failure.clone();
    let authority: MemoryCommitAuthority = Arc::new(move || {
        observed_calls.fetch_add(1, Ordering::SeqCst);
        let result = current();
        if let Err(error) = &result {
            assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
            *observed_failure.lock().unwrap() = Some(error.message.clone());
        }
        result
    });
    (authority, calls, failure)
}

async fn expire_while_writer_is_held(fixture: &CommitFixture, original: &VerifiedTenantContext) {
    assert!(!original.is_expired_at(crate::now_ms()), "original assertion admitted while current");
    tokio::time::timeout(Duration::from_secs(4), async {
        while !original.is_expired_at(crate::now_ms()) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("actual assertion expiry");
    assert_eq!(
        fixture.state.enterprise.hosted_policy.authorize(Some(original)),
        Err("hosted_policy_or_identity_expired"),
    );
    let renewed = fixture.identity(60_000);
    assert_ne!(renewed.assertion_id, original.assertion_id);
    fixture.state.enterprise.hosted_policy.authorize(Some(&renewed))
        .expect("current policy and a fresh assertion remain usable");
}

async fn started(receiver: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(3), receiver).await
        .expect("native writer future entered").expect("writer start witness");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_memory_insert_rechecks_original_identity_after_actual_sqlite_writer_wait() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let writer = fixture.writer();
        let original = fixture.identity(if expired { 1_500 } else { 60_000 });
        let tenant = original.tenant_context.clone();
        let row = record(&tenant, if expired { "expired-insert" } else { "current-insert" });
        let id = row.id.clone();
        let (authority, calls, failure) = observed_authority(&fixture.state, &original);
        let (entered, receiver) = oneshot::channel();
        let state = fixture.state.clone();
        let commit_state = state.clone();
        let verified = original.clone();
        let store = fixture.store.clone();
        let task = tokio::spawn(async move {
            commit_derived_memory_with_current_policy(&state, &tenant, Some(&verified), async move {
                entered.send(()).unwrap();
                persist_global_memory_record_with_commit_authority(
                    &commit_state, store.as_ref(), row, Some(authority),
                ).await
            }).await
        });
        started(receiver).await;
        assert!(!task.is_finished());
        assert!(fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "callback must wait for actual BEGIN IMMEDIATE");
        if expired { expire_while_writer_is_held(&fixture, &original).await; }
        writer.execute_batch("COMMIT").expect("release external SQLite writer");
        let write = tokio::time::timeout(Duration::from_secs(4), task).await.unwrap().unwrap()
            .expect("hosted side-effect task completed");
        assert_eq!(write.is_some(), !expired);
        let count: i64 = writer.query_row("SELECT COUNT(*) FROM memory_records WHERE id = ?1", [&id], |row| row.get(0)).unwrap();
        assert_eq!(count, i64::from(!expired), "denial must roll back the actual row");
        if expired {
            assert_eq!(failure.lock().unwrap().as_deref(), Some("hosted_policy_or_identity_expired"));
            assert!(calls.load(Ordering::SeqCst) >= 1);
        } else {
            assert!(failure.lock().unwrap().is_none());
            assert!(write.unwrap().stored);
            assert!(calls.load(Ordering::SeqCst) >= 2, "check after writer acquisition and before COMMIT");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_memory_insert_rechecks_snapshot_expiry_after_actual_sqlite_writer_wait() {
    // Keep the production TTL unchanged. The file represents an already-aged
    // snapshot that is still valid at writer admission and naturally expires
    // while the actual target writer remains locked.
    let fixture = CommitFixture::new_with_policy_remaining(Some(3_000)).await;
    let writer = fixture.writer();
    let original = fixture.identity(60_000);
    let tenant = original.tenant_context.clone();
    let expiry = fixture.state.enterprise.hosted_policy.current().unwrap().unwrap().expires_at_ms();
    let row = record(&tenant, "snapshot-expired-insert");
    let id = row.id.clone();
    let (authority, calls, failure) = observed_authority(&fixture.state, &original);
    let (entered, receiver) = oneshot::channel();
    let state = fixture.state.clone();
    let commit_state = state.clone();
    let verified = original.clone();
    let store = fixture.store.clone();
    let task = tokio::spawn(async move {
        commit_derived_memory_with_current_policy(&state, &tenant, Some(&verified), async move {
            entered.send(()).unwrap();
            persist_global_memory_record_with_commit_authority(
                &commit_state, store.as_ref(), row, Some(authority),
            ).await
        }).await
    });
    started(receiver).await;
    assert!(crate::now_ms() < expiry, "snapshot is still live at writer admission");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!task.is_finished());
    tokio::time::timeout(Duration::from_secs(4), async {
        while crate::now_ms() < expiry {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("actual default policy TTL expiry");
    assert!(!original.is_expired_at(crate::now_ms()), "assertion remains live");
    assert_eq!(fixture.state.enterprise.hosted_policy.authorize(Some(&original)),
        Err("hosted_policy_or_identity_expired"));
    writer.execute_batch("COMMIT").expect("release external writer after snapshot expiry");
    let result = tokio::time::timeout(Duration::from_secs(4), task).await.unwrap().unwrap()
        .expect("owned persistence finishes");
    assert!(result.is_none());
    assert_eq!(failure.lock().unwrap().as_deref(), Some("hosted_policy_or_identity_expired"));
    let count: i64 = writer.query_row("SELECT COUNT(*) FROM memory_records WHERE id = ?1", [&id], |row| row.get(0)).unwrap();
    assert_eq!(count, 0, "expired snapshot cannot leave an inserted row");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_memory_update_rechecks_original_identity_after_actual_sqlite_writer_wait() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let seed_identity = fixture.identity(60_000);
        let seed = record(&seed_identity.tenant_context, if expired { "expired-update" } else { "current-update" });
        let seeded = persist_global_memory_record(&fixture.state, fixture.store.as_ref(), seed.clone())
            .await.expect("existing canonical record");
        assert!(seeded.stored);
        let writer = fixture.writer();
        let original = fixture.identity(if expired { 1_500 } else { 60_000 });
        let tenant = original.tenant_context.clone();
        let (authority, calls, failure) = observed_authority(&fixture.state, &original);
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: tenant.org_id.clone(), workspace_id: tenant.workspace_id.clone(),
            deployment_id: tenant.deployment_id.clone(),
        });
        scope.subject = Some("alice".to_owned());
        let mutation = tandem_memory::MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope, id: seed.id.clone(), visibility: "shared".to_owned(), demoted: false,
            metadata: seed.metadata.clone(), provenance: seed.provenance.clone(),
        };
        let (entered, receiver) = oneshot::channel();
        let state = fixture.state.clone();
        let verified = original.clone();
        let store = fixture.store.clone();
        let task = tokio::spawn(async move {
            commit_derived_memory_with_current_policy(&state, &tenant, Some(&verified), async move {
                entered.send(()).unwrap();
                store.mutate_with_commit_authority(mutation, authority).await
            }).await
        });
        started(receiver).await;
        assert!(!task.is_finished());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "mutation callback follows writer acquisition");
        if expired { expire_while_writer_is_held(&fixture, &original).await; }
        writer.execute_batch("COMMIT").expect("release external SQLite writer");
        let result = tokio::time::timeout(Duration::from_secs(4), task).await.unwrap().unwrap()
            .expect("owned mutation finished");
        let visibility: String = writer.query_row("SELECT visibility FROM memory_records WHERE id = ?1", [&seed.id], |row| row.get(0)).unwrap();
        if expired {
            assert_eq!(result.unwrap_err().kind, MemoryStoreErrorKind::ScopeViolation);
            assert_eq!(failure.lock().unwrap().as_deref(), Some("hosted_policy_or_identity_expired"));
            assert_eq!(visibility, "private", "expired mutation retains the original row");
        } else {
            assert!(matches!(result.unwrap(), tandem_memory::MemoryStoreMutationResult::Changed(true)));
            assert_eq!(visibility, "shared");
            assert!(calls.load(Ordering::SeqCst) >= 2);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_cancelled_memory_wait_retains_policy_guard_until_native_commit() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let writer = fixture.writer();
        let original = fixture.identity(if expired { 1_500 } else { 60_000 });
        let tenant = original.tenant_context.clone();
        let row = record(&tenant, if expired { "cancelled-expired-insert" } else { "cancelled-current-insert" });
        let id = row.id.clone();
        let (authority, calls, failure) = observed_authority(&fixture.state, &original);
        let (entered, receiver) = oneshot::channel();
        let (native_done, completed) = oneshot::channel();
        let state = fixture.state.clone();
        let commit_state = state.clone();
        let verified = original.clone();
        let store = fixture.store.clone();
        let task = tokio::spawn(async move {
            commit_derived_memory_with_current_policy(&state, &tenant, Some(&verified), async move {
                entered.send(()).unwrap();
                let result = persist_global_memory_record_with_commit_authority(
                    &commit_state, store.as_ref(), row, Some(authority),
                ).await;
                native_done.send(result.is_some()).unwrap();
                result
            }).await
        });
        started(receiver).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        write_policy(&fixture.directory.path().join("policy.json"), 5);
        let reload_state = fixture.state.clone();
        let (reload_entered, reload_started) = oneshot::channel();
        let reload = tokio::spawn(async move {
            reload_entered.send(()).unwrap();
            reload_state.reload_hosted_policy().await
        });
        started(reload_started).await;
        assert!(!reload.is_finished(), "publication must wait for the detached native writer");
        if expired { expire_while_writer_is_held(&fixture, &original).await; }
        writer.execute_batch("COMMIT").expect("release actual native writer");
        let stored = tokio::time::timeout(Duration::from_secs(4), completed).await.unwrap().unwrap();
        assert_eq!(stored, !expired);
        tokio::time::timeout(Duration::from_secs(4), reload).await.unwrap().unwrap()
            .expect("publication resumes only after native completion");
        let count: i64 = writer.query_row("SELECT COUNT(*) FROM memory_records WHERE id = ?1", [&id], |row| row.get(0)).unwrap();
        assert_eq!(count, i64::from(!expired));
        assert_eq!(failure.lock().unwrap().is_some(), expired);
        assert!(!fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test());
    }
}

#[tokio::test]
async fn tan_829_memory_commit_preserves_standalone_without_skipping_configured_hosted_authority() {
    let state = crate::test_support::test_state().await;
    let standalone_tenant = TenantContext::local_implicit();
    let store = Arc::new(tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await.expect("standalone native memory database"));
    let authority = derived_memory_commit_authority(&state, &standalone_tenant, None);
    let row = record(&standalone_tenant, "standalone-current-insert");
    let commit_state = state.clone();
    let saved = commit_derived_memory_with_current_policy(&state, &standalone_tenant, None,
        async move {
            persist_global_memory_record_with_commit_authority(
                &commit_state, store.as_ref(), row, Some(authority),
            ).await
        },
    ).await.expect("unconfigured standalone authority").expect("standalone guarded insert");
    assert!(saved.stored);

    let hosted = CommitFixture::new().await;
    // A local tenant label cannot bypass a configured hosted policy or cause a
    // supplied original assertion to be ignored.
    let missing = derived_memory_commit_authority(&hosted.state, &standalone_tenant, None);
    assert_eq!(missing().unwrap_err().kind, MemoryStoreErrorKind::ScopeViolation);
    let called = Arc::new(AtomicUsize::new(0));
    let commit_called = called.clone();
    let denied = commit_derived_memory_with_current_policy(&hosted.state, &standalone_tenant, None,
        async move { commit_called.fetch_add(1, Ordering::SeqCst); },
    ).await;
    assert_eq!(denied, Err(StatusCode::FORBIDDEN));
    assert_eq!(called.load(Ordering::SeqCst), 0);
    let mut expired = hosted.identity(60_000);
    expired.expires_at_ms = crate::now_ms();
    let supplied = derived_memory_commit_authority(&hosted.state, &standalone_tenant, Some(&expired));
    assert_eq!(supplied().unwrap_err().message, "hosted_policy_or_identity_expired");
}
