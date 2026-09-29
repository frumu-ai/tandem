// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Decision persistence for Goal Capability Learning discovery.

use std::collections::HashMap;
use std::sync::Arc;
use tandem_types::{CapabilityDiscoveryReport, GoalCapabilityLearningResponse, GoalSpec};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::goal_capability_learning::discovery::discover_capabilities_for_goal;
use crate::util::time::now_ms;

/// A recorded discovery decision.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryDecision {
    pub decision_id: String,
    pub goal: GoalSpec,
    pub report: CapabilityDiscoveryReport,
    pub tenant_id: String,
    pub owner_actor_id: Option<String>,
    pub created_at_ms: u64,
}

/// Stores and retrieves Goal Capability Learning discovery decisions.
pub struct GoalCapabilityLearningDecisionStore {
    decisions: Arc<RwLock<HashMap<String, DiscoveryDecision>>>,
}

impl GoalCapabilityLearningDecisionStore {
    pub fn new() -> Self {
        Self {
            decisions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Discover capabilities and record the decision.
    pub async fn discover_for_goal(
        &self,
        goal: GoalSpec,
        tenant_id: String,
        owner_actor_id: Option<String>,
    ) -> GoalCapabilityLearningResponse {
        self.discover_for_goal_guarded(goal, tenant_id, owner_actor_id, |commit| commit())
            .await
            .expect("unconditional capability discovery")
    }

    /// The guard runs after the store lock is acquired. It must keep revocable
    /// authority read guards alive while invoking the one-shot insert
    /// continuation, so publication cannot interleave before insertion.
    pub async fn discover_for_goal_guarded<F>(
        &self,
        goal: GoalSpec,
        tenant_id: String,
        owner_actor_id: Option<String>,
        authorize_and_commit: F,
    ) -> Option<GoalCapabilityLearningResponse>
    where
        F: FnOnce(
                &mut dyn FnMut() -> Option<GoalCapabilityLearningResponse>,
            ) -> Option<GoalCapabilityLearningResponse>
            + Send,
    {
        let report = discover_capabilities_for_goal(&goal);
        let uuid_str = Uuid::new_v4().to_string().replace('-', "");
        let decision_id = format!("gcl_{}", &uuid_str[..12]);

        let decision = DiscoveryDecision {
            decision_id: decision_id.clone(),
            goal,
            report: report.clone(),
            tenant_id,
            owner_actor_id,
            created_at_ms: now_ms(),
        };

        let mut decisions = self.decisions.write().await;
        let response = GoalCapabilityLearningResponse {
            request_id: decision_id.clone(),
            report,
        };
        let mut pending_decision = Some(decision);
        let mut attempts = 0;
        let guarded_result = {
            let mut insert_once = || {
                attempts += 1;
                if attempts != 1 {
                    return None;
                }
                let decision = pending_decision.take()?;
                decisions.insert(decision_id.clone(), decision);
                Some(response.clone())
            };
            authorize_and_commit(&mut insert_once)
        };
        if attempts != 1 || guarded_result.is_none() {
            if pending_decision.is_none() {
                decisions.remove(&decision_id);
            }
            return None;
        }
        Some(response)
    }

    /// Retrieve a discovery decision.
    pub async fn get_decision(&self, decision_id: &str) -> Option<DiscoveryDecision> {
        self.decisions.read().await.get(decision_id).cloned()
    }

    /// List decisions for a tenant.
    pub async fn list_for_tenant(&self, tenant_id: &str) -> Vec<DiscoveryDecision> {
        self.decisions
            .read()
            .await
            .values()
            .filter(|d| d.tenant_id == tenant_id)
            .cloned()
            .collect()
    }
}

impl Default for GoalCapabilityLearningDecisionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_goal() -> GoalSpec {
        GoalSpec {
            goal_id: "demo".to_string(),
            title: "Read and parse CSV".to_string(),
            description: "Demo CSV parsing".to_string(),
            input_parameters: vec![],
            expected_output_format: "JSON records".to_string(),
            constraints: vec![],
        }
    }

    #[tokio::test]
    async fn discover_and_store() {
        let store = GoalCapabilityLearningDecisionStore::new();
        let goal = demo_goal();
        let tenant = "tenant_1".to_string();

        let response = store
            .discover_for_goal(goal.clone(), tenant.clone(), Some("actor-a".to_string()))
            .await;

        assert!(response.request_id.starts_with("gcl_"));
        assert!(!response.report.composition_candidates.is_empty());
    }

    #[tokio::test]
    async fn guarded_discovery_rechecks_after_waiting_for_store_lock() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let store = Arc::new(GoalCapabilityLearningDecisionStore::new());
        let held = store.decisions.write().await;
        let allowed = Arc::new(AtomicBool::new(true));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let pending_store = Arc::clone(&store);
        let pending_allowed = Arc::clone(&allowed);
        let pending = tokio::spawn(async move {
            let _ = started_tx.send(());
            pending_store
                .discover_for_goal_guarded(
                    demo_goal(),
                    "tenant_1".to_string(),
                    Some("actor-a".to_string()),
                    move |commit| {
                        if pending_allowed.load(Ordering::SeqCst) {
                            commit()
                        } else {
                            None
                        }
                    },
                )
                .await
        });
        started_rx.await.expect("guarded discovery started");
        allowed.store(false, Ordering::SeqCst);
        drop(held);

        assert!(pending.await.expect("guarded discovery task").is_none());
        assert!(store.list_for_tenant("tenant_1").await.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn guarded_discovery_keeps_revocation_blocked_through_insert() {
        use std::sync::{mpsc, RwLock};

        let store = Arc::new(GoalCapabilityLearningDecisionStore::new());
        let allowed = Arc::new(RwLock::new(true));
        let (checked_tx, checked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let pending_store = Arc::clone(&store);
        let pending_allowed = Arc::clone(&allowed);
        let pending = tokio::spawn(async move {
            pending_store
                .discover_for_goal_guarded(
                    demo_goal(),
                    "tenant_1".to_string(),
                    Some("actor-a".to_string()),
                    move |commit| {
                        let policy = pending_allowed.read().expect("read authority");
                        if !*policy {
                            return None;
                        }
                        checked_tx.send(()).expect("signal authorized callback");
                        release_rx.recv().expect("release authorized callback");
                        commit()
                    },
                )
                .await
        });
        checked_rx
            .await
            .expect("authorization ran under policy read lock");
        assert!(
            allowed.try_write().is_err(),
            "revocation write lock must be unavailable before insert"
        );
        let writer_allowed = Arc::clone(&allowed);
        let (writer_started_tx, writer_started_rx) = tokio::sync::oneshot::channel();
        let revoker = tokio::task::spawn_blocking(move || {
            writer_started_tx
                .send(())
                .expect("signal revocation attempt");
            *writer_allowed.write().expect("write authority") = false;
        });
        writer_started_rx.await.expect("revocation attempt started");
        release_tx.send(()).expect("release insertion");
        let response = pending
            .await
            .expect("discovery task")
            .expect("authorized discovery");
        revoker.await.expect("revocation task");
        assert!(store.get_decision(&response.request_id).await.is_some());
        assert!(!*allowed.read().expect("read revoked authority"));
    }

    #[tokio::test]
    async fn guarded_discovery_rejects_skipped_or_repeated_insert() {
        let store = GoalCapabilityLearningDecisionStore::new();
        type Guard = fn(
            &mut dyn FnMut() -> Option<GoalCapabilityLearningResponse>,
        ) -> Option<GoalCapabilityLearningResponse>;
        let guards: [Guard; 2] = [
            |_commit: &mut dyn FnMut() -> Option<GoalCapabilityLearningResponse>| None,
            |commit: &mut dyn FnMut() -> Option<GoalCapabilityLearningResponse>| {
                let _ = commit();
                commit()
            },
        ];
        for authorize_and_commit in guards {
            assert!(store
                .discover_for_goal_guarded(
                    demo_goal(),
                    "tenant_1".to_string(),
                    Some("actor-a".to_string()),
                    authorize_and_commit,
                )
                .await
                .is_none());
        }
        assert!(store.list_for_tenant("tenant_1").await.is_empty());
    }

    #[tokio::test]
    async fn retrieve_decision() {
        let store = GoalCapabilityLearningDecisionStore::new();
        let goal = demo_goal();
        let tenant = "tenant_1".to_string();

        let response = store
            .discover_for_goal(goal, tenant, Some("actor-a".to_string()))
            .await;
        let id = response.request_id.clone();

        let decision = store.get_decision(&id).await;
        assert!(decision.is_some());
        assert_eq!(decision.unwrap().decision_id, id);
    }

    #[tokio::test]
    async fn list_tenant_decisions() {
        let store = GoalCapabilityLearningDecisionStore::new();
        let goal = demo_goal();

        store
            .discover_for_goal(goal.clone(), "t1".to_string(), Some("actor-a".to_string()))
            .await;
        store
            .discover_for_goal(goal.clone(), "t1".to_string(), Some("actor-b".to_string()))
            .await;
        store
            .discover_for_goal(goal, "t2".to_string(), Some("actor-a".to_string()))
            .await;

        let t1_decisions = store.list_for_tenant("t1").await;
        let t2_decisions = store.list_for_tenant("t2").await;

        assert_eq!(t1_decisions.len(), 2);
        assert_eq!(t2_decisions.len(), 1);
    }

    #[tokio::test]
    async fn decision_carries_owning_tenant_for_scoped_reads() {
        // The HTTP layer scopes get-by-id by comparing the authenticated tenant
        // against the decision's recorded tenant. This guards that the store
        // records the owning tenant so that comparison is possible: a decision
        // created by tenant_a must not report tenant_b as its owner.
        let store = GoalCapabilityLearningDecisionStore::new();
        let response = store
            .discover_for_goal(
                demo_goal(),
                "tenant_a".to_string(),
                Some("actor-a".to_string()),
            )
            .await;

        let decision = store
            .get_decision(&response.request_id)
            .await
            .expect("decision exists");

        assert_eq!(decision.tenant_id, "tenant_a");
        assert_ne!(decision.tenant_id, "tenant_b");
        assert_eq!(decision.owner_actor_id.as_deref(), Some("actor-a"));
    }
}
