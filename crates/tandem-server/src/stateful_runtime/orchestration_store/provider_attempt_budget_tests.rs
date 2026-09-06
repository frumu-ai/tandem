use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tandem_providers::{AppConfig, ProviderAuthRecovery, ProviderConfig, ProviderRegistry};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn network_fixture(store: &OrchestrationStateStore, name: &str) -> BudgetFixture {
    let mut installation = InstallationFixture::new("a");
    installation.customer.config.scope.instance_id = name.into();
    for policy in [
        &mut installation.customer.blueprint.constraints,
        &mut installation.customer.config.constraints,
    ] {
        policy.allowed_providers = ["llama_cpp".into()].into();
        policy.allow_network_egress = true;
        policy.max_daily_cost_microusd = 50;
        policy.max_tokens_per_run = 100;
        policy.max_concurrent_runs = 4;
    }
    let model = installation.models.get_mut("local.fixture").unwrap();
    model.provider = "llama_cpp".into();
    model.uses_network = true;
    let (config, digest) = installation.seed(store);
    store
        .transition_solution_installation(
            installation.input(&config, &digest),
            None,
            SolutionInstallationTransition::Begin,
        )
        .unwrap();
    let fixture = BudgetFixture {
        installation,
        digest,
    };
    seed_execution(store, &fixture);
    fixture
}

#[path = "solution_execution_tests.rs"]
mod execution_tests;
use execution_tests::{root_id, seed_execution};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn configuration(address: std::net::SocketAddr, key: Option<&str>) -> AppConfig {
    AppConfig {
        providers: [(
            "llama_cpp".into(),
            ProviderConfig {
                url: Some(format!("http://{address}/v1")),
                api_key: key.map(str::to_string),
                default_model: Some("synthetic-model".into()),
            },
        )]
        .into(),
        default_provider: Some("llama_cpp".into()),
    }
}

fn registry(address: std::net::SocketAddr) -> ProviderRegistry {
    ProviderRegistry::new(configuration(address, None))
}

fn approval(
    fixture: &BudgetFixture,
    store: &OrchestrationStateStore,
) -> ApprovedSolutionProviderCharge {
    let installed = store
        .solution_installation(
            &fixture.installation.customer.context,
            &fixture.installation.customer.config.scope,
            1500,
        )
        .unwrap()
        .unwrap();
    ApprovedSolutionProviderCharge {
        verified: fixture.installation.customer.context.clone(),
        scope: fixture.installation.customer.config.scope.clone(),
        composition_sha256: fixture.digest.clone(),
        configuration: installed.config_version,
        installation_generation: installed.generation,
        model_class: "economy".into(),
        binding: installed.plan.models["economy"].clone(),
        root_run_id: root_id(fixture),
        execution: crate::stateful_runtime::orchestration_store::SolutionRunExecution {
            run_id: root_id(fixture),
            claim_id: "claim-1".into(),
            claimant_id: "executor-1".into(),
            lease_epoch: 1,
        },
        kind: SolutionChargeKind::Model,
        route_revision: sha256(b"synthetic-approved-account-model-price"),
        provider_id: "llama_cpp".into(),
        model_id: "synthetic-model".into(),
        protocol: tandem_providers::ProviderProtocol::ChatCompletions,
        endpoint_sha256: String::new(),
        credential_sha256: String::new(),
        maximum_input_tokens: 20,
        maximum_output_tokens: 10,
        run_budget: SolutionRunBudget {
            max_tokens: 100,
            max_cost_microusd: 50,
            max_requests: 4,
        },
        price: Some(ApprovedModelPrice {
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
            request_microusd: 0,
            valid_until_ms: 10_000,
        }),
    }
}

async fn current(
    approved: &ApprovedSolutionProviderCharge,
    registry: &ProviderRegistry,
) -> anyhow::Result<ApprovedSolutionProviderCharge> {
    let mut result = approved.clone();
    // Model/user authorization remains synthetic. Transport/account facts come
    // from the registry; run/claim/root facts are checked in protected storage.
    let binding = registry
        .runtime_binding_for_tenant(
            &approved.verified.tenant_context,
            &approved.provider_id,
            &approved.model_id,
        )
        .await?;
    anyhow::ensure!(binding.protocol == result.protocol, "protocol changed");
    result.endpoint_sha256 = binding.endpoint_sha256;
    result.credential_sha256 = binding.credential_sha256;
    Ok(result)
}

fn policy(
    store: &OrchestrationStateStore,
    registry: &ProviderRegistry,
    approved: ApprovedSolutionProviderCharge,
    clock: Arc<AtomicU64>,
) -> tandem_providers::ProviderAttemptPolicy {
    let registry = registry.clone();
    store
        .solution_provider_attempt_policy(
            10,
            4096,
            move |_attempt| {
                let approved = approved.clone();
                let registry = registry.clone();
                async move { current(&approved, &registry).await }
            },
            move || clock.load(Ordering::SeqCst),
        )
        .unwrap()
}

async fn complete(
    registry: &ProviderRegistry,
    policy: tandem_providers::ProviderAttemptPolicy,
) -> anyhow::Result<String> {
    registry
        .scope_tenant_provider_auth_with_recovery(
            tandem_types::TenantContext::explicit(
                "org-a",
                "workspace-a",
                Some("deployment-a".into()),
            ),
            ProviderAuthRecovery::new(|_| async { Ok(false) }),
            true,
            policy.scope(registry.complete_for_provider(
                Some("llama_cpp"),
                "synthetic request",
                None,
            )),
        )
        .await
}

async fn request(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 2048];
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let len: usize = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() - header_end < len {
        let mut chunk = [0; 2048];
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + len]).unwrap()
}

async fn reply(socket: &mut tokio::net::TcpStream, with_usage: bool) {
    let body = if with_usage {
        r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}"#
    } else {
        r#"{"choices":[{"message":{"content":"ok"}}],"usage":{}}"#
    };
    socket
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    socket.shutdown().await.unwrap();
}

async fn account_row(
    store: &OrchestrationStateStore,
    fixture: &BudgetFixture,
    key: &str,
) -> Option<serde_json::Value> {
    let store = store.clone();
    let tenant = fixture.installation.customer.context.tenant_context.clone();
    let scope = fixture.installation.customer.config.scope.clone();
    let key = key.to_string();
    crate::encrypted_file_store::spawn_protected_blocking(move || {
        store.with_connection(|connection| {
            crate::stateful_runtime::orchestration_store::solution_budget_records::load::<
                serde_json::Value,
            >(connection, &tenant, &scope, &key)
        })
    })
    .await
    .unwrap()
    .unwrap()
    .map(|(_, value)| value)
}

async fn account(
    store: &OrchestrationStateStore,
    fixture: &BudgetFixture,
    key: &str,
) -> serde_json::Value {
    account_row(store, fixture, key).await.unwrap()
}

#[test]
#[serial]
fn solution_budget_provider_actual_sends_share_the_durable_ceiling() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "provider-ceiling");
            let approved = approval(&fixture, store);
            runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let registry = registry(listener.local_addr().unwrap());
            let (arrived, waiting) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                assert_eq!(request(&mut socket).await["max_tokens"], 10);
                arrived.send(()).unwrap();
                released.await.unwrap();
                reply(&mut socket, true).await;
                let (mut socket, _) = listener.accept().await.unwrap();
                request(&mut socket).await;
                reply(&mut socket, true).await;
            });
            let clock = Arc::new(AtomicU64::new(1500));
            let policy = policy(store, &registry, approved, clock);
            let first = complete(&registry, policy.clone());
            tokio::pin!(first);
            tokio::select! {
                result = &mut first => panic!("first request completed before release: {result:?}"),
                result = waiting => result.unwrap(),
            }
            let error = complete(&registry, policy.clone()).await.unwrap_err();
            assert!(error.to_string().contains("budget"));
            assert_eq!(account(store, &fixture, "global").await["outstanding"], 1);
            release.send(()).unwrap();
            assert_eq!(first.await.unwrap(), "ok");
            assert_eq!(complete(&registry, policy).await.unwrap(), "ok");
            tokio::time::timeout(std::time::Duration::from_secs(3), server).await.unwrap().unwrap();
            assert_eq!(account(store, &fixture, "global").await["outstanding"], 0);
            let root = account(store, &fixture, &format!("root:{}", sha256(root_id(&fixture).as_bytes()))).await;
            assert_eq!(root["requests"], 2, "repeated policy checks must not double-charge");
            assert_eq!(root["committed_cost"], 10);
        });
        })
    });
}

#[test]
#[serial]
fn solution_budget_provider_missing_usage_survives_reopen_and_blocks_overspend() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "provider-unknown");
            let approved = approval(&fixture, store);
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    request(&mut socket).await;
                    reply(&mut socket, false).await;
                    listener
                });
                let policy = policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
                assert_eq!(complete(&registry, policy.clone()).await.unwrap(), "ok");
                let reopened = store.clone();
                crate::encrypted_file_store::spawn_protected_blocking(move || {
                    reopened.initialize()
                })
                .await
                .unwrap()
                .unwrap();
                assert!(complete(&registry, policy)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("budget"));
                let listener = tokio::time::timeout(std::time::Duration::from_secs(3), server)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    listener.accept()
                )
                .await
                .is_err());
                assert_eq!(account(store, &fixture, "global").await["outstanding"], 1);
            });
        })
    });
}

#[test]
#[serial]
fn solution_budget_provider_confirmed_receipt_settles_after_user_assertion_expires() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "provider-expiry");
            let approved = approval(&fixture, store);
            let expiry = approved.verified.expires_at_ms;
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                let clock = Arc::new(AtomicU64::new(1500));
                let tick = clock.clone();
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    request(&mut socket).await;
                    tick.store(expiry + 1, Ordering::SeqCst);
                    reply(&mut socket, true).await;
                    listener
                });
                let policy = policy(store, &registry, approved, clock);
                assert_eq!(complete(&registry, policy.clone()).await.unwrap(), "ok");
                assert!(
                    complete(&registry, policy).await.is_err(),
                    "old assertion cannot authorize another send"
                );
                assert_eq!(account(store, &fixture, "global").await["outstanding"], 0);
                assert_eq!(
                    account(
                        store,
                        &fixture,
                        &format!("root:{}", sha256(root_id(&fixture).as_bytes()))
                    )
                    .await["committed_cost"],
                    5
                );
                let listener = tokio::time::timeout(std::time::Duration::from_secs(3), server)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    listener.accept()
                )
                .await
                .is_err());
            });
        })
    });
}

#[test]
#[serial]
fn solution_budget_provider_rechecks_binding_after_reservation_without_sending() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "provider-stale");
            let approved = approval(&fixture, store);
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let registry = registry(address);
                let checks = Arc::new(AtomicUsize::new(0));
                let count = checks.clone();
                let runtime_registry = registry.clone();
                let policy = store
                    .solution_provider_attempt_policy(
                        10,
                        4096,
                        move |_attempt| {
                            let approved = approved.clone();
                            let registry = runtime_registry.clone();
                            let changed = count.fetch_add(1, Ordering::SeqCst) > 0;
                            async move {
                                if changed {
                                    registry
                                        .reload(configuration(
                                            address,
                                            Some("synthetic-rebound-account"),
                                        ))
                                        .await;
                                }
                                current(&approved, &registry).await
                            }
                        },
                        || 1500,
                    )
                    .unwrap();
                assert!(complete(&registry, policy)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("changed during"));
                assert_eq!(checks.load(Ordering::SeqCst), 2);
                assert_eq!(account(store, &fixture, "global").await["outstanding"], 0);
                let root = account(
                    store,
                    &fixture,
                    &format!("root:{}", sha256(root_id(&fixture).as_bytes())),
                )
                .await;
                assert_eq!(root["committed_cost"], 0);
                assert_eq!(root["requests"], 1);
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    listener.accept()
                )
                .await
                .is_err());
            });
        })
    });
}

#[test]
#[serial]
fn solution_budget_provider_rejects_stale_model_and_unknown_price_before_network() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "provider-invalid");
            let approved = approval(&fixture, store);
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                for variant in 0..4 {
                    let mut invalid = approved.clone();
                    match variant {
                        0 => invalid.installation_generation += 1,
                        1 => invalid.binding.binding_id = "different-approved-binding".into(),
                        2 => invalid.price = None,
                        _ => invalid.price.as_mut().unwrap().valid_until_ms = 1499,
                    }
                    let policy = policy(store, &registry, invalid, Arc::new(AtomicU64::new(1500)));
                    assert!(complete(&registry, policy).await.is_err());
                }
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    listener.accept()
                )
                .await
                .is_err());
                let global = account_row(store, &fixture, "global").await;
                assert!(
                    global.is_none(),
                    "rejected admission must not consume budget"
                );
            });
        })
    });
}

#[test]
fn solution_budget_provider_price_ceilings_round_up_and_reject_overflow() {
    let mut price = ApprovedModelPrice {
        input_microusd_per_million: 1,
        output_microusd_per_million: 1,
        request_microusd: 3,
        valid_until_ms: 2000,
    };
    assert_eq!(price.cost(1, 1).unwrap(), 5);
    assert_eq!(price.cost(0, 0).unwrap(), 3);
    price.input_microusd_per_million = u64::MAX;
    assert!(price.cost(u64::MAX, 0).is_err());
    price.input_microusd_per_million = 0;
    price.request_microusd = u64::MAX;
    assert!(price.cost(0, 1).is_err());
}

#[test]
#[serial]
fn solution_budget_provider_binds_persisted_revision_to_loaded_material_after_reservation() {
    use tandem_providers::{ProviderCredentialKind, ProviderCredentialLocation};
    encrypted(|| {
        for_each_backend(|_, store| {
            for variant in ["allowed", "stale-loaded", "reconnected"] {
                let fixture = network_fixture(store, &format!("provider-version-{variant}"));
                let approved = approval(&fixture, store);
                let security = tempfile::tempdir().unwrap();
                tandem_providers::set_provider_auth_for_tenant_in_dir(
                    security.path(),
                    &tandem_types::TenantContext::local_implicit(),
                    "llama_cpp",
                    "synthetic-a",
                )
                .unwrap();
                runtime().block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let registry = ProviderRegistry::new(configuration(
                        listener.local_addr().unwrap(),
                        Some("synthetic-a"),
                    ));
                    let reviewed = registry
                        .versioned_runtime_binding_for_tenant_in_dir(
                            security.path(),
                            &approved.verified.tenant_context,
                            "llama_cpp",
                            "synthetic-model",
                            ProviderCredentialKind::ApiKey,
                            ProviderCredentialLocation::HostService,
                        )
                        .await
                        .unwrap();
                    let expected_revision = reviewed.revision.authorization_revision;
                    let checks = Arc::new(AtomicUsize::new(0));
                    let count = checks.clone();
                    let path = security.path().to_path_buf();
                    let providers = registry.clone();
                    let policy = store
                        .solution_provider_attempt_policy(
                            10,
                            4096,
                            move |_attempt| {
                                let mut approved = approved.clone();
                                let expected_revision = expected_revision.clone();
                                let path = path.clone();
                                let providers = providers.clone();
                                let recheck = count.fetch_add(1, Ordering::SeqCst) == 1;
                                async move {
                                    if recheck && variant != "allowed" {
                                        let mut mutation =
                                            tandem_providers::provider_auth_mutation_in_dir(&path)
                                                .await?;
                                        let local = tandem_types::TenantContext::local_implicit();
                                        mutation.set_for_tenant(
                                            &local,
                                            "llama_cpp",
                                            "synthetic-b",
                                        )?;
                                        if variant == "reconnected" {
                                            mutation.set_for_tenant(
                                                &local,
                                                "llama_cpp",
                                                "synthetic-a",
                                            )?;
                                        }
                                    }
                                    let current = providers
                                        .versioned_runtime_binding_for_tenant_in_dir(
                                            &path,
                                            &approved.verified.tenant_context,
                                            &approved.provider_id,
                                            &approved.model_id,
                                            ProviderCredentialKind::ApiKey,
                                            ProviderCredentialLocation::HostService,
                                        )
                                        .await?;
                                    anyhow::ensure!(
                                        current.revision.authorization_revision
                                            == expected_revision,
                                        "credential authorization revision changed after review"
                                    );
                                    approved.route_revision =
                                        sha256(&tandem_solutions::canonical_json(&(
                                            &approved.route_revision,
                                            &expected_revision,
                                        ))?);
                                    approved.endpoint_sha256 = current.runtime.endpoint_sha256;
                                    approved.credential_sha256 = current.runtime.credential_sha256;
                                    Ok(approved)
                                }
                            },
                            || 1500,
                        )
                        .unwrap();
                    let listener = if variant == "allowed" {
                        let server = tokio::spawn(async move {
                            let (mut socket, _) = listener.accept().await.unwrap();
                            request(&mut socket).await;
                            reply(&mut socket, true).await;
                            listener
                        });
                        assert_eq!(complete(&registry, policy).await.unwrap(), "ok");
                        tokio::time::timeout(std::time::Duration::from_secs(3), server)
                            .await
                            .unwrap()
                            .unwrap()
                    } else {
                        let error = complete(&registry, policy).await.unwrap_err().to_string();
                        assert!(
                            error.contains(if variant == "reconnected" {
                                "authorization revision changed"
                            } else {
                                "loaded runtime credential differs"
                            }),
                            "{error}"
                        );
                        listener
                    };
                    assert_eq!(checks.load(Ordering::SeqCst), 2);
                    assert_eq!(account(store, &fixture, "global").await["outstanding"], 0);
                    let root = account(
                        store,
                        &fixture,
                        &format!("root:{}", sha256(root_id(&fixture).as_bytes())),
                    )
                    .await;
                    assert_eq!(root["requests"], 1);
                    assert_eq!(
                        root["committed_cost"],
                        if variant == "allowed" { 5 } else { 0 }
                    );
                    assert!(tokio::time::timeout(
                        std::time::Duration::from_millis(50),
                        listener.accept()
                    )
                    .await
                    .is_err());
                });
            }
        });
    });
}
