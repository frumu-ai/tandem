use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[tokio::test]
async fn checked_session_rule_revalidates_after_each_lock_wait() {
    for wait_on_rules in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("permissions.json");
        let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
            .await
            .unwrap();
        let saved = tokio::fs::read(&path).await.unwrap();
        let tenant = TenantContext::local_implicit();
        let held_transaction = if wait_on_rules {
            None
        } else {
            Some(manager.state_write_lock.lock().await)
        };
        let held_rules = if wait_on_rules {
            Some(manager.rules.read().await)
        } else {
            None
        };
        let allowed = AtomicBool::new(true);
        let calls = AtomicUsize::new(0);
        let insertion = manager.add_rule_for_session_checked(
            &tenant,
            "session-test",
            "read",
            "*",
            PermissionAction::Allow,
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                if allowed.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err("revoked")
                }
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
        drop(held_rules);
        drop(held_transaction);
        assert_eq!(insertion.await.unwrap_err(), "revoked");
        assert!(manager.list_rules().await.is_empty());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), saved);
        manager
            .add_rule_for_session_checked(
                &tenant,
                "session-test",
                "read",
                "*",
                PermissionAction::Allow,
                || Ok::<(), &str>(()),
            )
            .await
            .unwrap();
        assert_eq!(manager.list_rules().await.len(), 1);
        let reloaded = PermissionManager::new_with_state_file(EventBus::new(), path)
            .await
            .unwrap();
        assert_eq!(reloaded.list_rules().await.len(), 1);
    }
}
