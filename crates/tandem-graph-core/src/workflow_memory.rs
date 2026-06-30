use crate::{
    Freshness, GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput, GraphScope, Provenance,
    WorkflowBlocker, WorkflowBlockerKind, WorkflowGraph,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowMemoryQuery {
    pub step_id: String,
    pub step_kind: Option<String>,
    pub now_unix_ms: Option<u64>,
    pub include_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowMemoryCandidate {
    pub memory_id: String,
    pub collection_id: String,
    pub tier: String,
    pub policy_scope: Option<String>,
    pub workflow_template_id: Option<String>,
    pub workflow_step_id: Option<String>,
    pub step_kind: Option<String>,
    pub artifact_refs: Vec<String>,
    pub scope: GraphScope,
    pub summary: String,
    pub provenance: Provenance,
    pub freshness: Freshness,
    pub score: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowKnowledgeScopeRegistry {
    pub require_registered: bool,
    pub grants: Vec<WorkflowKnowledgeScopeGrant>,
}

impl WorkflowKnowledgeScopeRegistry {
    pub fn strict(grants: Vec<WorkflowKnowledgeScopeGrant>) -> Self {
        Self {
            require_registered: true,
            grants,
        }
    }

    fn decision_for(
        &self,
        candidate: &WorkflowMemoryCandidate,
        envelope: &GraphQueryEnvelope,
        query: &WorkflowMemoryQuery,
    ) -> WorkflowKnowledgeScopeDecision {
        let Some(grant) = self
            .grants
            .iter()
            .find(|grant| grant.matches_candidate(candidate))
        else {
            return if self.require_registered {
                WorkflowKnowledgeScopeDecision::deny("knowledge_scope_registry_missing")
            } else {
                WorkflowKnowledgeScopeDecision::allow("knowledge_scope_registry_not_required")
            };
        };

        grant.decision_for(candidate, envelope, query)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowKnowledgeScopeGrant {
    pub registry_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub memory_ids: Vec<String>,
    pub collection_id: Option<String>,
    pub policy_scope: Option<String>,
    pub data_class: String,
    pub risk_tier: Option<String>,
    pub owner_org_unit_id: Option<String>,
    pub resource_scope: Option<String>,
    pub allowed_workflow_phases: Vec<String>,
    pub allowed_memory_tiers: Vec<String>,
    pub expires_at_unix_ms: Option<u64>,
}

impl WorkflowKnowledgeScopeGrant {
    fn matches_candidate(&self, candidate: &WorkflowMemoryCandidate) -> bool {
        self.memory_ids
            .iter()
            .any(|memory_id| memory_id == &candidate.memory_id)
            || self
                .collection_id
                .as_ref()
                .is_some_and(|collection_id| collection_id == &candidate.collection_id)
            || self.policy_scope.as_ref().is_some_and(|scope| {
                candidate
                    .policy_scope
                    .as_ref()
                    .is_some_and(|candidate_scope| candidate_scope == scope)
            })
    }

    fn decision_for(
        &self,
        candidate: &WorkflowMemoryCandidate,
        envelope: &GraphQueryEnvelope,
        query: &WorkflowMemoryQuery,
    ) -> WorkflowKnowledgeScopeDecision {
        if self.tenant_id != envelope.scope.tenant_id || self.tenant_id != candidate.scope.tenant_id
        {
            return WorkflowKnowledgeScopeDecision::deny("knowledge_scope_tenant_mismatch");
        }
        if self.project_id != envelope.scope.project_id
            || self.project_id != candidate.scope.project_id
        {
            return WorkflowKnowledgeScopeDecision::deny("knowledge_scope_project_mismatch");
        }
        if self.workspace_id.is_some()
            && self.workspace_id != envelope.scope.workspace_id
            && self.workspace_id != candidate.scope.workspace_id
        {
            return WorkflowKnowledgeScopeDecision::deny("knowledge_scope_workspace_mismatch");
        }
        if self
            .expires_at_unix_ms
            .zip(query.now_unix_ms)
            .is_some_and(|(expires_at, now)| expires_at <= now)
        {
            return WorkflowKnowledgeScopeDecision::deny("knowledge_scope_registry_expired");
        }
        if !self.allowed_memory_tiers.is_empty()
            && !self
                .allowed_memory_tiers
                .iter()
                .any(|tier| tier == &candidate.tier)
        {
            return WorkflowKnowledgeScopeDecision::deny("knowledge_scope_tier_denied");
        }
        if !self.allowed_workflow_phases.is_empty()
            && !self.allowed_workflow_phases.iter().any(|phase| {
                phase == "*"
                    || phase == &query.step_id
                    || query
                        .step_kind
                        .as_ref()
                        .is_some_and(|step_kind| phase == step_kind)
            })
        {
            return WorkflowKnowledgeScopeDecision::deny("knowledge_scope_phase_denied");
        }

        WorkflowKnowledgeScopeDecision::allow("knowledge_scope_registry_allowed")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkflowKnowledgeScopeDecision {
    allowed: bool,
    reason: &'static str,
}

impl WorkflowKnowledgeScopeDecision {
    fn allow(reason: &'static str) -> Self {
        Self {
            allowed: true,
            reason,
        }
    }

    fn deny(reason: &'static str) -> Self {
        Self {
            allowed: false,
            reason,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowMemoryBundle {
    pub step_id: String,
    pub memories: Vec<WorkflowMemoryMatch>,
    pub fallback_to_semantic_search: bool,
    pub blockers: Vec<WorkflowBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowMemoryMatch {
    pub memory_id: String,
    pub collection_id: String,
    pub tier: String,
    pub summary: String,
    pub reason: String,
    pub provenance: Provenance,
    pub freshness: Freshness,
    pub score: Option<String>,
}

impl WorkflowGraph {
    pub fn workflow_memory_bundle(
        &self,
        envelope: &GraphQueryEnvelope,
        query: WorkflowMemoryQuery,
        candidates: &[WorkflowMemoryCandidate],
    ) -> GraphQueryOutput<WorkflowMemoryBundle> {
        self.workflow_memory_bundle_with_knowledge_scope_registry(
            envelope,
            query,
            candidates,
            &WorkflowKnowledgeScopeRegistry::default(),
        )
    }

    pub fn workflow_memory_bundle_with_knowledge_scope_registry(
        &self,
        envelope: &GraphQueryEnvelope,
        query: WorkflowMemoryQuery,
        candidates: &[WorkflowMemoryCandidate],
        registry: &WorkflowKnowledgeScopeRegistry,
    ) -> GraphQueryOutput<WorkflowMemoryBundle> {
        let mut audit = GraphQueryAudit::default();
        let mut blockers = self.envelope_blockers(envelope);
        let Some(summary) = self.dependencies_for_step(&query.step_id) else {
            blockers.push(WorkflowBlocker::new(
                &query.step_id,
                WorkflowBlockerKind::DependencyPending,
                &query.step_id,
                "workflow step is not present in the graph",
            ));
            return blocked_memory_bundle(query.step_id, blockers, audit);
        };

        for tier in &summary.memory_tiers {
            if !envelope.allows_memory_tier(tier) {
                blockers.push(WorkflowBlocker::new(
                    &query.step_id,
                    WorkflowBlockerKind::MemoryDenied,
                    tier,
                    format!("memory tier `{tier}` is not allowed"),
                ));
            }
        }
        if !blockers.is_empty() {
            return blocked_memory_bundle(query.step_id, blockers, audit);
        }

        let mut memories = Vec::new();
        let mut registry_denied_count = 0usize;
        for candidate in candidates {
            if !self.partition.is_visible_to(&candidate.scope) {
                audit.deny("memory outside the workflow partition scope");
                continue;
            }
            if !matches_query_run(candidate, envelope) {
                audit.deny("memory outside the query run scope");
                continue;
            }
            if !envelope.allows_memory_tier(&candidate.tier) {
                audit.deny("memory uses denied tier");
                continue;
            }
            if !summary
                .memory_tiers
                .iter()
                .any(|tier| tier == &candidate.tier)
            {
                continue;
            }
            if is_stale(candidate, &query) {
                audit.deny("memory is stale");
                continue;
            }
            let registry_decision = registry.decision_for(candidate, envelope, &query);
            if !registry_decision.allowed {
                registry_denied_count += 1;
                audit.deny(registry_decision.reason);
                continue;
            }
            let Some(reason) = memory_reason(candidate, summary, &query) else {
                continue;
            };
            memories.push(WorkflowMemoryMatch {
                memory_id: candidate.memory_id.clone(),
                collection_id: candidate.collection_id.clone(),
                tier: candidate.tier.clone(),
                summary: candidate.summary.clone(),
                reason,
                provenance: candidate.provenance.clone(),
                freshness: candidate.freshness.clone(),
                score: candidate.score.clone(),
            });
        }

        memories.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });

        GraphQueryOutput::new(
            WorkflowMemoryBundle {
                step_id: query.step_id,
                fallback_to_semantic_search: memories.is_empty() && registry_denied_count == 0,
                memories,
                blockers,
            },
            audit,
        )
    }
}

fn blocked_memory_bundle(
    step_id: String,
    blockers: Vec<WorkflowBlocker>,
    mut audit: GraphQueryAudit,
) -> GraphQueryOutput<WorkflowMemoryBundle> {
    for blocker in &blockers {
        audit.deny(blocker.detail.clone());
    }
    GraphQueryOutput::new(
        WorkflowMemoryBundle {
            step_id,
            memories: Vec::new(),
            fallback_to_semantic_search: true,
            blockers,
        },
        audit,
    )
}

fn matches_query_run(candidate: &WorkflowMemoryCandidate, envelope: &GraphQueryEnvelope) -> bool {
    let scope_run_id = envelope.scope.run_id.as_deref();
    let envelope_run_id = envelope.run_id.as_deref();
    let Some(query_run_id) = scope_run_id.or(envelope_run_id) else {
        return true;
    };

    if scope_run_id
        .zip(envelope_run_id)
        .is_some_and(|(scope_run_id, envelope_run_id)| scope_run_id != envelope_run_id)
    {
        return false;
    }

    candidate
        .scope
        .run_id
        .as_deref()
        .is_none_or(|candidate_run_id| candidate_run_id == query_run_id)
}

fn is_stale(candidate: &WorkflowMemoryCandidate, query: &WorkflowMemoryQuery) -> bool {
    !query.include_stale
        && query
            .now_unix_ms
            .is_some_and(|now| candidate.freshness.is_stale_at(now))
}

fn memory_reason(
    candidate: &WorkflowMemoryCandidate,
    summary: &crate::WorkflowStepDependencySummary,
    query: &WorkflowMemoryQuery,
) -> Option<String> {
    if candidate.workflow_step_id.as_deref() == Some(&query.step_id) {
        return Some(format!("linked to workflow step `{}`", query.step_id));
    }
    if query.step_kind.as_ref().is_some_and(|step_kind| {
        candidate
            .step_kind
            .as_ref()
            .is_some_and(|candidate_kind| candidate_kind == step_kind)
    }) {
        return Some(format!(
            "linked to prior successful `{}` steps",
            query.step_kind.as_deref().unwrap_or_default()
        ));
    }
    if candidate
        .policy_scope
        .as_ref()
        .is_some_and(|scope| summary.policy_scopes.iter().any(|needed| needed == scope))
    {
        return Some("matches a policy scope required by this step".to_string());
    }
    if candidate.artifact_refs.iter().any(|artifact| {
        summary
            .artifact_refs
            .iter()
            .any(|needed| needed == artifact)
    }) {
        return Some("linked to an artifact produced or consumed by this step".to_string());
    }
    None
}
