use super::*;
use std::sync::{mpsc, Arc};
use std::time::Duration;

async fn hosted_storage() -> (tempfile::TempDir, Arc<Storage>, Session) {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::new(directory.path()).await.unwrap());
    let mut session = Session::new(Some("hosted plan".into()), Some("/tmp".into()));
    session.tenant_context = TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        Some("deployment-a".into()),
        "alice",
    );
    session.messages.push(Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "existing user message".into(),
        }],
    ));
    storage.save_session(session.clone()).await.unwrap();
    (directory, storage, session)
}

#[tokio::test]
async fn guarded_plan_storage_todos_publish_exact_normalized_persisted_values() {
    let (_directory, storage, session) = hosted_storage().await;
    let before = serde_json::to_value(storage.get_session(&session.id).await.unwrap()).unwrap();
    let (published_tx, published_rx) = tokio::sync::oneshot::channel();
    storage
        .set_todos_with_commit_guard(
            &session.id,
            vec![
                json!({"text":"  first item  ", "unused":"discard"}),
                json!({"id":"kept-id", "content":" second item ", "status":"in_progress"}),
                json!({"content":"   "}),
                json!("not an object"),
            ],
            move |canonical, commit| {
                let published = canonical.to_vec();
                commit()?;
                published_tx
                    .send(published)
                    .map_err(|_| anyhow::anyhow!("publication observer gone"))?;
                Ok(())
            },
        )
        .await
        .unwrap();
    let published = published_rx.await.unwrap();
    assert_eq!(published.len(), 2);
    assert_eq!(published[0]["content"], "first item");
    assert_eq!(published[0]["status"], "pending");
    let generated = published[0]["id"]
        .as_str()
        .unwrap()
        .strip_prefix("todo-")
        .unwrap();
    Uuid::parse_str(generated).expect("one generated canonical todo UUID");
    assert_eq!(published[0].as_object().unwrap().len(), 3);
    assert_eq!(
        published[1],
        json!({"id":"kept-id", "content":"second item", "status":"in_progress"})
    );
    assert_eq!(storage.get_todos(&session.id).await, published);
    assert_eq!(
        serde_json::to_value(storage.get_session(&session.id).await.unwrap()).unwrap(),
        before
    );
}

#[tokio::test]
async fn guarded_plan_storage_question_publishes_exact_prepared_tenant_digest_tool_and_ttl() {
    let (directory, storage, session) = hosted_storage().await;
    let questions = vec![json!({"question":"Choose next step?", "options":["A", "B"]})];
    let expected_questions = questions.clone();
    let earliest = now_ms_u64();
    let (published_tx, published_rx) = tokio::sync::oneshot::channel();
    let request = storage
        .add_question_request_with_commit_guard(
            &session.id,
            "message-plan",
            questions,
            move |canonical, commit| {
                let published = serde_json::to_value(canonical)?;
                commit()?;
                published_tx
                    .send(published)
                    .map_err(|_| anyhow::anyhow!("publication observer gone"))?;
                Ok(())
            },
        )
        .await
        .unwrap();
    let latest = now_ms_u64();
    let published = published_rx.await.unwrap();
    assert_eq!(published, serde_json::to_value(&request).unwrap());
    assert_eq!(request.tenant_context, session.tenant_context);
    assert_eq!(request.requested_by.as_deref(), Some("alice"));
    assert_eq!(request.session_id, session.id);
    assert_eq!(request.questions, expected_questions);
    Uuid::parse_str(request.id.strip_prefix("q-").unwrap()).unwrap();
    let tool = request.tool.as_ref().expect("prepared tool reference");
    assert_eq!(tool.message_id, "message-plan");
    Uuid::parse_str(tool.call_id.strip_prefix("call-").unwrap()).unwrap();
    let prepared_at = request
        .expires_at_ms
        .checked_sub(QUESTION_REQUEST_TTL_MS)
        .unwrap();
    assert!((earliest..=latest).contains(&prepared_at));
    let expected_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!({
                "tenant":&request.tenant_context, "sessionID":&request.session_id,
                "questions":&request.questions, "tool":&request.tool,
            }))
            .unwrap()
        )
    );
    assert_eq!(request.action_digest, expected_digest);
    let connection = rusqlite::Connection::open(directory.path().join("sessions.sqlite3")).unwrap();
    let raw: String = connection
        .query_row(
            "SELECT request_json FROM session_question_requests WHERE request_id = ?1",
            [&request.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), published);
    let checked = storage
        .get_question_request_for_tenant(&request.id, &session.tenant_context, Some(&session.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::to_value(checked).unwrap(), published);
}

#[tokio::test]
async fn guarded_plan_storage_denial_skipped_commit_and_empty_question_publish_nothing() {
    let (_directory, storage, session) = hosted_storage().await;
    for denied in [true, false] {
        let (published_tx, mut published_rx) = tokio::sync::oneshot::channel::<()>();
        let result = storage
            .add_question_request_with_commit_guard(
                &session.id,
                "message-plan",
                vec![json!({"question":"Sensitive?"})],
                move |_, _| {
                    // Captured publication must be disposed without being invoked.
                    let _publisher = published_tx;
                    if denied {
                        anyhow::bail!("revoked");
                    }
                    Ok(())
                },
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            published_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
        assert!(storage.repository.list_questions().unwrap().is_empty());
        assert!(storage.question_write_lock.try_lock().is_ok());
    }
    storage
        .add_question_request_with_commit_guard(&session.id, "message-plan", vec![], |_, _| {
            panic!("empty request must not reach native callback")
        })
        .await
        .expect_err("empty question request rejected before native work");
    assert!(storage.repository.list_questions().unwrap().is_empty());
}

#[tokio::test]
async fn guarded_plan_storage_cancellation_retains_question_writer_through_commit_and_callback() {
    let (_directory, storage, session) = hosted_storage().await;
    let writer_storage = storage.clone();
    let session_id = session.id.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (commit_tx, commit_rx) = mpsc::channel();
    let (committed_tx, committed_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let awaiting = tokio::spawn(async move {
        writer_storage
            .add_question_request_with_commit_guard(
                &session_id,
                "message-plan",
                vec![json!({"question":"Native write?"})],
                move |canonical, commit| {
                    let _ = entered_tx.send(canonical.id.clone());
                    commit_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release SQL commit");
                    commit()?;
                    let _ = committed_tx.send(());
                    finish_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release native callback");
                    Ok(())
                },
            )
            .await
    });
    let request_id = tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    awaiting.abort();
    assert!(awaiting.await.unwrap_err().is_cancelled());
    assert!(
        storage.question_write_lock.try_lock().is_err(),
        "cancelled awaiter must not release native-owned question mutex"
    );
    assert!(
        storage.repository.list_questions().unwrap().is_empty(),
        "native transaction has not committed"
    );
    let decision = storage.reply_question(&request_id);
    tokio::pin!(decision);
    assert!(
        futures::poll!(decision.as_mut()).is_pending(),
        "real question decision waits for native-owned mutex"
    );
    commit_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), committed_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        storage.repository.list_questions().unwrap()[0].id,
        request_id
    );
    assert!(
        storage.question_write_lock.try_lock().is_err(),
        "question serialization covers post-commit publication callback"
    );
    assert!(futures::poll!(decision.as_mut()).is_pending());
    finish_tx.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(5), decision)
        .await
        .unwrap()
        .unwrap());
    assert!(storage.question_write_lock.try_lock().is_ok());
    assert!(storage.repository.list_questions().unwrap().is_empty());
}

#[tokio::test]
async fn guarded_plan_storage_cancelled_denial_rolls_back_before_question_writer_release() {
    let (_directory, storage, session) = hosted_storage().await;
    let writer_storage = storage.clone();
    let session_id = session.id.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (deny_tx, deny_rx) = mpsc::channel();
    let awaiting = tokio::spawn(async move {
        writer_storage
            .add_question_request_with_commit_guard(
                &session_id,
                "message-plan",
                vec![json!({"question":"Denied native write?"})],
                move |_, _| {
                    let _ = entered_tx.send(());
                    deny_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release native denial");
                    anyhow::bail!("revoked")
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    awaiting.abort();
    assert!(awaiting.await.unwrap_err().is_cancelled());
    assert!(storage.question_write_lock.try_lock().is_err());
    deny_tx.send(()).unwrap();
    let released = tokio::time::timeout(Duration::from_secs(5), storage.question_write_lock.lock())
        .await
        .unwrap();
    assert!(storage.repository.list_questions().unwrap().is_empty());
    // After acquiring the serialized question writer, an independent SQLite
    // writer must succeed: rollback precedes release of the question mutex.
    let connection = rusqlite::Connection::open(storage.base.join("sessions.sqlite3")).unwrap();
    connection
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK")
        .unwrap();
    drop(released);
}
