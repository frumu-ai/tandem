use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::Duration;

fn hold_sqlite_writer(database_path: &Path) -> Connection {
    let connection = Connection::open(database_path).expect("open competing writer");
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold SQLite writer lock");
    connection
}

#[test]
fn guarded_session_save_denies_revocation_while_sqlite_writer_is_held() {
    let directory = tempfile::tempdir().expect("session store directory");
    let repository = SessionRepository::open(directory.path()).expect("session repository");
    let blocker = hold_sqlite_writer(&directory.path().join("sessions.sqlite3"));
    let session = Session::new(Some("planner".to_string()), Some("/tmp".to_string()));
    let session_id = session.id.clone();
    let authority = Arc::new(AtomicBool::new(true));
    let worker_authority = authority.clone();
    let worker_repository = repository.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (guard_tx, guard_rx) = mpsc::channel();
    let pending = thread::spawn(move || {
        started_tx.send(()).expect("signal save attempt");
        worker_repository.save_session_with_commit_guard(&session, |commit| {
            guard_tx.send(()).expect("signal transaction acquired");
            anyhow::ensure!(
                worker_authority.load(Ordering::SeqCst),
                "planner write revoked"
            );
            commit()
        })
    });

    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("save worker started");
    assert!(
        guard_rx.try_recv().is_err(),
        "guard cannot run behind writer lock"
    );
    authority.store(false, Ordering::SeqCst);
    blocker
        .execute_batch("COMMIT")
        .expect("release writer lock");
    guard_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("guard ran after writer release");
    assert!(pending.join().expect("save worker").is_err());
    assert!(repository.get_session(&session_id).unwrap().is_none());
}

#[test]
fn guarded_message_append_denies_revocation_while_sqlite_writer_is_held() {
    let directory = tempfile::tempdir().expect("session store directory");
    let repository = SessionRepository::open(directory.path()).expect("session repository");
    let session = Session::new(Some("planner".to_string()), Some("/tmp".to_string()));
    let session_id = session.id.clone();
    repository.save_session(&session).expect("seed session");
    let blocker = hold_sqlite_writer(&directory.path().join("sessions.sqlite3"));
    let authority = Arc::new(AtomicBool::new(true));
    let worker_authority = authority.clone();
    let worker_repository = repository.clone();
    let worker_session_id = session_id.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (guard_tx, guard_rx) = mpsc::channel();
    let pending = thread::spawn(move || {
        let message = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "plan a workflow".to_string(),
            }],
        );
        started_tx.send(()).expect("signal append attempt");
        worker_repository.append_message_with_commit_guard(&worker_session_id, &message, |commit| {
            guard_tx.send(()).expect("signal transaction acquired");
            anyhow::ensure!(
                worker_authority.load(Ordering::SeqCst),
                "planner write revoked"
            );
            commit()
        })
    });

    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("append worker started");
    assert!(
        guard_rx.try_recv().is_err(),
        "guard cannot run behind writer lock"
    );
    authority.store(false, Ordering::SeqCst);
    blocker
        .execute_batch("COMMIT")
        .expect("release writer lock");
    guard_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("guard ran after writer release");
    assert!(pending.join().expect("append worker").is_err());
    assert!(repository
        .get_session(&session_id)
        .unwrap()
        .unwrap()
        .messages
        .is_empty());
}

#[test]
fn guarded_message_batch_rolls_back_when_second_insert_fails() {
    let directory = tempfile::tempdir().expect("session store directory");
    let repository = SessionRepository::open(directory.path()).expect("session repository");
    let session = Session::new(Some("direct KB".to_string()), Some("/tmp".to_string()));
    let session_id = session.id.clone();
    repository.save_session(&session).expect("seed session");
    let existing = Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "earlier message".to_string(),
        }],
    );
    repository
        .append_message(&session_id, &existing)
        .expect("seed earlier message");

    let connection =
        Connection::open(directory.path().join("sessions.sqlite3")).expect("open session store");
    connection
        .execute_batch(
            "CREATE TRIGGER reject_assistant BEFORE INSERT ON session_messages
             WHEN NEW.role = 'assistant'
             BEGIN SELECT RAISE(ABORT, 'reject assistant'); END;",
        )
        .expect("reject the second message in a batch");
    let user = Message::new(
        MessageRole::User,
        vec![MessagePart::ToolInvocation {
            tool: "mcp.kb.answer_question".to_string(),
            args: serde_json::json!({"question": "question"}),
            result: Some(serde_json::json!("sensitive retrieved excerpt")),
            error: None,
        }],
    );
    let assistant = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::Text {
            text: "ordinary answer".to_string(),
        }],
    );
    let mut guard_calls = 0;
    let result = repository.append_messages_with_commit_guard(
        &session_id,
        &[user.clone(), assistant.clone()],
        |commit| {
            guard_calls += 1;
            commit()
        },
    );
    assert!(result.is_err(), "second insert must fail");
    assert_eq!(guard_calls, 1, "one authority decision covers the batch");
    let persisted = repository.get_session(&session_id).unwrap().unwrap();
    assert_eq!(persisted.messages.len(), 1);
    assert_eq!(persisted.messages[0].id, existing.id);

    connection
        .execute_batch("DROP TRIGGER reject_assistant")
        .expect("allow assistant message");
    repository
        .append_messages_with_commit_guard(
            &session_id,
            &[user.clone(), assistant.clone()],
            |commit| commit(),
        )
        .expect("append complete batch");
    let persisted = repository.get_session(&session_id).unwrap().unwrap();
    assert_eq!(persisted.messages.len(), 3);
    assert_eq!(persisted.messages[1].id, user.id);
    assert_eq!(persisted.messages[2].id, assistant.id);
}
