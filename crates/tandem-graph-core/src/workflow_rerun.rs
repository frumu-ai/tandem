use crate::{
    stable_graph_hash, GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput, StableGraphHashError,
    WorkflowBlocker, WorkflowGraph, WorkflowStepDependencySummary,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepCacheKey {
    pub step_id: String,
    pub input_hash: String,
    pub tool_schema_hash: String,
    pub policy_hash: String,
    pub memory_snapshot_hash: String,
    pub model_id: String,
    pub prompt_hash: String,
}

impl WorkflowStepCacheKey {
    pub fn stable_key(&self) -> Result<String, StableGraphHashError> {
        stable_graph_hash(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowRerunChange {
    StepFailed {
        step_id: String,
    },
    InputHashChanged {
        step_id: String,
        old_hash: String,
        new_hash: String,
    },
    PromptHashChanged {
        step_id: Option<String>,
        old_hash: String,
        new_hash: String,
    },
    PolicyHashChanged {
        policy_scope: Option<String>,
        old_hash: String,
        new_hash: String,
    },
    ToolSchemaChanged {
        tool_name: Option<String>,
        old_hash: String,
        new_hash: String,
    },
    MemorySnapshotChanged {
        tier: Option<String>,
        old_hash: String,
        new_hash: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRerunPlan {
    pub dirty_steps: Vec<WorkflowRerunStep>,
    pub reusable_steps: Vec<String>,
    pub blockers: Vec<WorkflowBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRerunStep {
    pub step_id: String,
    pub reason: String,
    pub caused_by: Vec<String>,
    pub cache_key: Option<String>,
}

impl WorkflowGraph {
    pub fn workflow_rerun_plan(
        &self,
        envelope: &GraphQueryEnvelope,
        changes: &[WorkflowRerunChange],
        cache_keys: &[WorkflowStepCacheKey],
    ) -> GraphQueryOutput<WorkflowRerunPlan> {
        let mut audit = GraphQueryAudit::default();
        let blockers = self.envelope_blockers(envelope);
        if !blockers.is_empty() {
            for blocker in &blockers {
                audit.deny(blocker.detail.clone());
            }
            return GraphQueryOutput::new(
                WorkflowRerunPlan {
                    dirty_steps: Vec::new(),
                    reusable_steps: Vec::new(),
                    blockers,
                },
                audit,
            );
        }

        let seeds = dirty_seed_steps(&self.step_dependencies, changes);
        let dirty = downstream_closure(&self.step_dependencies, &seeds);
        let dirty_steps = self
            .step_dependencies
            .iter()
            .filter(|(step_id, _)| dirty.contains(step_id))
            .map(|(step_id, _)| WorkflowRerunStep {
                step_id: step_id.clone(),
                reason: rerun_reason(step_id, &seeds),
                caused_by: causes_for_step(step_id, &seeds),
                cache_key: cache_keys
                    .iter()
                    .find(|key| key.step_id == *step_id)
                    .and_then(|key| key.stable_key().ok()),
            })
            .collect::<Vec<_>>();
        let reusable_steps = self
            .step_dependencies
            .iter()
            .filter_map(|(step_id, _)| (!dirty.contains(step_id)).then_some(step_id.clone()))
            .collect();

        GraphQueryOutput::new(
            WorkflowRerunPlan {
                dirty_steps,
                reusable_steps,
                blockers,
            },
            audit,
        )
    }
}

fn dirty_seed_steps(
    dependencies: &[(String, WorkflowStepDependencySummary)],
    changes: &[WorkflowRerunChange],
) -> BTreeSet<String> {
    let mut seeds = BTreeSet::new();
    for change in changes {
        match change {
            WorkflowRerunChange::StepFailed { step_id }
            | WorkflowRerunChange::InputHashChanged { step_id, .. } => {
                seeds.insert(step_id.clone());
            }
            WorkflowRerunChange::PromptHashChanged { step_id, .. } => {
                if let Some(step_id) = step_id {
                    seeds.insert(step_id.clone());
                } else {
                    seeds.extend(dependencies.iter().map(|(step_id, _)| step_id.clone()));
                }
            }
            WorkflowRerunChange::PolicyHashChanged { policy_scope, .. } => {
                seeds.extend(matching_steps(dependencies, |summary| {
                    policy_scope.as_ref().is_none_or(|scope| {
                        summary
                            .policy_scopes
                            .iter()
                            .any(|candidate| candidate == scope)
                    })
                }));
            }
            WorkflowRerunChange::ToolSchemaChanged { tool_name, .. } => {
                seeds.extend(matching_steps(dependencies, |summary| {
                    tool_name.as_ref().is_none_or(|tool| {
                        summary
                            .required_tools
                            .iter()
                            .any(|candidate| candidate == tool)
                    })
                }));
            }
            WorkflowRerunChange::MemorySnapshotChanged { tier, .. } => {
                seeds.extend(matching_steps(dependencies, |summary| {
                    tier.as_ref().is_none_or(|tier| {
                        summary
                            .memory_tiers
                            .iter()
                            .any(|candidate| candidate == tier)
                    })
                }));
            }
        }
    }
    seeds
}

fn matching_steps(
    dependencies: &[(String, WorkflowStepDependencySummary)],
    predicate: impl Fn(&WorkflowStepDependencySummary) -> bool,
) -> Vec<String> {
    dependencies
        .iter()
        .filter_map(|(step_id, summary)| predicate(summary).then_some(step_id.clone()))
        .collect()
}

fn downstream_closure(
    dependencies: &[(String, WorkflowStepDependencySummary)],
    seeds: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut dirty = seeds.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for (step_id, summary) in dependencies {
            if dirty.contains(step_id) {
                continue;
            }
            if summary
                .depends_on
                .iter()
                .any(|upstream| dirty.contains(upstream))
            {
                dirty.insert(step_id.clone());
                changed = true;
            }
        }
    }
    dirty
}

fn rerun_reason(step_id: &str, seeds: &BTreeSet<String>) -> String {
    if seeds.contains(step_id) {
        "directly affected by a changed workflow input".to_string()
    } else {
        "downstream of a dirty workflow step".to_string()
    }
}

fn causes_for_step(step_id: &str, seeds: &BTreeSet<String>) -> Vec<String> {
    if seeds.contains(step_id) {
        vec![step_id.to_string()]
    } else {
        seeds.iter().cloned().collect()
    }
}
