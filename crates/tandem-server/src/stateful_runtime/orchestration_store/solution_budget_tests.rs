use super::*;
use std::sync::{Arc, Barrier};

struct BudgetFixture {
    installation: InstallationFixture,
    digest: String,
}

impl BudgetFixture {
    fn new(
        store: &OrchestrationStateStore,
        customer: &str,
        instance: &str,
        concurrency: u32,
    ) -> Self {
        let mut installation = InstallationFixture::new(customer);
        installation.customer.config.scope.instance_id = instance.into();
        installation.customer.context.expires_at_ms = 10 * 86_400_000;
        for policy in [
            &mut installation.customer.blueprint.constraints,
            &mut installation.customer.config.constraints,
        ] {
            policy.max_daily_cost_microusd = 100;
            policy.max_tokens_per_run = 100;
            policy.max_concurrent_runs = concurrency;
        }
        let (config, digest) = installation.seed(store);
        store
            .transition_solution_installation(
                installation.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        Self {
            installation,
            digest,
        }
    }

    fn input(&self, now_ms: u64) -> SolutionBudgetInput<'_> {
        SolutionBudgetInput {
            verified: &self.installation.customer.context,
            scope: &self.installation.customer.config.scope,
            composition_sha256: &self.digest,
            now_ms,
        }
    }

    fn charge(id: &str, root: &str, cost: u64, tokens: u64) -> SolutionChargeIntent {
        SolutionChargeIntent {
            reservation_id: id.into(),
            root_run_id: root.into(),
            kind: SolutionChargeKind::Model,
            route_revision: sha256(b"current-host-approved-model-and-price"),
            maximum_tokens: tokens,
            maximum_cost_microusd: Some(cost),
            run_budget: SolutionRunBudget {
                max_cost_microusd: 100,
                max_tokens: 100,
                max_requests: 4,
            },
        }
    }
}

fn encrypted<T>(operation: impl FnOnce() -> T) -> T {
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x73; 32]),
        None,
        async { operation() },
    ))
}

#[test]
#[serial]
fn solution_budget_concurrent_workers_reserve_one_shared_ceiling() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "concurrent", 16);
            let barrier = Arc::new(Barrier::new(8));
            let results = std::thread::scope(|scope| {
                let threads: Vec<_> = (0..8)
                    .map(|index| {
                        let barrier = barrier.clone();
                        let fixture = &fixture;
                        scope.spawn(move || {
                            encrypted(|| {
                                barrier.wait();
                                store.reserve_solution_charge(
                                    fixture.input(1500),
                                    BudgetFixture::charge(
                                        &format!("attempt-{index}"),
                                        &format!("root-{index}"),
                                        60,
                                        10,
                                    ),
                                )
                            })
                        })
                    })
                    .collect();
                threads
                    .into_iter()
                    .map(|thread| thread.join().unwrap())
                    .collect::<Vec<_>>()
            });
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            let winning = results
                .into_iter()
                .find_map(Result::ok)
                .unwrap()
                .reservation;
            assert!(
                !store
                    .reserve_solution_charge(fixture.input(1500), winning.intent.clone())
                    .unwrap()
                    .newly_reserved
            );
            let settled = store
                .settle_solution_charge(fixture.input(1501), &winning.intent, 5, 30)
                .unwrap();
            assert_eq!(
                store
                    .settle_solution_charge(fixture.input(1501), &winning.intent, 5, 30)
                    .unwrap(),
                settled
            );
            assert!(store
                .settle_solution_charge(fixture.input(1501), &winning.intent, 0, 0)
                .is_err());
            assert!(store
                .reserve_solution_charge(
                    fixture.input(1501),
                    BudgetFixture::charge("too-much", "new-root", 71, 1)
                )
                .is_err());
            assert!(
                store
                    .reserve_solution_charge(
                        fixture.input(1501),
                        BudgetFixture::charge("remaining", "new-root", 70, 1)
                    )
                    .unwrap()
                    .newly_reserved
            );
        })
    });
}

#[test]
#[serial]
fn solution_budget_children_retries_and_free_requests_share_root_limits() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "root-budget", 4);
            let mut parent = BudgetFixture::charge("parent-attempt", "root", 0, 60);
            parent.run_budget.max_requests = 2;
            store
                .reserve_solution_charge(fixture.input(1500), parent.clone())
                .unwrap();
            store
                .settle_solution_charge(fixture.input(1500), &parent, 60, 0)
                .unwrap();
            let mut child = BudgetFixture::charge("child-retry", "root", 0, 41);
            child.kind = SolutionChargeKind::Retry;
            assert!(store
                .reserve_solution_charge(fixture.input(1500), child.clone())
                .is_err());
            child.maximum_tokens = 40;
            // A child asking for higher ceilings cannot widen the root's limits.
            child.run_budget.max_tokens = 999;
            child.run_budget.max_requests = 99;
            store
                .reserve_solution_charge(fixture.input(1500), child.clone())
                .unwrap();
            store
                .settle_solution_charge(fixture.input(1500), &child, 20, 0)
                .unwrap();
            assert!(store
                .reserve_solution_charge(
                    fixture.input(1500),
                    BudgetFixture::charge("third", "root", 0, 0)
                )
                .is_err());
            let mut altered = parent;
            altered.route_revision = sha256(b"another-route");
            assert!(store
                .reserve_solution_charge(fixture.input(1500), altered)
                .is_err());
        })
    });
}

#[test]
#[serial]
fn solution_budget_unknown_attempt_survives_midnight_and_restart_without_timeout_refund() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "uncertain", 1);
            let first = BudgetFixture::charge("interrupted", "root", 80, 50);
            store
                .reserve_solution_charge(fixture.input(1500), first.clone())
                .unwrap();
            store.initialize().unwrap();
            let tomorrow = 86_400_001;
            assert!(store
                .reserve_solution_charge(
                    fixture.input(tomorrow),
                    BudgetFixture::charge("next", "next-root", 1, 1)
                )
                .is_err());
            assert!(
                !store
                    .reserve_solution_charge(fixture.input(tomorrow), first.clone())
                    .unwrap()
                    .newly_reserved
            );
            // Only a confirmed adapter result can release the outstanding charge.
            store
                .settle_solution_charge(fixture.input(tomorrow), &first, 40, 70)
                .unwrap();
            assert!(store
                .reserve_solution_charge(
                    fixture.input(1500),
                    BudgetFixture::charge("clock-rollback", "next-root", 1, 1)
                )
                .is_err());
            assert!(
                store
                    .reserve_solution_charge(
                        fixture.input(tomorrow),
                        BudgetFixture::charge("next", "next-root", 100, 1)
                    )
                    .unwrap()
                    .newly_reserved
            );
        })
    });
}

#[test]
#[serial]
fn solution_budget_unknown_price_overrun_and_foreign_scope_fail_closed() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let a = BudgetFixture::new(store, "a", "same-id", 2);
            let b = BudgetFixture::new(store, "b", "same-id", 2);
            let mut unknown = BudgetFixture::charge("unknown", "root", 0, 1);
            unknown.maximum_cost_microusd = None;
            assert!(store
                .reserve_solution_charge(a.input(1500), unknown)
                .is_err());
            let charge = BudgetFixture::charge("same-attempt", "root", 30, 10);
            let a_reserved = store
                .reserve_solution_charge(a.input(1500), charge.clone())
                .unwrap();
            assert!(
                store
                    .reserve_solution_charge(b.input(1500), charge.clone())
                    .unwrap()
                    .newly_reserved
            );
            let foreign = SolutionBudgetInput {
                scope: b.input(1500).scope,
                ..a.input(1500)
            };
            assert!(store
                .settle_solution_charge(foreign, &charge, 10, 30)
                .is_err());
            let overrun = store
                .settle_solution_charge(a.input(1500), &charge, 10, 120)
                .unwrap();
            assert!(matches!(
                overrun.status,
                SolutionChargeStatus::Settled {
                    overrun: true,
                    cost_microusd: 120,
                    ..
                }
            ));
            assert!(store
                .reserve_solution_charge(
                    a.input(86_400_001),
                    BudgetFixture::charge("new-day", "root-b", 0, 0)
                )
                .is_err());
            assert_eq!(
                store
                    .settle_solution_charge(a.input(1500), &charge, 10, 120)
                    .unwrap(),
                overrun
            );
            assert_eq!(
                a_reserved.reservation.status,
                SolutionChargeStatus::Reserved
            );
            assert_eq!(
                store
                    .settle_solution_charge(b.input(1500), &charge, 2, 10)
                    .unwrap()
                    .status,
                SolutionChargeStatus::Settled {
                    tokens: 2,
                    cost_microusd: 10,
                    overrun: false
                }
            );
        })
    });
}

#[test]
#[serial]
fn solution_budget_partial_record_rollback_and_missing_heads_are_detected() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "corruption", 2);
            let charge = BudgetFixture::charge("first", "root", 60, 20);
            store
                .reserve_solution_charge(fixture.input(1500), charge.clone())
                .unwrap();
            store
                .settle_solution_charge(fixture.input(1500), &charge, 20, 40)
                .unwrap();
            store.with_connection(|connection| {
            let ciphertext: String = connection.query_row("SELECT record_json FROM solution_budget_versions WHERE record_key='global' AND generation=1", [], |row| row.get(0))?;
            assert!(crate::encrypted_file_store::is_encrypted_payload(&ciphertext));
            connection.execute("UPDATE solution_budget_records SET generation=1,record_json=?1 WHERE record_key='global'", params![ciphertext])?;
            Ok(())
        }).unwrap();
            assert!(store
                .reserve_solution_charge(
                    fixture.input(1500),
                    BudgetFixture::charge("new", "root", 60, 20)
                )
                .is_err());
            store
                .with_connection(|connection| {
                    connection.execute(
                        "DELETE FROM solution_budget_records WHERE record_key='global'",
                        [],
                    )?;
                    Ok(())
                })
                .unwrap();
            assert!(store
                .reserve_solution_charge(
                    fixture.input(1500),
                    BudgetFixture::charge("new", "root", 60, 20)
                )
                .is_err());
        })
    });
}

#[test]
#[serial]
fn solution_budget_v7_upgrade_is_atomic_and_preserves_existing_installation() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = BudgetFixture::new(store, "a", "upgrade", 2);
            let installation = fixture.installation.read(store);
            store
                .with_connection(|connection| {
                    connection.execute_batch(
                        "DROP TABLE solution_budget_records; DROP TABLE solution_budget_versions;
                UPDATE schema_metadata SET schema_version=7;
                CREATE TABLE solution_budget_versions (broken TEXT);",
                    )?;
                    Ok(())
                })
                .unwrap();
            assert!(store.initialize().is_err());
            store
                .with_connection(|connection| {
                    let version: i64 = connection.query_row(
                        "SELECT schema_version FROM schema_metadata",
                        [],
                        |row| row.get(0),
                    )?;
                    assert_eq!(version, 7);
                    connection.execute_batch("DROP TABLE solution_budget_versions;")?;
                    Ok(())
                })
                .unwrap();
            // If the failed migration partially created the current table, this
            // retry fails. Both schema version and DDL must roll back together.
            store.initialize().unwrap();
            assert_eq!(fixture.installation.read(store), installation);
            assert!(
                store
                    .reserve_solution_charge(
                        fixture.input(1500),
                        BudgetFixture::charge("after-upgrade", "root", 30, 10)
                    )
                    .unwrap()
                    .newly_reserved
            );
        })
    });
}

#[cfg(feature = "storage-postgres")]
pub(crate) struct TransferBudget {
    fixture: BudgetFixture,
    reserved: SolutionChargeIntent,
}

#[cfg(feature = "storage-postgres")]
pub(crate) fn seed_budget_for_transfer(store: &OrchestrationStateStore) -> TransferBudget {
    encrypted(|| {
        let fixture = BudgetFixture::new(store, "b", "budget-transfer", 2);
        let first = BudgetFixture::charge("finished", "root-a", 60, 20);
        store
            .reserve_solution_charge(fixture.input(1500), first.clone())
            .unwrap();
        store
            .settle_solution_charge(fixture.input(1500), &first, 10, 40)
            .unwrap();
        let reserved = BudgetFixture::charge("interrupted", "root-b", 60, 20);
        store
            .reserve_solution_charge(fixture.input(1500), reserved.clone())
            .unwrap();
        TransferBudget { fixture, reserved }
    })
}

#[cfg(feature = "storage-postgres")]
pub(crate) fn assert_budget_after_transfer(
    store: &OrchestrationStateStore,
    saved: &TransferBudget,
) {
    encrypted(|| {
        let repeated = store
            .reserve_solution_charge(saved.fixture.input(1500), saved.reserved.clone())
            .unwrap();
        assert!(!repeated.newly_reserved);
        assert_eq!(repeated.reservation.status, SolutionChargeStatus::Reserved);
        assert!(store
            .reserve_solution_charge(
                saved.fixture.input(1500),
                BudgetFixture::charge("overspend", "third-root", 1, 1)
            )
            .is_err());
    })
}
