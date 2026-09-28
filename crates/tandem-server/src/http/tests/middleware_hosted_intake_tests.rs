// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

async fn intake_fixture() -> (AppState, tempfile::TempDir, ed25519_dalek::SigningKey) {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[67; 32]);
    let raw = json!({"key-a": {"purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
        "organization_id": "org-a", "deployment_id": "dep-a", "allowed_audiences": ["tandem-runtime"], "status": "active"}}).to_string();
    let security = crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(&raw, &temp.path().join("replay.json"));
    *state.context_assertion_security.write().unwrap() = Some(std::sync::Arc::new(security));
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            monitored_projects: vec![crate::IncidentMonitorMonitoredProject {
                project_id: "payments".into(),
                name: "Payments".into(),
                repo: "acme/payments".into(),
                workspace_root: temp.path().display().to_string(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    state
        .put_incident_monitor_intake_key(crate::IncidentMonitorProjectIntakeKey {
            key_id: "existing".into(),
            project_id: "payments".into(),
            name: "Existing".into(),
            key_hash: crate::sha256_hex(&["existing-raw"]),
            enabled: true,
            scopes: vec!["incident_monitor:report".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    (state, temp, key)
}

fn management_request(method: &str, route: &str) -> Request {
    Request::builder()
        .method(method)
        .uri(route)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"project_id": "payments", "name": "Created"}).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn hosted_intake_key_management_requires_current_admin() {
    let (state, temp, key) = intake_fixture().await;
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    for (version, role, permissions, allowed) in [
        (1, "viewer", json!([]), false),
        (2, "member", json!([]), false),
        (
            3,
            "viewer",
            json!(["automation.write", "automation.execute"]),
            false,
        ),
        (4, "admin", json!([]), true),
        (5, "owner", json!([]), true),
        (6, "viewer", json!(["hosted.admin"]), true),
    ] {
        let now = crate::now_ms();
        write_policy(&path, version, Some(role), now);
        if version == 3 || version == 6 {
            let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
                "resource_id": "dep-a", "permissions": permissions}]);
            std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        }
        state.reload_hosted_policy().await.unwrap();
        for through_ingress in [true, false] {
            for (method, route) in [
                ("GET", "/incident-monitor/intake/keys"),
                ("HEAD", "/incident-monitor/intake/keys"),
                ("POST", "/incident-monitor/intake/keys"),
                ("POST", "/incident-monitor/intake/keys/existing/disable"),
            ] {
                let before =
                    serde_json::to_value(state.list_incident_monitor_intake_keys().await).unwrap();
                let persisted = std::fs::read(&state.incident_monitor_intake_keys_path).unwrap();
                let mut assertion = claims("alice", role, version, now);
                assertion.assertion_id = uuid::Uuid::new_v4().to_string();
                let router = crate::http::routes_incident_monitor::apply(Router::new());
                let app = if through_ingress {
                    router
                        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
                        .with_state(state.clone())
                } else {
                    router
                        .layer(Extension(VerifiedTenantContext::from(assertion.clone())))
                        .with_state(state.clone())
                };
                let mut request = management_request(method, route);
                request.headers_mut().insert(
                    "x-tandem-context-assertion",
                    crate::http::middleware::tests::sign_test_context_assertion(
                        &key, "key-a", assertion,
                    )
                    .parse()
                    .unwrap(),
                );
                let response = app.oneshot(request).await.unwrap();
                assert_eq!(
                    response.status(),
                    if allowed {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                    "{role}: {method} {route}, ingress={through_ingress}"
                );
                let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                if !allowed {
                    assert_eq!(
                        serde_json::to_value(state.list_incident_monitor_intake_keys().await)
                            .unwrap(),
                        before
                    );
                    assert_eq!(
                        std::fs::read(&state.incident_monitor_intake_keys_path).unwrap(),
                        persisted
                    );
                    assert!(!String::from_utf8_lossy(&bytes).contains("tim_intake_"));
                } else if method != "HEAD" {
                    let payload: Value = serde_json::from_slice(&bytes).unwrap();
                    if method == "GET" {
                        assert!(payload["keys"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|row| row["key_hash"] == "[redacted]"));
                    } else if route.ends_with("/disable") {
                        assert_eq!(payload["key"]["enabled"], false);
                        assert!(state
                            .validate_incident_monitor_intake_key(
                                "existing-raw",
                                "payments",
                                "incident_monitor:report"
                            )
                            .await
                            .is_none());
                    } else {
                        assert_eq!(payload["key"]["key_hash"], "[redacted]");
                        assert!(state
                            .validate_incident_monitor_intake_key(
                                payload["raw_key"].as_str().unwrap(),
                                "payments",
                                "incident_monitor:report"
                            )
                            .await
                            .is_some());
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn local_intake_key_management_remains_available() {
    let (state, _temp, _key) = intake_fixture().await;
    let app = crate::http::routes_incident_monitor::apply(Router::new()).with_state(state);
    for (method, route) in [
        ("GET", "/incident-monitor/intake/keys"),
        ("HEAD", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys/existing/disable"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(management_request(method, route))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn hosted_intake_key_management_rechecks_authority_after_lock_wait() {
    let (state, temp, _key) = intake_fixture().await;
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    for (index, (method, route)) in [
        ("GET", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys/existing/disable"),
    ]
    .into_iter()
    .enumerate()
    {
        let version = 1 + 2 * index as u64;
        let now = crate::now_ms();
        write_policy(&path, version, Some("admin"), now);
        state.reload_hosted_policy().await.unwrap();
        let verified: VerifiedTenantContext = claims("alice", "admin", version, now).into();
        state
            .enterprise
            .hosted_policy
            .authorize_permission(Some(&verified), AccessPermission::HostedAdmin)
            .unwrap();
        let app = crate::http::routes_incident_monitor::apply(Router::new())
            .layer(Extension(verified))
            .with_state(state.clone());
        let held = state.incident_monitor_intake_keys.write().await;
        let before = serde_json::to_value(&*held).unwrap();
        let persisted = std::fs::read(&state.incident_monitor_intake_keys_path).unwrap();
        let pending = app.oneshot(management_request(method, route));
        tokio::pin!(pending);
        assert!(futures::poll!(&mut pending).is_pending());
        write_policy(&path, version + 1, Some("viewer"), now);
        state.reload_hosted_policy().await.unwrap();
        drop(held);
        assert_eq!(
            pending.await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "revoked {method} {route}"
        );
        assert_eq!(
            serde_json::to_value(&*state.incident_monitor_intake_keys.read().await).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read(&state.incident_monitor_intake_keys_path).unwrap(),
            persisted
        );
    }
    // A direct handler must fail closed without verified identity, even for a
    // nonexistent key, rather than revealing existence before authorization.
    let app = crate::http::routes_incident_monitor::apply(Router::new()).with_state(state);
    for (method, route) in [
        ("GET", "/incident-monitor/intake/keys"),
        ("HEAD", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys/missing/disable"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(management_request(method, route))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn local_intake_key_usage_cannot_restore_a_concurrently_disabled_key() {
    let (state, _temp, _key) = intake_fixture().await;
    let held = state.incident_monitor_intake_keys.write().await;
    let validation = state.validate_incident_monitor_intake_key(
        "existing-raw",
        "payments",
        "incident_monitor:report",
    );
    let disable = state.disable_incident_monitor_intake_key_checked("existing", || Ok(()));
    tokio::pin!(validation);
    tokio::pin!(disable);
    // Queue validation first and disable second. With the old read/clone/put
    // sequence, validation then queues a second write behind the disable.
    assert!(futures::poll!(&mut validation).is_pending());
    assert!(futures::poll!(&mut disable).is_pending());
    drop(held);
    assert!(futures::poll!(&mut validation).is_pending());
    let (disabled, validated) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(&mut disable, &mut validation)
    })
    .await
    .expect("queued key operations must finish");
    assert!(
        !state.incident_monitor_intake_keys.read().await["existing"].enabled,
        "usage bookkeeping restored a disabled credential",
    );
    assert!(!disabled.unwrap().unwrap().enabled);
    // The report was admitted before disable; its completed authorization may
    // return, but it must never restore the old credential record.
    assert!(validated.is_some());
    state.load_incident_monitor_intake_keys().await.unwrap();
    assert!(
        state
            .validate_incident_monitor_intake_key(
                "existing-raw",
                "payments",
                "incident_monitor:report",
            )
            .await
            .is_none(),
        "disabled state must survive reload"
    );
}

#[tokio::test]
async fn local_intake_key_persistence_serializes_concurrent_writers() {
    let (state, _temp, _key) = intake_fixture().await;
    state
        .disable_incident_monitor_intake_key_checked("existing", || Ok(()))
        .await
        .unwrap();
    for _ in 0..8 {
        let (first, second, third) = tokio::join!(
            state.persist_incident_monitor_intake_keys(),
            state.persist_incident_monitor_intake_keys(),
            state.persist_incident_monitor_intake_keys(),
        );
        first.unwrap();
        second.unwrap();
        third.unwrap();
        state.load_incident_monitor_intake_keys().await.unwrap();
        assert!(!state.incident_monitor_intake_keys.read().await["existing"].enabled);
    }
}

#[tokio::test]
async fn local_intake_key_report_burst_coalesces_management_publication() {
    let (state, _temp, _key) = intake_fixture().await;
    let held = state.incident_monitor_intake_keys_persistence.lock().await;
    let mut reports = Vec::new();
    for _ in 0..128 {
        let mut report = Box::pin(state.validate_incident_monitor_intake_key(
            "existing-raw",
            "payments",
            "incident_monitor:report",
        ));
        assert!(futures::poll!(&mut report).is_pending());
        reports.push(report);
        tokio::task::yield_now().await;
    }
    // This read-only barrier is behind the burst but before the disable.
    // Coalescing must include the disable in the already pending snapshot,
    // rather than queue another filesystem job behind this barrier.
    let barrier = state.incident_monitor_intake_keys_persistence.lock();
    tokio::pin!(barrier);
    assert!(futures::poll!(&mut barrier).is_pending());
    let disable = state.disable_incident_monitor_intake_key_checked("existing", || Ok(()));
    tokio::pin!(disable);
    assert!(futures::poll!(&mut disable).is_pending());
    assert!(!state.incident_monitor_intake_keys.try_read().unwrap()["existing"].enabled);
    drop(held);
    let barrier = tokio::time::timeout(std::time::Duration::from_secs(20), barrier)
        .await
        .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(1), &mut disable).await;
    drop(barrier);
    // Drain original behavior before asserting so failure leaves no writers
    // racing fixture cleanup. This is not an extra persistence operation.
    if completed.is_err() {
        (&mut disable).await.unwrap();
    }
    for report in reports {
        assert!(report.await.is_some());
    }
    assert!(
        completed.is_ok(),
        "disable was queued as a separate publication after the report burst"
    );
    assert!(!completed.unwrap().unwrap().unwrap().enabled);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(&state.incident_monitor_intake_keys_path).unwrap())
            .unwrap();
    assert_eq!(saved["existing"]["enabled"], false);
}

#[test]
fn local_intake_key_slow_persistence_does_not_delay_disable() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (state, _temp, _key) = intake_fixture().await;
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let occupied = tokio::task::spawn_blocking(move || {
            let _ = entered_tx.send(());
            // Dropping the sender also releases the worker on test failure.
            let _ = release_rx.recv();
        });
        entered_rx.await.unwrap();
        {
            let pending = state.persist_incident_monitor_intake_keys();
            tokio::pin!(pending);
            assert!(futures::poll!(&mut pending).is_pending());
            assert!(
                state.incident_monitor_intake_keys.try_write().is_ok(),
                "a queued filesystem write must not hold the authorization map lock",
            );
        }
        assert!(
            state.incident_monitor_intake_keys.try_write().is_ok(),
            "cancelled persistence must not block key management",
        );
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while state
                .incident_monitor_intake_keys_persistence
                .try_lock()
                .is_ok()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            state
                .incident_monitor_intake_keys_persistence
                .try_lock()
                .is_err(),
            "cancelling the caller must not release publication order before IO finishes",
        );
        let queued = state.persist_incident_monitor_intake_keys();
        tokio::pin!(queued);
        assert!(futures::poll!(&mut queued).is_pending());
        let disable = state.disable_incident_monitor_intake_key_checked("existing", || Ok(()));
        tokio::pin!(disable);
        assert!(futures::poll!(&mut disable).is_pending());
        assert!(!state.incident_monitor_intake_keys.try_read().unwrap()["existing"].enabled);
        let validation = state.validate_incident_monitor_intake_key(
            "existing-raw",
            "payments",
            "incident_monitor:report",
        );
        tokio::pin!(validation);
        assert!(matches!(
            futures::poll!(&mut validation),
            std::task::Poll::Ready(None)
        ));
        let listing = state.list_incident_monitor_intake_keys_checked(|| Ok(()));
        tokio::pin!(listing);
        assert!(matches!(
            futures::poll!(&mut listing),
            std::task::Poll::Ready(Ok(_))
        ));
        release_tx.send(()).unwrap();
        occupied.await.unwrap();
        let (saved_queue, disabled) =
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                tokio::join!(&mut queued, &mut disable)
            })
            .await
            .unwrap();
        saved_queue.unwrap();
        assert!(!disabled.unwrap().unwrap().enabled);
        let current = state.incident_monitor_intake_keys.read().await;
        let saved: Value = serde_json::from_slice(
            &std::fs::read(&state.incident_monitor_intake_keys_path).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, serde_json::to_value(&*current).unwrap());
        drop(current);
        state.load_incident_monitor_intake_keys().await.unwrap();
        assert!(!state.incident_monitor_intake_keys.read().await["existing"].enabled);
    });
}

#[tokio::test]
async fn local_intake_key_queued_persistence_snapshots_after_publication_lock() {
    let (state, _temp, _key) = intake_fixture().await;
    let held = state.incident_monitor_intake_keys_persistence.lock().await;
    let queued = state.persist_incident_monitor_intake_keys();
    tokio::pin!(queued);
    assert!(futures::poll!(&mut queued).is_pending());
    {
        let disable = state.disable_incident_monitor_intake_key_checked("existing", || Ok(()));
        tokio::pin!(disable);
        assert!(futures::poll!(&mut disable).is_pending());
        assert!(!state.incident_monitor_intake_keys.try_read().unwrap()["existing"].enabled);
        // Cancel this caller before publication; the older queued writer must
        // still snapshot the latest map, not the enabled state at enqueue time.
    }
    drop(held);
    tokio::time::timeout(std::time::Duration::from_secs(10), queued)
        .await
        .unwrap()
        .unwrap();
    state.load_incident_monitor_intake_keys().await.unwrap();
    assert!(!state.incident_monitor_intake_keys.read().await["existing"].enabled);
}

#[tokio::test]
async fn local_intake_key_publication_reports_filesystem_errors() {
    let (mut state, temp, _key) = intake_fixture().await;
    let blocked_parent = temp.path().join("not-a-directory");
    std::fs::write(&blocked_parent, "fixture").unwrap();
    state.incident_monitor_intake_keys_path = blocked_parent.join("keys.json");
    assert!(state
        .disable_incident_monitor_intake_key_checked("existing", || Ok(()))
        .await
        .is_err());
    assert!(!state.incident_monitor_intake_keys.read().await["existing"].enabled);
    assert!(
        state
            .incident_monitor_intake_keys_persistence
            .try_lock()
            .is_ok(),
        "failed publications must release publication order"
    );
}

#[test]
fn local_intake_key_cancelled_queued_disable_survives_reload() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (state, _temp, _key) = intake_fixture().await;
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let occupied = tokio::task::spawn_blocking(move || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        });
        entered_rx.await.unwrap();
        let older = state.persist_incident_monitor_intake_keys();
        tokio::pin!(older);
        assert!(futures::poll!(&mut older).is_pending());
        // On this single-thread runtime, a scheduled publication runs through
        // its uncontended snapshot to the blocked filesystem task in one poll.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while state
                .incident_monitor_intake_keys_persistence
                .try_lock()
                .is_ok()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(state.incident_monitor_intake_keys.try_write().is_ok());
        {
            let disable = state.disable_incident_monitor_intake_key_checked("existing", || Ok(()));
            tokio::pin!(disable);
            assert!(futures::poll!(&mut disable).is_pending());
            assert!(!state.incident_monitor_intake_keys.try_read().unwrap()["existing"].enabled);
            // Drop the caller while its publication is queued behind an
            // already captured enabled snapshot, not before that snapshot.
        }
        tokio::task::yield_now().await;
        // Queue a read-only completion barrier after the cancelled caller's
        // publication. Another persist call would mask the bug by repairing it.
        let barrier = state.incident_monitor_intake_keys_persistence.lock();
        tokio::pin!(barrier);
        assert!(futures::poll!(&mut barrier).is_pending());
        release_tx.send(()).unwrap();
        occupied.await.unwrap();
        let held = tokio::time::timeout(std::time::Duration::from_secs(10), barrier)
            .await
            .unwrap();
        older.await.unwrap();
        let saved: Value = serde_json::from_slice(
            &std::fs::read(&state.incident_monitor_intake_keys_path).unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved["existing"]["enabled"], false,
            "cancelled disable was lost on disk"
        );
        drop(held);
        state.load_incident_monitor_intake_keys().await.unwrap();
        assert!(
            state
                .validate_incident_monitor_intake_key(
                    "existing-raw",
                    "payments",
                    "incident_monitor:report",
                )
                .await
                .is_none(),
            "restart must not resurrect the disabled key"
        );
    });
}

#[tokio::test]
async fn local_intake_key_coalesced_errors_reach_all_callers_and_allow_retry() {
    let (mut state, temp, _key) = intake_fixture().await;
    let original_path = state.incident_monitor_intake_keys_path.clone();
    let blocked_parent = temp.path().join("coalesced-not-a-directory");
    std::fs::write(&blocked_parent, "fixture").unwrap();
    state.incident_monitor_intake_keys_path = blocked_parent.join("keys.json");
    {
        let held = state.incident_monitor_intake_keys_persistence.lock().await;
        let first = state.persist_incident_monitor_intake_keys();
        tokio::pin!(first);
        assert!(futures::poll!(&mut first).is_pending());
        let disable = state.disable_incident_monitor_intake_key_checked("existing", || Ok(()));
        tokio::pin!(disable);
        assert!(futures::poll!(&mut disable).is_pending());
        assert!(!state.incident_monitor_intake_keys.try_read().unwrap()["existing"].enabled);
        drop(held);
        let (first, disable) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(first, disable)
        })
        .await
        .unwrap();
        assert!(first.is_err());
        assert!(disable.is_err());
    }
    state.incident_monitor_intake_keys_path = original_path;
    state.persist_incident_monitor_intake_keys().await.unwrap();
    state.load_incident_monitor_intake_keys().await.unwrap();
    assert!(!state.incident_monitor_intake_keys.read().await["existing"].enabled);
}
