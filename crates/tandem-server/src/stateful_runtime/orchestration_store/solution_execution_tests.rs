use super::*;
use crate::stateful_runtime::backend::{params, Executor, TransactionBehavior};
use crate::stateful_runtime::orchestration_store::protected_records;
use tandem_automation::{AutomationV2RunRecord, GoalRunLink, LongRunningGoal};

pub(super) fn root_id(fixture: &BudgetFixture) -> String {
    format!(
        "root-{}",
        fixture.installation.customer.config.scope.instance_id
    )
}

fn goal_id(fixture: &BudgetFixture) -> String {
    format!(
        "goal-{}",
        fixture.installation.customer.config.scope.instance_id
    )
}

fn execution_run(fixture: &BudgetFixture, id: &str) -> AutomationV2RunRecord {
    serde_json::from_value(serde_json::json!({
        "run_id": id, "automation_id": "synthetic-worker",
        "tenant_context": fixture.installation.customer.context.tenant_context,
        "trigger_type": "goal_start", "status": "running", "created_at_ms": 1000,
        "updated_at_ms": 1000, "started_at_ms": 1000, "checkpoint": {},
        "execution_claim_epoch": 1,
        "execution_claim": {"claim_id": "claim-1", "claimant_id": "executor-1",
            "claimed_at_ms": 1000, "lease_expires_at_ms": 10000, "lease_epoch": 1}
    }))
    .unwrap()
}

pub(super) fn seed_execution(store: &OrchestrationStateStore, fixture: &BudgetFixture) {
    let root = execution_run(fixture, &root_id(fixture));
    let goal: LongRunningGoal = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "goal_id": goal_id(fixture), "orchestration_id": "synthetic-orchestration",
        "orchestration_version": 1, "objective": "Exercise current runtime lineage",
        "status": "active", "tenant_context": root.tenant_context,
        "policy": {"max_hops": 4, "deadline_at_ms": 10000}, "active_run_id": root.run_id,
        "current_node_id": "work", "hop_count": 0, "total_tokens": 0,
        "total_cost_usd": 0.0, "created_at_ms": 1000, "updated_at_ms": 1000
    })).unwrap();
    let link = GoalRunLink {
        goal_id: goal.goal_id.clone(),
        run_id: root.run_id.clone(),
        orchestration_node_id: "work".into(),
        orchestration_version: 1,
        hop_index: 0,
        parent_run_id: None,
        triggering_handoff_id: None,
        created_at_ms: 1000,
    };
    store
        .start_goal(
            &goal,
            &root,
            &link,
            &tandem_types::PrincipalRef::new(
                tandem_types::PrincipalKind::HumanUser,
                &fixture.installation.customer.context.human_actor.actor_id,
            ),
        )
        .unwrap();
}

fn seed_child(store: &OrchestrationStateStore, fixture: &BudgetFixture) -> String {
    let child_id = format!("child-{}", root_id(fixture));
    let child = execution_run(fixture, &child_id);
    let mut goal = store.get_goal(&goal_id(fixture)).unwrap().unwrap();
    goal.active_run_id = Some(child_id.clone());
    goal.hop_count = 1;
    let link = GoalRunLink {
        goal_id: goal.goal_id.clone(),
        run_id: child_id.clone(),
        orchestration_node_id: "work".into(),
        orchestration_version: 1,
        hop_index: 1,
        parent_run_id: Some(root_id(fixture)),
        triggering_handoff_id: None,
        created_at_ms: 1000,
    };
    store.with_connection(|connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::stateful_runtime::orchestration_store::upsert_automation_run(&transaction, &child)?;
        crate::stateful_runtime::orchestration_store::upsert_goal(&transaction, &goal)?;
        transaction.execute("INSERT INTO goal_run_links
            (goal_id,run_id,orchestration_node_id,orchestration_version,hop_index,parent_run_id,
             triggering_handoff_id,link_json,created_at_ms) VALUES (?1,?2,'work',1,1,?3,NULL,?4,1000)",
            params![goal.goal_id,child_id,root_id(fixture),
                protected_records::encode(&child.tenant_context,"link",&child_id,&link)?])?;
        transaction.commit()?;
        Ok(())
    }).unwrap();
    child_id
}

#[test]
#[serial]
fn solution_budget_provider_current_goal_and_claim_are_required_before_any_send() {
    encrypted(|| {
        for_each_backend(|_, store| {
            for fault in [
                "missing-run",
                "root",
                "claim",
                "claimant",
                "epoch",
                "expired",
                "paused",
                "cancelled",
                "goal-deadline",
                "goal-active-run",
                "run-paused",
                "tenant",
            ] {
                let fixture = network_fixture(store, &format!("lineage-{fault}"));
                let mut approved = approval(&fixture, store);
                let mut goal = store.get_goal(&goal_id(&fixture)).unwrap().unwrap();
                let mut run = store
                    .get_automation_run(&root_id(&fixture))
                    .unwrap()
                    .unwrap();
                match fault {
                    "missing-run" => approved.execution.run_id = "nonexistent".into(),
                    "root" => approved.root_run_id = "invented-budget".into(),
                    "claim" => approved.execution.claim_id = "stale-claim".into(),
                    "claimant" => approved.execution.claimant_id = "other-executor".into(),
                    "epoch" => approved.execution.lease_epoch = 2,
                    "expired" => run.execution_claim.as_mut().unwrap().lease_expires_at_ms = 1500,
                    "paused" => goal.status = tandem_automation::LongRunningGoalStatus::Paused,
                    "cancelled" => {
                        goal.status = tandem_automation::LongRunningGoalStatus::Cancelled
                    }
                    "goal-deadline" => goal.policy.deadline_at_ms = Some(1500),
                    "goal-active-run" => goal.active_run_id = Some("other-run".into()),
                    "run-paused" => run.status = tandem_automation::AutomationRunStatus::Paused,
                    "tenant" => run.tenant_context.workspace_id = "other-workspace".into(),
                    _ => unreachable!(),
                }
                store.put_goal(&goal).unwrap();
                store.upsert_automation_runs([&run]).unwrap();
                runtime().block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let registry = registry(listener.local_addr().unwrap());
                    let policy = policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
                    assert!(complete(&registry, policy).await.is_err(), "{fault}");
                    assert!(
                        account_row(store, &fixture, "global").await.is_none(),
                        "{fault}"
                    );
                    assert!(tokio::time::timeout(
                        std::time::Duration::from_millis(30),
                        listener.accept()
                    )
                    .await
                    .is_err());
                });
            }
        })
    });
}

#[test]
#[serial]
fn solution_budget_provider_goal_pause_after_reservation_refunds_proven_non_dispatch() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "lineage-pause-after-reserve");
            let approved = approval(&fixture, store);
            let mut paused = store.get_goal(&goal_id(&fixture)).unwrap().unwrap();
            paused.status = tandem_automation::LongRunningGoalStatus::Paused;
            runtime().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                let registry_for_auth = registry.clone();
                let mutator = store.clone();
                let calls = Arc::new(AtomicUsize::new(0));
                let policy = store
                    .solution_provider_attempt_policy(
                        10,
                        4096,
                        move |_| {
                            let current_registry = registry_for_auth.clone();
                            let approved = approved.clone();
                            let paused = paused.clone();
                            let mutator = mutator.clone();
                            let second = calls.fetch_add(1, Ordering::SeqCst) == 1;
                            async move {
                                if second {
                                    crate::encrypted_file_store::spawn_protected_blocking(
                                        move || mutator.put_goal(&paused),
                                    )
                                    .await??;
                                }
                                current(&approved, &current_registry).await
                            }
                        },
                        || 1500,
                    )
                    .unwrap();
                assert!(complete(&registry, policy).await.is_err());
                let root = account(
                    store,
                    &fixture,
                    &format!("root:{}", sha256(root_id(&fixture).as_bytes())),
                )
                .await;
                assert_eq!(root["requests"], 1);
                assert_eq!(root["committed_cost"], 0);
                assert_eq!(root["reserved_cost"], 0);
                assert_eq!(account(store, &fixture, "global").await["outstanding"], 0);
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
fn solution_budget_provider_child_inherits_persisted_root_after_restart() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "lineage-child");
            let mut approved = approval(&fixture, store);
            approved.run_budget.max_requests = 2;
            let rt = runtime();
            let (listener, registry, first) = rt.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let registry = registry(listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    request(&mut socket).await;
                    reply(&mut socket, true).await;
                    listener
                });
                let first = policy(
                    store,
                    &registry,
                    approved.clone(),
                    Arc::new(AtomicU64::new(1500)),
                );
                assert_eq!(complete(&registry, first.clone()).await.unwrap(), "ok");
                (server.await.unwrap(), registry, first)
            });
            // The native lifecycle writes these same protected goal/run/link rows.
            // This fixture isolates accounting lineage from handoff policy.
            approved.execution.run_id = seed_child(store, &fixture);
            store.initialize().unwrap();
            rt.block_on(async {
                assert!(
                    complete(&registry, first).await.is_err(),
                    "old active run must stop"
                );
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    request(&mut socket).await;
                    reply(&mut socket, true).await;
                    listener
                });
                let child_policy =
                    policy(store, &registry, approved, Arc::new(AtomicU64::new(1500)));
                assert_eq!(
                    complete(&registry, child_policy.clone()).await.unwrap(),
                    "ok"
                );
                assert!(complete(&registry, child_policy)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("budget"));
                let listener = server.await.unwrap();
                let root = account(
                    store,
                    &fixture,
                    &format!("root:{}", sha256(root_id(&fixture).as_bytes())),
                )
                .await;
                assert_eq!(root["requests"], 2);
                assert_eq!(root["committed_cost"], 10);
                assert!(account_row(
                    store,
                    &fixture,
                    &format!(
                        "root:{}",
                        sha256(format!("child-{}", root_id(&fixture)).as_bytes())
                    )
                )
                .await
                .is_none());
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
fn solution_budget_provider_parent_chain_rejects_missing_cyclic_and_foreign_roots() {
    encrypted(|| {
        for_each_backend(|_, store| {
            for fault in ["missing", "cycle", "foreign"] {
                let fixture = network_fixture(store, &format!("parent-{fault}"));
                let foreign = network_fixture(store, &format!("other-parent-{fault}"));
                let mut approved = approval(&fixture, store);
                approved.execution.run_id = seed_child(store, &fixture);
                store
                    .validate_solution_execution(
                        &approved.verified.tenant_context,
                        &approved.execution,
                        &approved.root_run_id,
                        || 1500,
                    )
                    .unwrap();
                store.with_connection(|connection| {
                let raw: String = connection.query_row(
                    "SELECT link_json FROM goal_run_links WHERE run_id=?1",
                    [&approved.execution.run_id], |row| row.get(0))?;
                let mut link: GoalRunLink = protected_records::decode(
                    &approved.verified.tenant_context, "link", &approved.execution.run_id, &raw)?;
                link.parent_run_id = Some(match fault {
                    "missing" => "missing-parent".into(),
                    "cycle" => approved.execution.run_id.clone(),
                    "foreign" => root_id(&foreign),
                    _ => unreachable!(),
                });
                connection.execute("UPDATE goal_run_links SET parent_run_id=?2,link_json=?3 WHERE run_id=?1",
                    params![approved.execution.run_id,link.parent_run_id,
                        protected_records::encode(&approved.verified.tenant_context,"link",&approved.execution.run_id,&link)?])?;
                Ok(())
            }).unwrap();
                assert!(
                    store
                        .validate_solution_execution(
                            &approved.verified.tenant_context,
                            &approved.execution,
                            &approved.root_run_id,
                            || 1500
                        )
                        .is_err(),
                    "{fault}"
                );
            }
        })
    });
}
