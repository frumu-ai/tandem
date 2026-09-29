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
