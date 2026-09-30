// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#![cfg(feature = "storage-postgres")]

use super::*;
use crate::stateful_runtime::backend::{postgres::migrate_schema_v7_to_v8, Connection};

fn migrate_after_observation(
    mut connection: Connection,
    observed_version: i64,
    barrier: &Barrier,
) -> anyhow::Result<(i64, i64)> {
    barrier.wait();
    let migrated_version = migrate_schema_v7_to_v8(&mut connection, observed_version)?;
    let persisted_version = connection.query_row(
        "SELECT schema_version FROM schema_metadata LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    Ok((migrated_version, persisted_version))
}

#[test]
#[serial]
fn solution_budget_v7_concurrent_postgres_migration_rechecks_locked_version() {
    encrypted(|| {
        for_each_backend(|name, store| {
            if name != "postgres" {
                return;
            }
            let fixture = BudgetFixture::new(store, "a", "concurrent-upgrade", 2);
            let installation = fixture.installation.read(store);
            store
                .with_connection(|connection| {
                    connection.execute_batch(
                        "DROP TABLE solution_budget_records;
                         DROP TABLE solution_budget_versions;
                         UPDATE schema_metadata SET schema_version=7;",
                    )?;
                    Ok(())
                })
                .unwrap();

            // Keep both native connections checked out and observe v7 on each
            // before releasing either migration. This deterministically
            // reproduces two initializers making the same pre-lock decision.
            let first = store.open_connection().unwrap();
            let second = store.open_connection().unwrap();
            let first_pid: i64 = first
                .query_row("SELECT pg_backend_pid()", [], |row| row.get(0))
                .unwrap();
            let second_pid: i64 = second
                .query_row("SELECT pg_backend_pid()", [], |row| row.get(0))
                .unwrap();
            assert_ne!(first_pid, second_pid);
            let first_version: i64 = first
                .query_row(
                    "SELECT schema_version FROM schema_metadata LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let second_version: i64 = second
                .query_row(
                    "SELECT schema_version FROM schema_metadata LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!((first_version, second_version), (7, 7));

            let barrier = Barrier::new(2);
            let (first_result, second_result) = std::thread::scope(|scope| {
                let first =
                    scope.spawn(|| migrate_after_observation(first, first_version, &barrier));
                let second =
                    scope.spawn(|| migrate_after_observation(second, second_version, &barrier));
                (
                    first.join().expect("first migration worker panicked"),
                    second.join().expect("second migration worker panicked"),
                )
            });
            assert_eq!(first_result.unwrap(), (8, 8));
            assert_eq!(second_result.unwrap(), (8, 8));

            store.initialize().unwrap();
            assert_eq!(fixture.installation.read(store), installation);
            store
                .with_connection(|connection| {
                    let tables: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM information_schema.tables
                         WHERE table_schema=current_schema()
                           AND table_name IN ('solution_budget_records', 'solution_budget_versions')",
                        [],
                        |row| row.get(0),
                    )?;
                    assert_eq!(tables, 2);
                    Ok(())
                })
                .unwrap();
            assert!(
                store
                    .reserve_solution_charge(
                        fixture.input(1500),
                        BudgetFixture::charge("after-concurrent-upgrade", "root", 30, 10),
                    )
                    .unwrap()
                    .newly_reserved
            );
        });
    });
}
