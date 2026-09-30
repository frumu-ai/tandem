use super::*;

struct PlaintextBindingEnvironment {
    _key_dir: tempfile::TempDir,
    previous_key_file: Option<std::ffi::OsString>,
    previous_required: Option<std::ffi::OsString>,
}

impl PlaintextBindingEnvironment {
    fn new() -> Self {
        let key_dir = tempfile::tempdir().unwrap();
        let previous_key_file = std::env::var_os("TANDEM_MEMORY_LOCAL_KEY_FILE");
        let previous_required = std::env::var_os("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
        std::env::set_var(
            "TANDEM_MEMORY_LOCAL_KEY_FILE",
            key_dir.path().join("solution-binding.key"),
        );
        std::env::set_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED", "false");
        Self {
            _key_dir: key_dir,
            previous_key_file,
            previous_required,
        }
    }
}

impl Drop for PlaintextBindingEnvironment {
    fn drop(&mut self) {
        for (name, previous) in [
            (
                "TANDEM_MEMORY_LOCAL_KEY_FILE",
                self.previous_key_file.take(),
            ),
            (
                "TANDEM_MEMORY_ENCRYPTION_REQUIRED",
                self.previous_required.take(),
            ),
        ] {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

fn plaintext_binding<T>(operation: impl FnOnce() -> T) -> T {
    let _environment = PlaintextBindingEnvironment::new();
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::plaintext(),
        None,
        async { operation() },
    ))
}

#[test]
#[serial]
fn solution_budget_provider_historical_metadata_cannot_forge_goal_binding() {
    encrypted(assert_historical_metadata_cannot_forge_goal_binding);
}

#[test]
#[serial]
fn solution_budget_provider_plaintext_mode_rejects_forged_goal_binding() {
    plaintext_binding(assert_historical_metadata_cannot_forge_goal_binding);
}

fn assert_historical_metadata_cannot_forge_goal_binding() {
    for_each_backend(|_, store| {
        for fault in ["plaintext", "copied-envelope"] {
            let a = network_fixture(store, &format!("historical-{fault}"));
            let b = network_fixture(store, &format!("other-{fault}"));
            let approved = approval(&a, store);
            let mut goal = store.get_goal(&goal_id(&a)).unwrap().unwrap();
            let forged = if fault == "plaintext" {
                serde_json::Value::String(
                    serde_json::json!({
                        "schema_version": 1, "scope": approved.scope,
                        "configuration": approved.configuration,
                        "installation_generation": approved.installation_generation,
                        "composition_sha256": approved.composition_sha256,
                        "actor_id": approved.verified.human_actor.actor_id,
                        "root_run_id": approved.root_run_id,
                    })
                    .to_string(),
                )
            } else {
                store
                    .get_goal(&goal_id(&b))
                    .unwrap()
                    .unwrap()
                    .metadata
                    .unwrap()["tandem_solution_binding"]
                    .clone()
            };
            goal.metadata.as_mut().unwrap()["tandem_solution_binding"] = forged;
            // Simulate metadata persisted by an old server. Its outer record
            // uses the configured storage mode; only the host-issued inner proof
            // is absent or belongs to a different goal.
            store
                .with_connection(|connection| {
                    connection.execute(
                        "UPDATE long_running_goals SET goal_json=?1 WHERE goal_id=?2",
                        params![
                            protected_records::encode(
                                &goal.tenant_context,
                                "goal",
                                &goal.goal_id,
                                &goal
                            )?,
                            goal.goal_id
                        ],
                    )?;
                    Ok(())
                })
                .unwrap();
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                let policy = policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
                assert!(complete(&registry, policy).await.is_err(), "{fault}");
                assert!(account_row(store, &a, "global").await.is_none());
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(30),
                    listener.accept()
                )
                .await
                .is_err());
            });
        }
    });
}

#[test]
#[serial]
fn solution_budget_provider_plaintext_goal_binding_survives_restart_and_remains_immutable() {
    plaintext_binding(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "plaintext-goal-binding", 4);
            let installed = staged_installation(store, &fixture);
            let (goal, root, link) = execution_records(&fixture);
            let actor = tandem_types::PrincipalRef::human_user(
                &fixture.installation.customer.context.human_actor.actor_id,
            );
            assert!(matches!(
                store
                    .start_solution_goal(
                        &goal,
                        &root,
                        &link,
                        &actor,
                        goal_start(&fixture, &installed),
                        || 1500,
                    )
                    .unwrap(),
                StartGoalOutcome::Created { .. }
            ));
            let bound = store.get_goal(&goal.goal_id).unwrap().unwrap();
            let binding = bound.metadata.as_ref().unwrap()["tandem_solution_binding"].clone();
            assert!(crate::encrypted_file_store::is_encrypted_payload(
                binding.as_str().unwrap()
            ));
            let raw: String = store
                .with_connection(|connection| {
                    Ok(connection.query_row(
                        "SELECT goal_json FROM long_running_goals WHERE goal_id=?1",
                        [&goal.goal_id],
                        |row| row.get(0),
                    )?)
                })
                .unwrap();
            assert!(
                !crate::encrypted_file_store::is_encrypted_payload(&raw),
                "ordinary outer runtime storage must stay in configured plaintext mode"
            );
            let outer: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(
                outer["payload"]["metadata"]["tandem_solution_binding"],
                binding
            );

            store.initialize().unwrap();
            assert!(matches!(
                store
                    .start_solution_goal(
                        &goal,
                        &root,
                        &link,
                        &actor,
                        goal_start(&fixture, &installed),
                        || 1500,
                    )
                    .unwrap(),
                StartGoalOutcome::AlreadyStarted { .. }
            ));
            let restarted = store.get_goal(&goal.goal_id).unwrap().unwrap();
            assert_eq!(restarted, bound);
            assert_eq!(
                restarted.metadata.as_ref().unwrap()["tandem_solution_binding"],
                binding,
                "idempotent replay must preserve the originally stored sealed binding"
            );

            assert!(store.start_goal(&bound, &root, &link, &actor).is_err());
            assert!(store.start_goal(&goal, &root, &link, &actor).is_err());
            let mut changed = bound.clone();
            changed.metadata = None;
            assert!(store.put_goal(&changed).is_err());
            changed = bound.clone();
            changed.goal_id = "plaintext-forged-copy".into();
            assert!(store.put_goal(&changed).is_err());
            assert!(store.get_goal("plaintext-forged-copy").unwrap().is_none());
            assert_eq!(store.get_goal(&goal.goal_id).unwrap().unwrap(), bound);
        });
    });
}

#[test]
#[serial]
fn solution_budget_provider_goal_binding_requires_staging_and_is_immutable() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "goal-binding", 4);
            let (goal, root, link) = execution_records(&fixture);
            let actor = tandem_types::PrincipalRef::human_user(
                &fixture.installation.customer.context.human_actor.actor_id,
            );
            let pending = fixture.installation.read(store);
            assert!(store
                .start_solution_goal(
                    &goal,
                    &root,
                    &link,
                    &actor,
                    goal_start(&fixture, &pending),
                    || 1500
                )
                .is_err());
            assert!(store.get_goal(&goal.goal_id).unwrap().is_none());
            assert!(store.get_automation_run(&root.run_id).unwrap().is_none());
            let installed = staged_installation(store, &fixture);
            // The initiating input still says 1500. Admission must instead use
            // the trusted clock sampled inside its writer transaction.
            let expired = fixture.installation.customer.context.expires_at_ms + 1;
            assert!(store
                .start_solution_goal(
                    &goal,
                    &root,
                    &link,
                    &actor,
                    goal_start(&fixture, &installed),
                    || expired,
                )
                .is_err());
            assert!(store.get_goal(&goal.goal_id).unwrap().is_none());
            assert!(store.get_automation_run(&root.run_id).unwrap().is_none());
            assert!(matches!(
                store
                    .start_solution_goal(
                        &goal,
                        &root,
                        &link,
                        &actor,
                        goal_start(&fixture, &installed),
                        || 1500,
                    )
                    .unwrap(),
                StartGoalOutcome::Created { .. }
            ));
            store.initialize().unwrap();
            assert!(matches!(
                store
                    .start_solution_goal(
                        &goal,
                        &root,
                        &link,
                        &actor,
                        goal_start(&fixture, &installed),
                        || 1500,
                    )
                    .unwrap(),
                StartGoalOutcome::AlreadyStarted { .. }
            ));
            let bound = store.get_goal(&goal.goal_id).unwrap().unwrap();
            assert!(store.start_goal(&bound, &root, &link, &actor).is_err());
            assert!(store.start_goal(&goal, &root, &link, &actor).is_err());
            let mut changed = bound.clone();
            changed.metadata = None;
            assert!(store.put_goal(&changed).is_err());
            changed = bound.clone();
            changed.goal_id = "forged-copy".into();
            assert!(store.put_goal(&changed).is_err());
            assert!(store.get_goal("forged-copy").unwrap().is_none());
            assert_eq!(store.get_goal(&goal.goal_id).unwrap().unwrap(), bound);
        })
    });
}

#[test]
#[serial]
fn solution_budget_provider_goal_cannot_charge_another_installation() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let a = network_fixture(store, "goal-owner-a");
            let b = network_fixture(store, "goal-owner-b");
            let original = approval(&a, store);
            let mut other = approval(&b, store);
            other.execution = original.execution;
            other.root_run_id = original.root_run_id;
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                let policy = policy(store, &registry, other, Arc::new(AtomicU64::new(1500)));
                let denied = complete(&registry, policy).await.unwrap_err();
                assert!(format!("{denied:#}").contains("another solution"));
                assert!(account_row(store, &b, "global").await.is_none());
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(30),
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
fn solution_budget_provider_concurrent_goal_start_cannot_rebind_installation() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let a = BudgetFixture::new(store, "a", "start-race-a", 4);
            let b = BudgetFixture::new(store, "a", "start-race-b", 4);
            let ia = staged_installation(store, &a);
            let ib = staged_installation(store, &b);
            let (goal, root, link) = execution_records(&a);
            let actor = tandem_types::PrincipalRef::human_user(
                &a.installation.customer.context.human_actor.actor_id,
            );
            let barrier = std::sync::Barrier::new(2);
            let results = std::thread::scope(|scope| {
                let handles: Vec<_> = [(&a, &ia), (&b, &ib)]
                    .into_iter()
                    .map(|(fixture, installed)| {
                        let (goal, root, link, actor, barrier) =
                            (&goal, &root, &link, &actor, &barrier);
                        scope.spawn(move || {
                            encrypted(|| {
                                barrier.wait();
                                store.start_solution_goal(
                                    goal,
                                    root,
                                    link,
                                    actor,
                                    goal_start(fixture, installed),
                                    || 1500,
                                )
                            })
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect::<Vec<_>>()
            });
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
            let winner = store.get_goal(&goal.goal_id).unwrap().unwrap();
            store.initialize().unwrap();
            assert_eq!(store.get_goal(&goal.goal_id).unwrap().unwrap(), winner);
        })
    });
}
