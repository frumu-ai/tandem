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
        for candidate in candidates {
            if !self.partition.is_visible_to(&candidate.scope) {
                audit.deny(format!(
                    "memory `{}` is outside the workflow partition scope",
                    candidate.memory_id
                ));
                continue;
            }
            if !envelope.allows_memory_tier(&candidate.tier) {
                audit.deny(format!(
                    "memory `{}` uses denied tier `{}`",
                    candidate.memory_id, candidate.tier
                ));
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
                audit.deny(format!("memory `{}` is stale", candidate.memory_id));
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
                fallback_to_semantic_search: memories.is_empty(),
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
