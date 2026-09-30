// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::{future::Future, sync::atomic::AtomicBool, time::Duration};

async fn protected<F: Future>(future: F) -> F::Output {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x73; 32]),
        None,
        future,
    )
    .await
}

async fn stored_attempt(
    store: &OrchestrationStateStore,
    fixture: &BudgetFixture,
) -> (u64, SolutionChargeReservation) {
    let store = store.clone();
    let tenant = fixture.installation.customer.context.tenant_context.clone();
    let scope = fixture.installation.customer.config.scope.clone();
    protected(crate::encrypted_file_store::spawn_protected_blocking(move || {
        store.with_connection(|connection| {
            let count: u64 = connection.query_row(
                "SELECT COUNT(*) FROM solution_budget_records WHERE instance_id=?1 AND record_key LIKE 'attempt:%'",
                params![scope.instance_id],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "cancellation must retain the immutable attempt row");
            let key: String = connection.query_row(
                "SELECT record_key FROM solution_budget_records WHERE instance_id=?1 AND record_key LIKE 'attempt:%'",
                params![scope.instance_id],
                |row| row.get(0),
            )?;
            Ok(solution_budget_records::load(connection, &tenant, &scope, &key)?.unwrap())
        })
    }))
    .await
    .unwrap()
    .unwrap()
}

async fn assert_released_once(
    store: &OrchestrationStateStore,
    fixture: &BudgetFixture,
    listener: &tokio::net::TcpListener,
) {
    // Drop happens outside TEST_CRYPTO. The detached cleanup must carry its own
    // protected context and finish without needing the cancelled task to poll.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if protected(account_row(store, fixture, "global"))
                .await
                .is_some_and(|row| row["outstanding"] == 0)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("undispatched cancellation must durably settle at zero");
    let reopened = store.clone();
    let reopened = protected(crate::encrypted_file_store::spawn_protected_blocking(
        move || {
            reopened.initialize()?;
            Ok::<_, anyhow::Error>(reopened)
        },
    ))
    .await
    .unwrap()
    .unwrap();
    for key in [
        "global".into(),
        "day:0".into(),
        format!("root:{}", sha256(root_id(fixture).as_bytes())),
    ] {
        let row = protected(account(&reopened, fixture, &key)).await;
        for field in [
            "committed_tokens",
            "reserved_tokens",
            "committed_cost",
            "reserved_cost",
            "outstanding",
        ] {
            assert_eq!(row[field], 0, "{key}.{field}");
        }
        if key != "global" {
            assert_eq!(row["requests"], 1, "admission count must not be refunded");
        }
    }
    let (generation, attempt) = stored_attempt(&reopened, fixture).await;
    assert_eq!(generation, 2, "one reserve plus one settlement");
    assert_eq!(
        attempt.status,
        SolutionChargeStatus::Settled {
            tokens: 0,
            cost_microusd: 0,
            overrun: false,
            cost_basis: SolutionChargeCostBasis::Confirmed,
        }
    );
    let retry_store = reopened.clone();
    let verified = fixture.installation.customer.context.clone();
    let scope = fixture.installation.customer.config.scope.clone();
    let digest = fixture.digest.clone();
    protected(crate::encrypted_file_store::spawn_protected_blocking(
        move || {
            retry_store.settle_solution_charge(
                SolutionBudgetInput {
                    verified: &verified,
                    scope: &scope,
                    composition_sha256: &digest,
                    now_ms: 1500,
                },
                &attempt.intent,
                0,
                0,
            )
        },
    ))
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        stored_attempt(&reopened, fixture).await.0,
        2,
        "retry is idempotent"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "cancelled presend attempt must not open TCP"
    );
}

#[test]
#[serial]
fn solution_budget_provider_cancelled_blocking_worker_output_releases_reservation() {
    for_each_backend(|_, store| {
        let fixture = encrypted(|| network_fixture(store, "provider-cancel-worker"));
        let approved = encrypted(|| approval(&fixture, store));
        runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let registry = registry(listener.local_addr().unwrap());
            let policy = policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
            let (started, waiting) = tokio::sync::oneshot::channel();
            let started = std::sync::Mutex::new(Some(started));
            let (release, released) = std::sync::mpsc::channel();
            let released = std::sync::Mutex::new(released);
            let barrier: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                started.lock().unwrap().take().unwrap().send(()).unwrap();
                released.lock().unwrap().recv_timeout(Duration::from_secs(3)).unwrap();
            });
            {
                let operation = crate::stateful_runtime::orchestration_store::provider_attempt_budget::BEFORE_RESERVATION
                    .scope(barrier, protected(complete(&registry, policy)));
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => panic!("blocked worker returned: {result:?}"),
                    result = tokio::time::timeout(Duration::from_secs(3), waiting) => result.unwrap().unwrap(),
                }
            } // Drop the awaiter and its TEST_CRYPTO scope before the worker commits.
            release.send(()).unwrap();
            assert_released_once(store, &fixture, &listener).await;
        });
    });
}

#[test]
#[serial]
fn solution_budget_provider_cancelled_worker_during_runtime_shutdown_releases_reservation() {
    for_each_backend(|_, store| {
        let fixture = encrypted(|| network_fixture(store, "provider-cancel-shutdown"));
        let approved = encrypted(|| approval(&fixture, store));
        let active_runtime = runtime();
        let (release, released) = std::sync::mpsc::channel();
        let released = std::sync::Mutex::new(released);
        let listener = active_runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let registry = registry(listener.local_addr().unwrap());
            let policy = policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
            let (started, waiting) = tokio::sync::oneshot::channel();
            let started = std::sync::Mutex::new(Some(started));
            let barrier: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                started.lock().unwrap().take().unwrap().send(()).unwrap();
                released.lock().unwrap().recv_timeout(Duration::from_secs(3)).unwrap();
            });
            {
                let operation = crate::stateful_runtime::orchestration_store::provider_attempt_budget::BEFORE_RESERVATION
                    .scope(barrier, protected(complete(&registry, policy)));
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => panic!("blocked worker returned: {result:?}"),
                    result = tokio::time::timeout(Duration::from_secs(3), waiting) => result.unwrap().unwrap(),
                }
            }
            // Deregister before stopping this reactor; check the same socket on
            // a new runtime rather than reusing a dead reactor's listener.
            listener.into_std().unwrap()
        });
        active_runtime.shutdown_background();
        release.send(()).unwrap();
        runtime().block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            assert_released_once(store, &fixture, &listener).await;
        });
    });
}

#[test]
#[serial]
fn solution_budget_provider_cancelled_server_authorize_recheck_releases_reservation() {
    for_each_backend(|_, store| {
        let fixture = encrypted(|| network_fixture(store, "provider-cancel-authorize"));
        let approved = encrypted(|| approval(&fixture, store));
        runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let registry = registry(listener.local_addr().unwrap());
            let started = Arc::new(tokio::sync::Notify::new());
            let signal = started.clone();
            let checks = Arc::new(AtomicUsize::new(0));
            let count = checks.clone();
            let runtime_registry = registry.clone();
            let policy = store.solution_provider_attempt_policy(10, 4096, move |_| {
                let approved = approved.clone();
                let registry = runtime_registry.clone();
                let signal = signal.clone();
                let recheck = count.fetch_add(1, Ordering::SeqCst) > 0;
                async move {
                    if recheck {
                        signal.notify_one();
                        std::future::pending().await
                    } else {
                        current(&approved, &registry).await
                    }
                }
            }, || 1500).unwrap();
            {
                let operation = protected(complete(&registry, policy));
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => panic!("authorize recheck returned: {result:?}"),
                    result = tokio::time::timeout(Duration::from_secs(3), started.notified()) => result.unwrap(),
                }
                assert_eq!(protected(account(store, &fixture, "global")).await["outstanding"], 1);
            }
            assert_eq!(checks.load(Ordering::SeqCst), 2);
            assert_released_once(store, &fixture, &listener).await;
        });
    });
}

#[test]
#[serial]
fn solution_budget_provider_cancelled_adapter_authority_recheck_releases_reservation() {
    for_each_backend(|_, store| {
        let fixture = encrypted(|| network_fixture(store, "provider-cancel-authority"));
        let approved = encrypted(|| approval(&fixture, store));
        runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let registry = registry(listener.local_addr().unwrap());
            let rechecked = Arc::new(AtomicBool::new(false));
            let ready = rechecked.clone();
            let checks = Arc::new(AtomicUsize::new(0));
            let count = checks.clone();
            let runtime_registry = registry.clone();
            let policy = store.solution_provider_attempt_policy(10, 4096, move |_| {
                let approved = approved.clone();
                let registry = runtime_registry.clone();
                let ready = ready.clone();
                let recheck = count.fetch_add(1, Ordering::SeqCst) > 0;
                async move {
                    let approval = current(&approved, &registry).await?;
                    if recheck { ready.store(true, Ordering::SeqCst); }
                    Ok(approval)
                }
            }, || 1500).unwrap();
            let started = Arc::new(tokio::sync::Notify::new());
            let signal = started.clone();
            let authority = tandem_providers::ProviderDispatchAuthority::new(move || {
                let signal = signal.clone();
                let ready = rechecked.load(Ordering::SeqCst);
                async move {
                    if ready {
                        signal.notify_one();
                        std::future::pending().await
                    } else { Ok(()) }
                }
            });
            {
                let operation = authority.scope(protected(complete(&registry, policy)));
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => panic!("adapter authority recheck returned: {result:?}"),
                    result = tokio::time::timeout(Duration::from_secs(3), started.notified()) => result.unwrap(),
                }
                assert_eq!(protected(account(store, &fixture, "global")).await["outstanding"], 1);
            }
            assert_eq!(checks.load(Ordering::SeqCst), 2);
            assert_released_once(store, &fixture, &listener).await;
        });
    });
}

#[test]
#[serial]
fn solution_budget_provider_cancelled_after_send_keeps_reservation() {
    for_each_backend(|_, store| {
        let fixture = encrypted(|| network_fixture(store, "provider-cancel-sent"));
        let approved = encrypted(|| approval(&fixture, store));
        runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let registry = registry(listener.local_addr().unwrap());
            let policy = policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
            let (arrived, waiting) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                request(&mut socket).await;
                arrived.send(()).unwrap();
                released.await.unwrap();
            });
            {
                let operation = protected(complete(&registry, policy));
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => panic!("sent attempt completed: {result:?}"),
                    result = tokio::time::timeout(Duration::from_secs(3), waiting) => result.unwrap().unwrap(),
                }
            }
            release.send(()).unwrap();
            server.await.unwrap();
            let reopened = store.clone();
            protected(crate::encrypted_file_store::spawn_protected_blocking(move || reopened.initialize()))
                .await.unwrap().unwrap();
            assert_eq!(protected(account(store, &fixture, "global")).await["outstanding"], 1);
            let root = protected(account(store, &fixture, &format!("root:{}", sha256(root_id(&fixture).as_bytes())))).await;
            assert_eq!(root["reserved_tokens"], 30);
            assert_eq!(root["reserved_cost"], 30);
            assert_eq!(root["requests"], 1);
            let (generation, attempt) = stored_attempt(store, &fixture).await;
            assert_eq!(generation, 1);
            assert_eq!(attempt.status, SolutionChargeStatus::Reserved);
        });
    });
}
