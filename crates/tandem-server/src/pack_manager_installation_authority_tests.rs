// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::time::Duration;

const COMPONENTS: [&str; 2] = ["central-brain", "review-notes"];

async fn component_fixture(component: &str) -> Fixture {
    let mut entries = fixture();
    if component == "review-notes" {
        // Keep the real signed routine artifact and installer, while isolating
        // this sink from the preceding template component's writer.
        let blueprint = entries
            .iter_mut()
            .find(|(path, _)| path == "solution.json")
            .unwrap();
        let mut document: serde_json::Value = serde_json::from_str(&blueprint.1).unwrap();
        document["components"]
            .as_object_mut()
            .unwrap()
            .remove("central-brain");
        document["components"]["review-notes"]
            .as_object_mut()
            .unwrap()
            .remove("depends_on");
        blueprint.1 = serde_json::to_string(&document).unwrap();
        entries.retain(|(path, _)| path != "agents/central-brain.json");
    }
    let mut fixture = Fixture::new_with_entries(entries).await;
    if component == "central-brain" {
        fixture
            .configuration
            .configuration
            .optional_components
            .clear();
    } else {
        // The isolated routine declares no model class. Global memory spaces
        // and approved references remain declared by the signed blueprint.
        fixture.configuration.configuration.models.clear();
    }
    fixture
}

async fn hold_writer(state: &AppState, component: &str) -> tokio::sync::OwnedMutexGuard<()> {
    if component == "central-brain" {
        state
            .agent_teams
            .lock_solution_template_writer_for_test()
            .await
    } else {
        state.routine_persistence.clone().lock_owned().await
    }
}

fn resource_id(request: &SolutionStagingRequest, component: &str) -> String {
    tandem_solutions::solution_resource_id(
        &request.scope.org_id,
        &request.scope.workspace_id,
        &request.scope.deployment_id,
        &request.scope.instance_id,
        component,
    )
    .unwrap()
}

fn native_exists(fixture: &Fixture, request: &SolutionStagingRequest, component: &str) -> bool {
    if component == "central-brain" {
        fixture
            .root
            .path()
            .join(".tandem/agent-team/templates")
            .join(format!("{}.yaml", resource_id(request, component)))
            .is_file()
    } else {
        fixture.state.routines_path.is_file()
    }
}

async fn wait_for_claim(fixture: &Fixture, request: &SolutionStagingRequest, component: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let claimed = OrchestrationStateStore::from_automation_runs_path(
                &fixture.state.automation_v2_runs_path,
            )
            .expect("the real staging store must open")
            .solution_installation(&fixture.verified, &request.scope, crate::now_ms())
            .expect("the controller must be authorized to read real staging progress")
            .is_some_and(|journal| {
                matches!(
                    journal.components.get(component),
                    Some(SolutionComponentProgress::Claimed { .. })
                )
            });
            if claimed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("real staging must claim the native component");
}

async fn stage_with_controller<F, C>(stage: F, controller: C) -> F::Output
where
    F: std::future::Future,
    F::Output: std::fmt::Debug,
    C: std::future::Future<Output = ()>,
{
    tokio::pin!(stage);
    tokio::pin!(controller);
    tokio::time::timeout(Duration::from_secs(20), async {
        tokio::select! {
            biased;
            () = &mut controller => stage.await,
            result = &mut stage => panic!(
                "staging returned before the controller released its native writer: {result:?}"
            ),
        }
    })
    .await
    .expect("the admitted native staging attempt must finish after its writer is released")
}

async fn wait_for_guard(fixture: &Fixture, request: &SolutionStagingRequest, component: &str) {
    wait_for_claim(fixture, request, component).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fixture
            .state
            .enterprise
            .hosted_policy
            .publication_mutex_locked_for_test()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native commit must retain publication authority while its writer waits");
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn native_staging_rejects_revocation_published_while_target_writer_is_held() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            for component in COMPONENTS {
                let fixture = component_fixture(component).await;
                let request = fixture.review_and_save().await;
                let writer = hold_writer(&fixture.state, component).await;
                let publication = fixture.state.lock_hosted_policy_publication().await;
                fixture.write_policy(2, false);
                // Queue the real publisher first. Staging can authorize and
                // claim against policy 1 while both commits await this guard.
                let mut revoke = Box::pin(fixture.state.reload_hosted_policy());
                assert!(futures::poll!(&mut revoke).is_pending());
                let stage = fixture
                    .state
                    .stage_solution_installation(&fixture.verified, request.clone());
                let controller = async {
                    wait_for_claim(&fixture, &request, component).await;
                    drop(publication);
                    revoke.await.unwrap();
                    assert!(!native_exists(&fixture, &request, component));
                    drop(writer);
                };
                let result = stage_with_controller(stage, controller).await;
                assert!(result.is_err(), "revoked {component} staging must fail");
                assert!(!native_exists(&fixture, &request, component));
            }
        },
    )
    .await;
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn native_staging_rechecks_assertion_expiry_after_target_writer_wait() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            for component in COMPONENTS {
                let mut fixture = component_fixture(component).await;
                // Hosted grants inherit this expiry and enter the reviewed
                // authority hash. Review the same identity that will stage.
                fixture.verified.expires_at_ms = crate::now_ms() + 5_000;
                let request = fixture.review_and_save().await;
                let writer = hold_writer(&fixture.state, component).await;
                let stage = fixture
                    .state
                    .stage_solution_installation(&fixture.verified, request.clone());
                let controller = async {
                    wait_for_guard(&fixture, &request, component).await;
                    let remaining = fixture
                        .verified
                        .expires_at_ms
                        .saturating_sub(crate::now_ms());
                    tokio::time::sleep(Duration::from_millis(remaining + 1)).await;
                    drop(writer);
                };
                let result = stage_with_controller(stage, controller).await;
                assert!(
                    result.is_err(),
                    "expired {component} staging must fail after its wait"
                );
                assert!(!native_exists(&fixture, &request, component));
                if component == "review-notes" {
                    assert!(fixture.state.routines.read().await.is_empty());
                }
            }
        },
    )
    .await;
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn cancelled_native_staging_retains_guard_through_commit_or_routine_rollback() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            for (component, fail_persist) in [
                ("central-brain", false),
                ("review-notes", false),
                ("review-notes", true),
            ] {
                let fixture = component_fixture(component).await;
                let request = fixture.review_and_save().await;
                let writer = hold_writer(&fixture.state, component).await;
                if fail_persist {
                    std::fs::create_dir(crate::config::paths::sibling_tmp_path(
                        &fixture.state.routines_path,
                    ))
                    .unwrap();
                }
                let state = fixture.state.clone();
                let actor = fixture.verified.clone();
                let intent = request.clone();
                let mut stage =
                    tokio::spawn(crate::encrypted_file_store::with_test_crypto_provider(
                        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
                        None,
                        async move { state.stage_solution_installation(&actor, intent).await },
                    ));
                tokio::select! {
                    biased;
                    () = wait_for_guard(&fixture, &request, component) => (),
                    result = &mut stage => panic!(
                        "staging returned before cancellation at the native writer: {result:?}"
                    ),
                }
                stage.abort();
                assert!(stage.await.unwrap_err().is_cancelled());
                fixture.write_policy(2, false);
                let mut revoke = Box::pin(fixture.state.reload_hosted_policy());
                assert!(
                    futures::poll!(&mut revoke).is_pending(),
                    "cancelling the waiter must not release the native commit's guard"
                );
                assert!(!native_exists(&fixture, &request, component));
                drop(writer);
                tokio::time::timeout(Duration::from_secs(10), revoke)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    native_exists(&fixture, &request, component),
                    !fail_persist,
                    "the real publisher may finish only after native persistence or rollback"
                );
                if component == "central-brain" {
                    let path = fixture
                        .root
                        .path()
                        .join(".tandem/agent-team/templates")
                        .join(format!("{}.yaml", resource_id(&request, component)));
                    let template: tandem_orchestrator::AgentTemplate =
                        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                    assert!(!template.enabled);
                } else if fail_persist {
                    assert!(
                        fixture.state.routines.read().await.is_empty(),
                        "the detached commit must still roll back cached insertion on failure"
                    );
                } else {
                    let rows: std::collections::HashMap<
                        String,
                        crate::routines::types::RoutineSpec,
                    > = serde_json::from_slice(
                        &std::fs::read(&fixture.state.routines_path).unwrap(),
                    )
                    .unwrap();
                    let routine = rows
                        .values()
                        .find(|row| row.routine_id == resource_id(&request, component))
                        .unwrap();
                    assert!(routine.installation_disabled());
                    assert_eq!(
                        routine.status,
                        crate::routines::types::RoutineStatus::Paused
                    );
                    assert!(routine.next_fire_at_ms.is_none());
                }
            }
        },
    )
    .await;
}
