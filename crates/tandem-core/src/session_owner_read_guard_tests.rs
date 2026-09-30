use super::super::Storage;
use super::*;
use tandem_types::Session;

fn tenant(actor: &str) -> TenantContext {
    TenantContext::explicit_user_workspace("org-a", "workspace-a", Some("dep-a".into()), actor)
}

#[tokio::test]
async fn session_owner_read_guard_serializes_independent_storage_ownership_transfer() {
    fn assert_send<T: Send>() {}
    assert_send::<SessionOwnerReadGuard>();
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::new(directory.path()).await.unwrap();
    let independent = Storage::new(directory.path()).await.unwrap();
    let mut session = Session::new(Some("owner guard".into()), None);
    session.tenant_context = tenant("alice");
    let session_id = session.id.clone();
    storage.save_session(session.clone()).await.unwrap();
    let owners = storage
        .session_owner_read_guard(vec![session_id.clone()])
        .await
        .unwrap();
    assert_eq!(owners.tenant_context(&session_id), Some(&tenant("alice")));
    session.tenant_context = tenant("bob");
    let (committed_tx, mut committed_rx) = tokio::sync::oneshot::channel();
    let writer = independent.save_session_with_commit_guard(session, move |commit| {
        let result = commit();
        let _ = committed_tx.send(());
        result
    });
    tokio::pin!(writer);
    assert!(futures::poll!(writer.as_mut()).is_pending());
    assert!(
        committed_rx.try_recv().is_err(),
        "independent writer cannot acquire SQLite while the owner guard is held"
    );
    assert_eq!(
        storage
            .get_session(&session_id)
            .await
            .unwrap()
            .tenant_context,
        tenant("alice")
    );
    drop(owners);
    tokio::time::timeout(std::time::Duration::from_secs(5), writer)
        .await
        .unwrap()
        .unwrap();
    committed_rx.await.unwrap();
    let current = storage
        .session_owner_read_guard(vec![session_id.clone()])
        .await
        .unwrap();
    assert_eq!(current.tenant_context(&session_id), Some(&tenant("bob")));
}

#[tokio::test]
async fn session_owner_read_guard_missing_malformed_and_misbound_headers_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::new(directory.path()).await.unwrap();
    let mut session = Session::new(Some("owner guard".into()), None);
    session.tenant_context = tenant("alice");
    let session_id = session.id.clone();
    storage.save_session(session).await.unwrap();
    for header in [
        "{".to_owned(),
        r#"{"id":"another-session","tenant_context":{}}"#.to_owned(),
        serde_json::json!({"id": session_id, "tenant_context": tenant("alice")}).to_string(),
    ] {
        let connection = Connection::open(directory.path().join("sessions.sqlite3")).unwrap();
        connection
            .execute(
                "UPDATE session_records SET session_json = ?1 WHERE session_id = ?2",
                [header.as_str(), session_id.as_str()],
            )
            .unwrap();
        let owners = storage
            .session_owner_read_guard(vec![session_id.clone(), "missing".into()])
            .await
            .unwrap();
        assert!(owners.tenant_context(&session_id).is_none());
        assert!(owners.tenant_context("missing").is_none());
    }
}

#[tokio::test]
async fn session_owner_read_guard_empty_batch_does_not_wait_on_sqlite_writer() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::new(directory.path()).await.unwrap();
    let blocker = Connection::open(directory.path().join("sessions.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let empty = storage.session_owner_read_guard(Vec::new());
    tokio::pin!(empty);
    assert!(
        futures::poll!(empty.as_mut()).is_ready(),
        "non-session frames must open no SQLite transaction"
    );
    blocker.execute_batch("ROLLBACK").unwrap();
}
