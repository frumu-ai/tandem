use super::super::QuestionToolRef;
use super::*;
use tandem_types::TenantContext;

fn seeded_repository() -> (tempfile::TempDir, SessionRepository, Session) {
    let directory = tempfile::tempdir().expect("session store directory");
    let repository = SessionRepository::open(directory.path()).expect("session repository");
    let mut session = Session::new(Some("original header".into()), Some("/tmp".into()));
    session.messages.push(Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "existing message".into(),
        }],
    ));
    repository.save_session(&session).expect("seed session");
    repository
        .update_metadata(&session.id, |metadata| {
            metadata.parent_id = Some("parent-session".into());
            metadata.archived = true;
            metadata.shared = true;
            metadata.share_id = Some("existing-share".into());
            metadata.summary = Some("existing summary".into());
            metadata.todos = vec![json!({"id":"old", "content":"existing", "status":"done"})];
        })
        .expect("seed metadata");
    let connection = Connection::open(directory.path().join("sessions.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER forbid_plan_header_update BEFORE UPDATE ON session_records
         BEGIN SELECT RAISE(ABORT, 'plan write replaced session header'); END;
         CREATE TRIGGER forbid_plan_header_delete BEFORE DELETE ON session_records
         BEGIN SELECT RAISE(ABORT, 'plan write removed session header'); END;
         CREATE TRIGGER forbid_plan_message_delete BEFORE DELETE ON session_messages
         BEGIN SELECT RAISE(ABORT, 'plan write replaced session messages'); END;
         CREATE TRIGGER forbid_plan_part_delete BEFORE DELETE ON session_message_parts
         BEGIN SELECT RAISE(ABORT, 'plan write replaced message parts'); END;",
        )
        .unwrap();
    (directory, repository, session)
}

fn question(session_id: &str) -> QuestionRequest {
    QuestionRequest {
        id: "q-existing".into(),
        tenant_context: TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".into()),
            "alice",
        ),
        requested_by: Some("alice".into()),
        action_digest: "repository preserves this exact prepared binding".into(),
        expires_at_ms: u64::MAX,
        session_id: session_id.into(),
        questions: vec![json!({"question":"Existing question?"})],
        tool: Some(QuestionToolRef {
            call_id: "call-existing".into(),
            message_id: "message-existing".into(),
        }),
    }
}

fn unchanged_session_rows(repository: &SessionRepository, session_id: &str) -> Value {
    repository.with_connection(|connection| {
        let header: (i64, String) = connection.query_row(
            "SELECT rowid, session_json FROM session_records WHERE session_id = ?1",
            [session_id], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut messages = connection.prepare(
            "SELECT rowid, ordinal, message_json FROM session_messages WHERE session_id = ?1 ORDER BY ordinal",
        )?;
        let messages = messages.query_map([session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut parts = connection.prepare(
            "SELECT rowid, message_ordinal, ordinal, part_json FROM session_message_parts WHERE session_id = ?1 ORDER BY message_ordinal, ordinal",
        )?;
        let parts = parts.query_map([session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"header":header, "messages":messages, "parts":parts}))
    }).unwrap()
}

fn metadata(repository: &SessionRepository, session_id: &str) -> Value {
    repository
        .with_connection(|connection| {
            Ok(serde_json::to_value(load_metadata(
                connection, session_id,
            )?)?)
        })
        .unwrap()
}

#[test]
fn guarded_plan_repository_denial_and_skipped_commit_preserve_existing_rows() {
    let (_directory, repository, session) = seeded_repository();
    let old_question = question(&session.id);
    repository.add_question(&old_question).unwrap();
    let rows = unchanged_session_rows(&repository, &session.id);
    let old_metadata = metadata(&repository, &session.id);
    let old_questions = serde_json::to_value(repository.list_questions().unwrap()).unwrap();
    for denied in [true, false] {
        let todo_error = repository
            .set_todos_with_commit_guard(
                &session.id,
                vec![json!({"id":"new", "content":"sensitive plan", "status":"pending"})],
                |_, _| {
                    if denied {
                        anyhow::bail!("revoked at native writer boundary");
                    }
                    Ok(())
                },
            )
            .expect_err("denied or skipped todo continuation");
        assert!(todo_error
            .to_string()
            .contains(if denied { "revoked" } else { "skipped commit" }));
        let mut replacement = old_question.clone();
        replacement.questions = vec![json!({"question":"Sensitive replacement?"})];
        let question_error = repository
            .add_question_with_commit_guard(&replacement, |_, _| {
                if denied {
                    anyhow::bail!("revoked at native writer boundary");
                }
                Ok(())
            })
            .expect_err("denied or skipped question continuation");
        assert!(question_error.to_string().contains(if denied {
            "revoked"
        } else {
            "skipped commit"
        }));
        assert_eq!(metadata(&repository, &session.id), old_metadata);
        assert_eq!(
            serde_json::to_value(repository.list_questions().unwrap()).unwrap(),
            old_questions
        );
        assert_eq!(unchanged_session_rows(&repository, &session.id), rows);
    }
}

#[test]
fn guarded_plan_repository_sql_failure_rolls_back_without_success_publication() {
    let (directory, repository, session) = seeded_repository();
    let old_question = question(&session.id);
    repository.add_question(&old_question).unwrap();
    let rows = unchanged_session_rows(&repository, &session.id);
    let old_metadata = metadata(&repository, &session.id);
    let old_questions = serde_json::to_value(repository.list_questions().unwrap()).unwrap();
    let connection = Connection::open(directory.path().join("sessions.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_plan_todos BEFORE UPDATE ON session_metadata
         BEGIN SELECT RAISE(ABORT, 'reject plan todos'); END;
         CREATE TRIGGER reject_plan_question BEFORE UPDATE ON session_question_requests
         BEGIN SELECT RAISE(ABORT, 'reject plan question'); END;",
        )
        .unwrap();
    let mut publications = 0;
    let todo_result = repository.set_todos_with_commit_guard(
        &session.id,
        vec![json!({"id":"new", "content":"new", "status":"pending"})],
        |_, commit| {
            commit()?;
            publications += 1;
            Ok(())
        },
    );
    let mut replacement = old_question;
    replacement.questions = vec![json!({"question":"Replacement?"})];
    let question_result = repository.add_question_with_commit_guard(&replacement, |_, commit| {
        commit()?;
        publications += 1;
        Ok(())
    });
    assert!(todo_result.is_err());
    assert!(question_result.is_err());
    assert_eq!(publications, 0);
    assert_eq!(metadata(&repository, &session.id), old_metadata);
    assert_eq!(
        serde_json::to_value(repository.list_questions().unwrap()).unwrap(),
        old_questions
    );
    assert_eq!(unchanged_session_rows(&repository, &session.id), rows);
    connection
        .execute_batch("DROP TRIGGER reject_plan_todos; DROP TRIGGER reject_plan_question;")
        .unwrap();
    repository
        .set_todos_with_commit_guard(&session.id, vec![], |_, commit| commit())
        .expect("failed transaction released its writer");
}

#[test]
fn guarded_plan_repository_double_continuation_reports_committed_first_result() {
    let (_directory, repository, session) = seeded_repository();
    let rows = unchanged_session_rows(&repository, &session.id);
    let todos = vec![json!({"id":"one", "content":"one", "status":"pending"})];
    let error = repository
        .set_todos_with_commit_guard(&session.id, todos.clone(), |_, commit| {
            commit()?;
            commit()
        })
        .expect_err("second continuation cannot reuse consumed transaction");
    assert!(error.to_string().contains("after metadata commit"));
    assert!(format!("{error:#}").contains("already consumed"));
    assert_eq!(repository.get_todos(&session.id).unwrap(), todos);
    let request = question(&session.id);
    let error = repository
        .add_question_with_commit_guard(&request, |_, commit| {
            commit()?;
            commit()
        })
        .expect_err("second question continuation cannot reuse consumed transaction");
    assert!(error.to_string().contains("after request commit"));
    assert!(format!("{error:#}").contains("already consumed"));
    assert_eq!(
        serde_json::to_value(repository.list_questions().unwrap()).unwrap(),
        json!([request])
    );
    assert_eq!(unchanged_session_rows(&repository, &session.id), rows);
}

#[test]
fn guarded_todo_repository_loads_latest_metadata_after_sqlite_writer_release() {
    let (directory, repository, session) = seeded_repository();
    let rows = unchanged_session_rows(&repository, &session.id);
    let mut blocker = Connection::open(directory.path().join("sessions.sqlite3")).unwrap();
    let transaction = blocker
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let mut latest = load_metadata(&transaction, &session.id).unwrap();
    latest.summary = Some("concurrent summary".into());
    latest.share_id = Some("concurrent share".into());
    upsert_metadata(&transaction, &session.id, &latest).unwrap();
    let todos = vec![json!({"id":"one", "content":"native write", "status":"pending"})];
    let expected = todos.clone();
    let worker_repository = repository.clone();
    let session_id = session.id.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (guard_tx, guard_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        worker_repository.set_todos_with_commit_guard(&session_id, todos, |canonical, commit| {
            guard_tx.send(canonical.to_vec()).unwrap();
            commit()
        })
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        guard_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    transaction.commit().unwrap();
    assert_eq!(
        guard_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        expected
    );
    worker.join().unwrap().unwrap();
    latest.todos = expected;
    assert_eq!(
        metadata(&repository, &session.id),
        serde_json::to_value(latest).unwrap()
    );
    assert_eq!(unchanged_session_rows(&repository, &session.id), rows);
}
