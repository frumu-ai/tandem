// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{BTreeMap, BTreeSet};

use serial_test::serial;
use tandem_solutions::*;

use super::backend_conformance_tests::for_each_backend;
use super::customer_config_tests::Fixture;
use super::*;
use crate::stateful_runtime::backend::{params, Executor, ExecutorRaw};

#[path = "solution_budget_tests.rs"]
pub(super) mod budget_tests;

#[derive(Clone)]
struct InstallationFixture {
    customer: Fixture,
    models: BTreeMap<String, ModelBinding>,
    artifacts: BTreeMap<String, Vec<u8>>,
    readiness: BTreeSet<String>,
    host_facts: Option<String>,
}

#[cfg(feature = "storage-postgres")]
pub(super) fn seed_protected_installation_for_transfer(
    store: &OrchestrationStateStore,
) -> SolutionInstallation {
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x67; 32]),
        None,
        async {
            let fixture = InstallationFixture::new("b");
            let (config, digest) = fixture.seed(store);
            store
                .transition_solution_installation(
                    fixture.input(&config, &digest),
                    None,
                    SolutionInstallationTransition::Begin,
                )
                .unwrap();
            store
                .transition_solution_installation(
                    fixture.input(&config, &digest),
                    Some(1),
                    SolutionInstallationTransition::Claim {
                        component_id: "central-brain",
                        attempt_id: "interrupted",
                    },
                )
                .unwrap()
        },
    ))
}

#[cfg(feature = "storage-postgres")]
pub(super) fn assert_protected_installation_after_transfer(
    store: &OrchestrationStateStore,
    expected: &SolutionInstallation,
) {
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x67; 32]),
        None,
        async {
            assert_eq!(&InstallationFixture::new("b").read(store), expected);
            store
                .with_connection(|connection| {
                    let count: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM solution_installation_versions WHERE org_id='org-b' AND instance_id=?1",
                        [&expected.plan.instance_id],
                        |row| row.get(0),
                    )?;
                    assert_eq!(count, 2);
                    Ok(())
                })
                .unwrap();
        },
    ));
}

impl InstallationFixture {
    fn new(customer: &str) -> Self {
        let mut fixture = Fixture::new(customer);
        fixture
            .config
            .optional_components
            .insert("review-notes".into());
        Self {
            customer: fixture,
            host_facts: None,
            models: [("local.fixture".into(), ModelBinding {
                provider: "local".into(), model: "synthetic-model".into(),
                credential_ref: "secret-ref:local-fixture".into(), uses_network: false,
            })].into(),
            artifacts: [
                ("central-brain".into(), include_bytes!(
                    "../../../../tandem-solutions/fixtures/company-brain-text/agents/central-brain.json").to_vec()),
                ("review-notes".into(), include_bytes!(
                    "../../../../tandem-solutions/fixtures/company-brain-text/routines/review-notes.json").to_vec()),
            ].into(),
            readiness: ["governed-memory".into(), "verified-user-context".into()].into(),
        }
    }

    fn configuration(&self) -> CustomerConfigInput<'_> {
        let fixture = &self.customer;
        CustomerConfigInput {
            verified_context: &fixture.context,
            selected_scope: &fixture.config.scope,
            now_ms: 1500,
            current_revision: None,
            expected_revision: None,
            host_policy: &fixture.blueprint.constraints,
            approved_references: &fixture.refs,
            approved_connectors: &fixture.connectors,
            approved_subjects: &fixture.subjects,
            approved_org_units: &fixture.units,
            approved_projects: &fixture.projects,
        }
    }

    fn seed(&self, store: &OrchestrationStateStore) -> (StoredCustomerConfig, String) {
        let config = self
            .customer
            .save(store, &self.customer.config, None)
            .unwrap();
        let prepared = prepare_customer_config(
            &self.customer.blueprint,
            &self.customer.config,
            self.configuration(),
        )
        .unwrap();
        let plan = resolve(
            &self.customer.blueprint,
            ResolutionInput {
                host_facts_sha256: self.host_facts.as_deref(),
                request: &prepared.request,
                verified_context: &self.customer.context,
                now_ms: 1500,
                engine_version: "0.7.2",
                deployment_policy: &prepared.deployment_policy,
                available_deployment_requirements: &self.readiness,
                approved_models: &self.models,
                artifacts: &self.artifacts,
            },
        )
        .unwrap();
        (config, plan.composition_hash().unwrap())
    }

    fn input<'a>(
        &'a self,
        config: &'a StoredCustomerConfig,
        digest: &'a str,
    ) -> SolutionInstallationInput<'a> {
        SolutionInstallationInput {
            host_facts_sha256: self.host_facts.as_deref(),
            configuration: self.configuration(),
            expected_config: &config.version,
            blueprint: &self.customer.blueprint,
            engine_version: "0.7.2",
            available_deployment_requirements: &self.readiness,
            approved_models: &self.models,
            artifacts: &self.artifacts,
            reviewed_composition: digest,
        }
    }

    fn read(&self, store: &OrchestrationStateStore) -> SolutionInstallation {
        store
            .solution_installation(&self.customer.context, &self.customer.config.scope, 1500)
            .unwrap()
            .unwrap()
    }
}

#[test]
#[serial]
fn solution_installation_resume_requires_reconciliation_and_keeps_dependency_order() {
    for_each_backend(|_, store| {
        let fixture = InstallationFixture::new("a");
        let (config, digest) = fixture.seed(store);
        let begin = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        assert_eq!(begin.generation, 1);
        assert!(!begin.all_components_staged());
        assert_eq!(
            begin,
            store
                .transition_solution_installation(
                    fixture.input(&config, &digest),
                    None,
                    SolutionInstallationTransition::Begin
                )
                .unwrap()
        );
        assert!(store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(1),
                SolutionInstallationTransition::Claim {
                    component_id: "review-notes",
                    attempt_id: "child"
                }
            )
            .is_err());
        let claimed = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(1),
                SolutionInstallationTransition::Claim {
                    component_id: "central-brain",
                    attempt_id: "attempt-1",
                },
            )
            .unwrap();
        // Simulate an effect followed by process loss before RecordStaged.
        // A fresh store handle opens the same durable database, no in-memory receipt.
        let restarted = store.clone();
        restarted.initialize().unwrap();
        assert_eq!(fixture.read(&restarted), claimed);
        for attempt in ["attempt-1", "takeover"] {
            assert!(restarted
                .transition_solution_installation(
                    fixture.input(&config, &digest),
                    Some(2),
                    SolutionInstallationTransition::Claim {
                        component_id: "central-brain",
                        attempt_id: attempt
                    }
                )
                .is_err());
        }
        assert!(restarted
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(2),
                SolutionInstallationTransition::RecordStaged {
                    component_id: "central-brain",
                    attempt_id: "wrong",
                    resource_sha256: &"a".repeat(64)
                }
            )
            .is_err());
        let staged = restarted
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(2),
                SolutionInstallationTransition::RecordStaged {
                    component_id: "central-brain",
                    attempt_id: "attempt-1",
                    resource_sha256: &"a".repeat(64),
                },
            )
            .unwrap();
        assert!(!staged.all_components_staged());
        restarted
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(3),
                SolutionInstallationTransition::Claim {
                    component_id: "review-notes",
                    attempt_id: "child",
                },
            )
            .unwrap();
        let complete = restarted
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(4),
                SolutionInstallationTransition::RecordStaged {
                    component_id: "review-notes",
                    attempt_id: "child",
                    resource_sha256: &"b".repeat(64),
                },
            )
            .unwrap();
        assert!(complete.all_components_staged());
        assert_eq!(complete.generation, 5);
        assert_eq!(complete.plan.components, begin.plan.components);
    });
}

#[test]
#[serial]
fn solution_installation_concurrent_claims_have_exactly_one_winner() {
    for_each_backend(|_, store| {
        let fixture = InstallationFixture::new("a");
        let (config, digest) = fixture.seed(store);
        store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let run = |attempt| {
                barrier.wait();
                store
                    .transition_solution_installation(
                        fixture.input(&config, &digest),
                        Some(1),
                        SolutionInstallationTransition::Claim {
                            component_id: "central-brain",
                            attempt_id: attempt,
                        },
                    )
                    .is_ok()
            };
            let first = scope.spawn(move || run("first"));
            let second = scope.spawn(move || run("second"));
            [first.join().unwrap(), second.join().unwrap()]
        });
        assert_eq!(results.into_iter().filter(|result| *result).count(), 1);
        assert_eq!(fixture.read(store).generation, 2);
    });
}

#[test]
#[serial]
fn solution_installation_current_authority_and_bindings_are_rechecked_on_retries() {
    for_each_backend(|_, store| {
        let fixture = InstallationFixture::new("a");
        let (config, digest) = fixture.seed(store);
        let original = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        let mut revoked = fixture.clone();
        revoked.customer.refs.clear();
        assert!(store
            .transition_solution_installation(
                revoked.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin
            )
            .is_err());
        let mut changed_model = fixture.clone();
        changed_model.models.get_mut("local.fixture").unwrap().model = "replacement".into();
        assert!(store
            .transition_solution_installation(
                changed_model.input(&config, &digest),
                Some(1),
                SolutionInstallationTransition::Claim {
                    component_id: "central-brain",
                    attempt_id: "attempt"
                }
            )
            .is_err());
        let mut unavailable = fixture.clone();
        unavailable.readiness.clear();
        assert!(store
            .transition_solution_installation(
                unavailable.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin
            )
            .is_err());
        let mut expired = fixture.input(&config, &digest);
        expired.configuration.now_ms = 2001;
        assert!(store
            .transition_solution_installation(expired, None, SolutionInstallationTransition::Begin)
            .is_err());
        assert!(store
            .transition_solution_installation(
                fixture.input(&config, &"f".repeat(64)),
                None,
                SolutionInstallationTransition::Begin
            )
            .is_err());
        let mut changed_policy = fixture.clone();
        changed_policy
            .customer
            .blueprint
            .constraints
            .max_tokens_per_run = 1;
        assert!(store
            .transition_solution_installation(
                changed_policy.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin
            )
            .is_err());
        assert_eq!(fixture.read(store), original);
    });
}

#[test]
#[serial]
fn solution_installation_configuration_aba_blocks_old_preview() {
    for_each_backend(|_, store| {
        let fixture = InstallationFixture::new("a");
        let (first, digest) = fixture.seed(store);
        store
            .transition_solution_installation(
                fixture.input(&first, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        let mut edited = fixture.customer.config.clone();
        edited.timezone = "UTC".into();
        let second = fixture
            .customer
            .save(store, &edited, Some(&first.version))
            .unwrap();
        let restored = fixture
            .customer
            .save(store, &fixture.customer.config, Some(&second.version))
            .unwrap();
        assert_eq!(restored.version.sha256, first.version.sha256);
        assert!(store
            .transition_solution_installation(
                fixture.input(&first, &digest),
                Some(1),
                SolutionInstallationTransition::Claim {
                    component_id: "central-brain",
                    attempt_id: "attempt"
                }
            )
            .is_err());
        // A new configuration generation also cannot silently replace an existing intent.
        assert!(store
            .transition_solution_installation(
                fixture.input(&restored, &digest),
                None,
                SolutionInstallationTransition::Begin
            )
            .is_err());
        assert_eq!(fixture.read(store).generation, 1);
    });
}

#[test]
#[serial]
fn solution_installation_history_failure_rolls_back_claim() {
    for_each_backend(|_, store| {
        let fixture = InstallationFixture::new("a");
        let (config, digest) = fixture.seed(store);
        let original = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        store.with_connection(|connection| {
            connection.execute("INSERT INTO solution_installation_versions
                (org_id,workspace_id,deployment_id,instance_id,generation,record_json)
                SELECT org_id,workspace_id,deployment_id,instance_id,2,record_json FROM solution_installations", [])?;
            Ok(())
        }).unwrap();
        assert!(store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(1),
                SolutionInstallationTransition::Claim {
                    component_id: "central-brain",
                    attempt_id: "attempt"
                }
            )
            .is_err());
        assert_eq!(fixture.read(store), original);
    });
}

#[test]
#[serial]
fn solution_installation_encrypted_two_tenant_progress_rejects_substitution() {
    futures::executor::block_on(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x67; 32]),
        None,
        async {
            for_each_backend(|_, store| {
                let a = InstallationFixture::new("a");
                let b = InstallationFixture::new("b");
                let (a_config, a_digest) = a.seed(store);
                let (b_config, b_digest) = b.seed(store);
                let a_record = store
                    .transition_solution_installation(
                        a.input(&a_config, &a_digest),
                        None,
                        SolutionInstallationTransition::Begin,
                    )
                    .unwrap();
                let b_record = store
                    .transition_solution_installation(
                        b.input(&b_config, &b_digest),
                        None,
                        SolutionInstallationTransition::Begin,
                    )
                    .unwrap();
                assert_ne!(
                    a_record.plan.components["central-brain"].resource_id,
                    b_record.plan.components["central-brain"].resource_id
                );
                assert!(store
                    .solution_installation(&b.customer.context, &a.customer.config.scope, 1500)
                    .is_err());
                store
                    .with_connection(|connection| {
                        let payload: String = connection.query_row(
                            "SELECT record_json FROM solution_installations WHERE org_id='org-a'",
                            [],
                            |row| row.get(0),
                        )?;
                        assert!(crate::encrypted_file_store::is_encrypted_payload(&payload));
                        assert!(!payload.contains("synthetic-model"));
                        connection.execute(
                            "UPDATE solution_installations SET record_json=?1 WHERE org_id='org-b'",
                            params![payload],
                        )?;
                        Ok(())
                    })
                    .unwrap();
                assert!(store
                    .solution_installation(&b.customer.context, &b.customer.config.scope, 1500)
                    .is_err());
                assert_eq!(a.read(store), a_record);
            });
        },
    ));
}

#[test]
#[serial]
fn solution_installation_schema_upgrade_preserves_configuration() {
    for_each_backend(|_, store| {
        let fixture = InstallationFixture::new("a");
        let (config, digest) = fixture.seed(store);
        store
            .with_connection(|connection| {
                connection.execute_batch(
                    "DROP TABLE solution_installation_versions;
                DROP TABLE solution_installations;
                DROP TABLE solution_budget_records; DROP TABLE solution_budget_versions;
                UPDATE schema_metadata SET schema_version=6;",
                )?;
                Ok(())
            })
            .unwrap();
        store.initialize().unwrap();
        assert_eq!(
            store
                .customer_configuration(
                    &fixture.customer.context,
                    &fixture.customer.config.scope,
                    1500
                )
                .unwrap()
                .unwrap(),
            config
        );
        let record = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        store.initialize().unwrap();
        assert_eq!(fixture.read(store), record);
    });
}

#[test]
#[serial]
fn solution_installation_rejects_host_rebinding_before_claim_or_receipt() {
    for_each_backend(|_, store| {
        let mut fixture = InstallationFixture::new("a");
        fixture.host_facts = Some(sha256(b"approved endpoint and source revision 1"));
        let (config, digest) = fixture.seed(store);
        let begin = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                None,
                SolutionInstallationTransition::Begin,
            )
            .unwrap();
        let mut rebound = fixture.clone();
        rebound.host_facts = Some(sha256(b"same IDs, changed endpoint or source"));
        assert!(store
            .transition_solution_installation(
                rebound.input(&config, &digest),
                Some(begin.generation),
                SolutionInstallationTransition::Claim {
                    component_id: "central-brain",
                    attempt_id: "one"
                },
            )
            .unwrap_err()
            .to_string()
            .contains("preview is stale"));
        assert_eq!(fixture.read(store), begin);
        let claimed = store
            .transition_solution_installation(
                fixture.input(&config, &digest),
                Some(begin.generation),
                SolutionInstallationTransition::Claim {
                    component_id: "central-brain",
                    attempt_id: "one",
                },
            )
            .unwrap();
        let receipt = sha256(b"disabled native component");
        for host_facts in [rebound.host_facts.clone(), None] {
            rebound.host_facts = host_facts;
            assert!(store
                .transition_solution_installation(
                    rebound.input(&config, &digest),
                    Some(claimed.generation),
                    SolutionInstallationTransition::RecordStaged {
                        component_id: "central-brain",
                        attempt_id: "one",
                        resource_sha256: &receipt,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("preview is stale"));
            assert_eq!(fixture.read(store), claimed);
        }
    });
}
