// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{BTreeMap, BTreeSet};

use serial_test::serial;
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
    VerifiedTenantContext,
};
use tandem_solutions::*;

use super::backend_conformance_tests::for_each_backend;
use super::{CustomerConfigVersion, OrchestrationStateStore, StoredCustomerConfig};
use crate::stateful_runtime::backend::{params, Executor, ExecutorRaw};

#[derive(Clone)]
pub(super) struct Fixture {
    pub(super) blueprint: SolutionBlueprint,
    pub(super) config: CustomerConfig,
    pub(super) context: VerifiedTenantContext,
    pub(super) refs: BTreeSet<String>,
    pub(super) subjects: BTreeSet<String>,
    pub(super) projects: BTreeSet<String>,
    pub(super) units: BTreeSet<String>,
    pub(super) connectors: BTreeMap<String, ConnectorBinding>,
}

impl Fixture {
    pub(super) fn new(customer: &str) -> Self {
        let document = match customer {
            "a" => include_str!(
                "../../../../tandem-solutions/fixtures/company-brain-text/customer-a.yaml"
            ),
            "b" => include_str!(
                "../../../../tandem-solutions/fixtures/company-brain-text/customer-b.yaml"
            ),
            _ => panic!("unknown synthetic customer"),
        };
        let actor = format!("owner-{customer}");
        let mut config = parse_customer_config(document).unwrap();
        config.scope.instance_id = "same-installation-name".into();
        Self {
            blueprint: parse_blueprint(include_str!(
                "../../../../tandem-solutions/fixtures/company-brain-text/solution.json"
            ))
            .unwrap(),
            config,
            context: TenantContextAssertionClaims::new_v1(
                "issuer",
                "runtime",
                1000,
                2000,
                "synthetic-assertion",
                TenantContext::explicit_user_workspace(
                    format!("org-{customer}"),
                    format!("workspace-{customer}"),
                    Some(format!("deployment-{customer}")),
                    &actor,
                ),
                HumanActor::tandem_user(&actor),
                AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                    &actor, "fixture",
                )),
                vec!["workspace:user".into()],
            )
            .into(),
            refs: [
                format!("profile-ref:synthetic-company-{customer}"),
                format!("data-ref:synthetic-notes-{customer}"),
            ]
            .into(),
            subjects: [actor].into(),
            projects: [format!("project-{customer}")].into(),
            units: BTreeSet::new(),
            connectors: BTreeMap::new(),
        }
    }

    pub(super) fn save(
        &self,
        store: &OrchestrationStateStore,
        config: &CustomerConfig,
        expected: Option<&CustomerConfigVersion>,
    ) -> anyhow::Result<StoredCustomerConfig> {
        store.save_customer_configuration(
            &self.blueprint,
            config,
            CustomerConfigInput {
                verified_context: &self.context,
                selected_scope: &self.config.scope,
                now_ms: 1500,
                current_revision: None,
                expected_revision: expected.map(|value| value.sha256.as_str()),
                host_policy: &self.blueprint.constraints,
                approved_references: &self.refs,
                approved_subjects: &self.subjects,
                approved_org_units: &self.units,
                approved_projects: &self.projects,
                approved_connectors: &self.connectors,
            },
            expected,
        )
    }

    fn read(&self, store: &OrchestrationStateStore) -> StoredCustomerConfig {
        store
            .customer_configuration(&self.context, &self.config.scope, 1500)
            .unwrap()
            .unwrap()
    }
}

fn reopened(store: &OrchestrationStateStore) -> OrchestrationStateStore {
    let reopened = store.clone();
    reopened.initialize().unwrap();
    reopened
}

#[cfg(feature = "storage-postgres")]
pub(super) fn seed_protected_config_for_transfer(
    store: &OrchestrationStateStore,
) -> StoredCustomerConfig {
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x5a; 32]),
        None,
        async {
            let fixture = Fixture::new("a");
            let stored = fixture.save(store, &fixture.config, None).unwrap();
            store
                .with_connection(|connection| {
                    let payload: String = connection.query_row(
                        "SELECT record_json FROM solution_customer_configs WHERE org_id='org-a'",
                        [],
                        |row| row.get(0),
                    )?;
                    assert!(crate::encrypted_file_store::is_encrypted_payload(&payload));
                    assert!(!payload.contains(&fixture.config.profile_ref));
                    Ok(())
                })
                .unwrap();
            stored
        },
    ))
}

#[cfg(feature = "storage-postgres")]
pub(super) fn assert_protected_config_after_transfer(
    store: &OrchestrationStateStore,
    expected: &StoredCustomerConfig,
) {
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x5a; 32]),
        None,
        async {
            assert_eq!(&Fixture::new("a").read(store), expected);
            store.with_connection(|connection| {
                    let versions: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM solution_customer_config_versions WHERE org_id='org-a'",
                        [], |row| row.get(0))?;
                    assert_eq!(versions, 1);
                    Ok(())
                }).unwrap();
        },
    ));
}

#[test]
#[serial]
fn customer_config_two_scopes_persist_without_exporting_customer_data() {
    for_each_backend(|backend, store| {
        let a = Fixture::new("a");
        let b = Fixture::new("b");
        let sa = a.save(store, &a.config, None).unwrap();
        let sb = b.save(store, &b.config, None).unwrap();
        assert_eq!(sa.version.generation, 1, "{backend}");
        assert_eq!(sa.blueprint_sha256, sb.blueprint_sha256);
        assert_ne!(sa.version.sha256, sb.version.sha256);
        let reopened = reopened(store);
        assert_eq!(a.read(&reopened), sa);
        assert_eq!(b.read(&reopened), sb);
        assert!(store
            .customer_configuration(&a.context, &b.config.scope, 1500)
            .is_err());
        assert!(store
            .customer_configuration(&a.context, &a.config.scope, 2001)
            .is_err());
        let mut spoofed = a.config.clone();
        spoofed.scope = b.config.scope.clone();
        assert!(a.save(store, &spoofed, Some(&sa.version)).is_err());
        let export =
            serde_json::to_string(&customer_config_template(&a.blueprint).unwrap()).unwrap();
        for sensitive in [
            &a.config.scope.org_id,
            &b.config.scope.org_id,
            &a.config.profile_ref,
            &b.config.profile_ref,
            &a.config.scope.instance_id,
        ] {
            assert!(!export.contains(sensitive));
        }
        assert_eq!(a.read(store), sa);
        assert_eq!(b.read(store), sb);
    });
}

#[test]
#[serial]
fn customer_config_concurrent_edits_and_aba_reject_stale_versions() {
    for_each_backend(|backend, store| {
        let fixture = Fixture::new("a");
        let first = fixture.save(store, &fixture.config, None).unwrap();
        let mut left = fixture.config.clone();
        left.locale = "en-GB".into();
        let mut right = fixture.config.clone();
        right.locale = "fr-FR".into();
        let outcomes = std::thread::scope(|threads| {
            let l = threads.spawn(|| fixture.save(store, &left, Some(&first.version)));
            let r = threads.spawn(|| fixture.save(store, &right, Some(&first.version)));
            [l.join().unwrap(), r.join().unwrap()]
        });
        assert_eq!(
            outcomes.iter().filter(|result| result.is_ok()).count(),
            1,
            "{backend}"
        );
        let second = fixture.read(store);
        assert_eq!(second.version.generation, 2);
        let third = fixture
            .save(store, &fixture.config, Some(&second.version))
            .unwrap();
        assert_eq!(third.version.sha256, first.version.sha256);
        assert_eq!(third.version.generation, 3);
        assert!(fixture.save(store, &right, Some(&first.version)).is_err());
        assert_eq!(
            fixture
                .save(store, &fixture.config, Some(&third.version))
                .unwrap(),
            third
        );
        assert_eq!(fixture.read(&reopened(store)), third);
    });
}

#[test]
#[serial]
fn customer_config_failed_version_append_rolls_back_current_document() {
    for_each_backend(|backend, store| {
        let fixture = Fixture::new("a");
        let first = fixture.save(store, &fixture.config, None).unwrap();
        let scope = &fixture.config.scope;
        // A duplicate immutable version forces the second insert to fail after
        // the current document update. Neither backend may commit that update.
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO solution_customer_config_versions
                (org_id,workspace_id,deployment_id,instance_id,generation,revision,record_json)
                VALUES (?1,?2,?3,?4,2,'synthetic-conflict','synthetic-conflict')",
                    params![
                        scope.org_id,
                        scope.workspace_id,
                        scope.deployment_id,
                        scope.instance_id
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        let mut next = fixture.config.clone();
        next.locale = "fr-FR".into();
        assert!(
            fixture.save(store, &next, Some(&first.version)).is_err(),
            "{backend}"
        );
        assert_eq!(fixture.read(&reopened(store)), first);
    });
}

#[test]
#[serial]
fn customer_config_noop_still_checks_current_host_bindings() {
    for_each_backend(|backend, store| {
        let mut fixture = Fixture::new("a");
        let first = fixture.save(store, &fixture.config, None).unwrap();
        fixture.refs.clear();
        assert!(
            fixture
                .save(store, &fixture.config, Some(&first.version))
                .is_err(),
            "{backend}: unchanged content cannot reuse revoked reference approval"
        );
        assert_eq!(fixture.read(&reopened(store)), first);
    });
}

#[test]
#[serial]
fn customer_config_protected_payload_rejects_tenant_substitution() {
    // Poll the task-local crypto scope without entering Tokio: the synchronous
    // PostgreSQL client owns its own runtime while these store calls execute.
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x5a; 32]),
        None,
        async {
            for_each_backend(|_, store| {
                let a = Fixture::new("a");
                let b = Fixture::new("b");
                a.save(store, &a.config, None).unwrap();
                b.save(store, &b.config, None).unwrap();
                store
                    .with_connection(|connection| {
                        let payload: String = connection.query_row(
                            "SELECT record_json FROM solution_customer_configs WHERE org_id=?1",
                            params![a.config.scope.org_id],
                            |row| row.get(0),
                        )?;
                        assert!(crate::encrypted_file_store::is_encrypted_payload(&payload));
                        assert!(!payload.contains(&a.config.profile_ref));
                        connection.execute(
                            "UPDATE solution_customer_configs SET record_json=?1 WHERE org_id=?2",
                            params![payload, b.config.scope.org_id],
                        )?;
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(a.read(&reopened(store)).config, a.config);
                assert!(store
                    .customer_configuration(&b.context, &b.config.scope, 1500)
                    .is_err());
            });
        },
    ));
}

#[test]
#[serial]
fn customer_config_schema_upgrade_keeps_existing_runtime_records() {
    for_each_backend(|_, store| {
        store.with_connection(|connection| {
            connection.execute_batch("DROP TABLE solution_installation_versions;
                DROP TABLE solution_installations;
                DROP TABLE solution_budget_records;
                DROP TABLE solution_budget_versions;
                DROP TABLE solution_customer_config_versions;
                DROP TABLE solution_customer_configs;
                UPDATE schema_metadata SET schema_version=5;
                INSERT INTO orchestration_tool_requests
                (org_id,workspace_id,deployment_key,operation,idempotency_key,request_digest,created_at_ms)
                VALUES ('synthetic-org','workspace','deployment','fixture','request','digest',1);")?;
            Ok(())
        }).unwrap();
        let migrated = reopened(store);
        let fixture = Fixture::new("a");
        fixture.save(&migrated, &fixture.config, None).unwrap();
        reopened(&migrated)
            .with_connection(|connection| {
                let count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM orchestration_tool_requests WHERE org_id='synthetic-org'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(count, 1);
                let version: i64 = connection.query_row(
                    "SELECT schema_version FROM schema_metadata",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(version, super::SCHEMA_VERSION);
                Ok(())
            })
            .unwrap();
    });
}
