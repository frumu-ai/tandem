use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn two_rules() -> Vec<(String, String, PermissionAction)> {
    vec![
        ("read".into(), "*".into(), PermissionAction::Allow),
        ("write".into(), "*".into(), PermissionAction::Deny),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_rule_batch_checks_authority_after_staging_and_cleans_denied_temporary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("permissions.json");
    let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
        .await
        .unwrap();
    let before_rules = serde_json::to_value(manager.list_rules().await).unwrap();
    let before_file = std::fs::read(&path).unwrap();
    let check_directory = directory.path().to_path_buf();
    let check_path = path.clone();
    let check_before = before_file.clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    assert!(std::time::Instant::now() < deadline);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let guard_calls = calls.clone();
    let worker_manager = manager.clone();
    let worker = tokio::spawn(async move {
        worker_manager
            .add_rules_for_session_with_staging_hook(
                &TenantContext::local_implicit(),
                "session",
                two_rules(),
                move |_| {
                    guard_calls.fetch_add(1, Ordering::SeqCst);
                    let staged = std::fs::read_dir(&check_directory)
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .find(|entry| {
                            entry
                                .extension()
                                .is_some_and(|extension| extension == "tmp")
                        })
                        .expect("file staging must finish before the authority guard");
                    let prepared: PermissionStateFile =
                        serde_json::from_slice(&std::fs::read(staged).unwrap()).unwrap();
                    assert_eq!(prepared.rules.len(), 2);
                    assert_eq!(std::fs::read(&check_path).unwrap(), check_before);
                    assert!(std::time::Instant::now() >= deadline);
                    anyhow::bail!("authority expired during staging")
                },
                move || {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "authority must not be checked before staging finishes"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before_file);
    tokio::time::sleep(deadline.saturating_duration_since(std::time::Instant::now())).await;
    release_tx.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("authority expired"));
    assert_eq!(
        serde_json::to_value(manager.list_rules().await).unwrap(),
        before_rules
    );
    assert_eq!(std::fs::read(&path).unwrap(), before_file);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn permission_rule_batch_revalidates_after_both_lock_waits() {
    for wait_on_rules in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("permissions.json");
        let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
            .await
            .unwrap();
        let tenant = TenantContext::local_implicit();
        manager
            .add_rule_for_session(&tenant, "existing", "glob", "*", PermissionAction::Allow)
            .await;
        let before_rules = serde_json::to_value(manager.list_rules().await).unwrap();
        let before_file = tokio::fs::read(&path).await.unwrap();
        let transaction_lock = if wait_on_rules {
            None
        } else {
            Some(manager.state_write_lock.lock().await)
        };
        let rules_lock = if wait_on_rules {
            Some(manager.rules.read().await)
        } else {
            None
        };
        let allowed = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let commit_allowed = allowed.clone();
        let commit_calls = calls.clone();
        let insertion = manager.add_rules_for_session_with_commit_guard(
            &tenant,
            "new-session",
            two_rules(),
            move |commit| {
                commit_calls.fetch_add(1, Ordering::SeqCst);
                anyhow::ensure!(commit_allowed.load(Ordering::SeqCst), "revoked batch");
                commit()
            },
        );
        tokio::pin!(insertion);
        let first = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(insertion.as_mut(), cx))
        })
        .await;
        assert!(first.is_pending());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        allowed.store(false, Ordering::SeqCst);
        drop(rules_lock);
        drop(transaction_lock);
        assert!(insertion
            .await
            .unwrap_err()
            .to_string()
            .contains("revoked batch"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            serde_json::to_value(manager.list_rules().await).unwrap(),
            before_rules
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before_file);
        let reloaded = PermissionManager::new_with_state_file(EventBus::new(), path)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(reloaded.list_rules().await).unwrap(),
            before_rules
        );
    }
}

#[tokio::test]
async fn permission_rule_batch_commits_once_preserves_state_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("permissions.json");
    let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
        .await
        .unwrap();
    let tenant = TenantContext::local_implicit();
    let other_tenant =
        TenantContext::explicit("other-org", "other-workspace", Some("alice".into()));
    manager
        .add_rule_for_session(
            &other_tenant,
            "session",
            "read",
            "*",
            PermissionAction::Allow,
        )
        .await;
    manager
        .add_rule_for_session(
            &tenant,
            "other-session",
            "read",
            "*",
            PermissionAction::Deny,
        )
        .await;
    manager
        .add_rule_for_session(&tenant, "session", "read", "*", PermissionAction::Ask)
        .await;
    let request = manager
        .ask_for_session_for_tenant(&other_tenant, Some("other-session"), "glob", json!({}))
        .await;
    manager
        .reply_with_provenance_for_tenant(
            &other_tenant,
            Some("other-session"),
            &request.id,
            "once",
            Some("alice".into()),
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let before: PermissionStateFile =
        serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let commit_calls = calls.clone();
    let mut inputs = two_rules();
    inputs.push(("read".into(), "*".into(), PermissionAction::Allow));
    inputs.push(("read".into(), "*".into(), PermissionAction::Ask));
    manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", inputs, move |commit| {
            commit_calls.fetch_add(1, Ordering::SeqCst);
            commit()
        })
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let rules = manager.list_rules().await;
    assert_eq!(rules.len(), before.rules.len() + 2);
    assert_eq!(rules[before.rules.len()].permission, "read");
    assert_eq!(rules[before.rules.len() + 1].permission, "write");
    for (old, preserved) in before.rules.iter().zip(&rules) {
        assert_eq!(
            serde_json::to_value(old).unwrap(),
            serde_json::to_value(preserved).unwrap()
        );
    }
    assert!(matches!(
        manager
            .evaluate_for_tenant_and_session(&tenant, Some("session"), "read", "file")
            .await,
        PermissionAction::Allow
    ));
    assert!(matches!(
        manager
            .evaluate_for_tenant_and_session(&tenant, Some("session"), "write", "file")
            .await,
        PermissionAction::Deny
    ));
    assert!(matches!(
        manager
            .evaluate_for_tenant_and_session(&tenant, Some("other-session"), "read", "file")
            .await,
        PermissionAction::Deny
    ));
    let after: PermissionStateFile =
        serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&after.requests).unwrap(),
        serde_json::to_value(&before.requests).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&after.decisions).unwrap(),
        serde_json::to_value(&before.decisions).unwrap()
    );
    let reloaded = PermissionManager::new_with_state_file(EventBus::new(), path)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(reloaded.list_rules().await).unwrap(),
        serde_json::to_value(rules).unwrap()
    );
}

#[tokio::test]
async fn permission_rule_batch_persistence_failure_keeps_live_and_disk_baselines() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("permissions.json");
    let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
        .await
        .unwrap();
    let tenant = TenantContext::local_implicit();
    manager.add_rule("glob", "*", PermissionAction::Allow).await;
    let before_rules = serde_json::to_value(manager.list_rules().await).unwrap();
    let before_file = tokio::fs::read(&path).await.unwrap();
    let blocked = directory.path().join("blocked.json");
    tokio::fs::create_dir(&blocked).await.unwrap();
    tokio::fs::write(blocked.join("existing"), b"must remain")
        .await
        .unwrap();
    *manager.state_path.write().await = Some(blocked.clone());
    let error = manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |commit| commit())
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("failed to replace permission state file"));
    assert_eq!(
        serde_json::to_value(manager.list_rules().await).unwrap(),
        before_rules
    );
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before_file);
    assert_eq!(
        tokio::fs::read(blocked.join("existing")).await.unwrap(),
        b"must remain"
    );
    assert_eq!(
        std::fs::read_dir(directory.path()).unwrap().count(),
        2,
        "owned temporary file must be removed"
    );
    manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |commit| {
            assert!(commit().is_err());
            commit()
        })
        .await
        .expect_err("a failed commit cannot be retried by consuming an empty staged result");
    assert_eq!(
        serde_json::to_value(manager.list_rules().await).unwrap(),
        before_rules
    );
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before_file);
    *manager.state_path.write().await = Some(path.clone());
    manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |commit| commit())
        .await
        .unwrap();
    assert_eq!(manager.list_rules().await.len(), 3);
    let reloaded = PermissionManager::new_with_state_file(EventBus::new(), path)
        .await
        .unwrap();
    assert_eq!(reloaded.list_rules().await.len(), 3);
}

#[tokio::test]
async fn permission_rule_batch_empty_skips_guard_and_duplicate_still_checks_authority() {
    let manager = PermissionManager::new(EventBus::new());
    let tenant = TenantContext::local_implicit();
    manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", Vec::new(), |_| {
            panic!("empty batches must not require authority")
        })
        .await
        .unwrap();
    manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |commit| commit())
        .await
        .unwrap();
    let before = serde_json::to_value(manager.list_rules().await).unwrap();
    assert!(manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |_| {
            anyhow::bail!("duplicate batch revoked")
        })
        .await
        .is_err());
    manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |commit| commit())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(manager.list_rules().await).unwrap(),
        before
    );
    assert!(manager
        .add_rules_for_session_with_commit_guard(&tenant, "session", two_rules(), |_| Ok(()))
        .await
        .unwrap_err()
        .to_string()
        .contains("skipped batch commit"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_rule_batch_cancelled_caller_cannot_unlock_pending_commit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("permissions.json");
    let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
        .await
        .unwrap();
    let before_file = tokio::fs::read(&path).await.unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_manager = manager.clone();
    let worker = tokio::spawn(async move {
        worker_manager
            .add_rules_for_session_with_commit_guard(
                &TenantContext::local_implicit(),
                "session",
                two_rules(),
                move |commit| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(std::time::Duration::from_secs(5))?;
                    commit()
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(
        manager.rules.try_read().is_err(),
        "uncommitted rules must not be observable"
    );
    assert!(
        manager.state_write_lock.try_lock().is_err(),
        "worker retains transaction lock"
    );
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before_file);
    release_tx.send(()).unwrap();
    drop(
        tokio::time::timeout(Duration::from_secs(5), manager.state_write_lock.lock())
            .await
            .unwrap(),
    );
    assert_eq!(manager.list_rules().await.len(), 2);
    let reloaded = PermissionManager::new_with_state_file(EventBus::new(), path)
        .await
        .unwrap();
    assert_eq!(reloaded.list_rules().await.len(), 2);
}
