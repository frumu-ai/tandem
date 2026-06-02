// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::plan_package::{
    DependencyResolutionStrategy, PartialFailureMode, ReentryPoint, RoutinePackage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutineExecutionBatch {
    pub step_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutineExecutionPlan {
    pub routine_id: String,
    pub strategy: DependencyResolutionStrategy,
    pub partial_failure_mode: PartialFailureMode,
    pub reentry_point: ReentryPoint,
    #[serde(default)]
    pub external_prerequisites: Vec<String>,
    #[serde(default)]
    pub batches: Vec<RoutineExecutionBatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DependencyPlanningError {
    MissingStepDependency { step_id: String, dependency: String },
    StrictSequentialOrderContradictsDependency { step_id: String, dependency: String },
    CyclicStepDependencies { remaining_step_ids: Vec<String> },
}

pub fn plan_routine_execution(
    routine: &RoutinePackage,
) -> Result<RoutineExecutionPlan, DependencyPlanningError> {
    let step_ids = routine
        .steps
        .iter()
        .map(|step| step.step_id.clone())
        .collect::<Vec<_>>();
    let step_id_set = step_ids.iter().cloned().collect::<BTreeSet<_>>();
    let routine_dependency_ids = routine
        .dependencies
        .iter()
        .map(|dependency| dependency.routine_id.clone())
        .collect::<BTreeSet<_>>();

    let mut adjacency = BTreeMap::<String, Vec<String>>::new();
    let mut indegree = BTreeMap::<String, usize>::new();
    let mut declared_index = BTreeMap::<String, usize>::new();
    let mut external_prerequisites = BTreeSet::<String>::new();

    for (index, step) in routine.steps.iter().enumerate() {
        adjacency.entry(step.step_id.clone()).or_default();
        indegree.entry(step.step_id.clone()).or_insert(0);
        declared_index.insert(step.step_id.clone(), index);
    }

    for step in &routine.steps {
        for dependency in &step.dependencies {
            if step_id_set.contains(dependency) {
                adjacency
                    .entry(dependency.clone())
                    .or_default()
                    .push(step.step_id.clone());
                *indegree.entry(step.step_id.clone()).or_insert(0) += 1;
            } else if routine_dependency_ids.contains(dependency) {
                external_prerequisites.insert(dependency.clone());
            } else {
                return Err(DependencyPlanningError::MissingStepDependency {
                    step_id: step.step_id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(step_id, degree)| (*degree == 0).then_some(step_id.clone()))
        .collect::<Vec<_>>();
    ready.sort_by_key(|step_id| declared_index.get(step_id).copied().unwrap_or(usize::MAX));
    let mut ready = VecDeque::from(ready);
    let mut planned = Vec::<String>::new();
    let mut batches = Vec::<RoutineExecutionBatch>::new();

    match routine.dependency_resolution.strategy {
        DependencyResolutionStrategy::StrictSequential => {
            for step in &routine.steps {
                let step_index = declared_index
                    .get(&step.step_id)
                    .copied()
                    .unwrap_or(usize::MAX);
                for dependency in &step.dependencies {
                    if step_id_set.contains(dependency) {
                        let dependency_index = declared_index
                            .get(dependency)
                            .copied()
                            .unwrap_or(usize::MAX);
                        if dependency_index > step_index {
                            return Err(
                                DependencyPlanningError::StrictSequentialOrderContradictsDependency {
                                    step_id: step.step_id.clone(),
                                    dependency: dependency.clone(),
                                },
                            );
                        }
                    }
                }
            }
            batches = step_ids
                .iter()
                .map(|step_id| RoutineExecutionBatch {
                    step_ids: vec![step_id.clone()],
                })
                .collect();
            planned = step_ids.clone();
        }
        DependencyResolutionStrategy::TopologicalSequential => {
            while let Some(step_id) = ready.pop_front() {
                planned.push(step_id.clone());
                batches.push(RoutineExecutionBatch {
                    step_ids: vec![step_id.clone()],
                });
                release_dependents(
                    &step_id,
                    &adjacency,
                    &mut indegree,
                    &declared_index,
                    &mut ready,
                );
            }
        }
        DependencyResolutionStrategy::TopologicalParallel => {
            while !ready.is_empty() {
                let current_batch = ready.drain(..).collect::<Vec<_>>();
                for step_id in &current_batch {
                    planned.push(step_id.clone());
                }
                batches.push(RoutineExecutionBatch {
                    step_ids: current_batch.clone(),
                });

                let mut next_ready = Vec::<String>::new();
                for step_id in &current_batch {
                    collect_released_dependents(
                        step_id,
                        &adjacency,
                        &mut indegree,
                        &mut next_ready,
                    );
                }
                next_ready.sort_by_key(|step_id| {
                    declared_index.get(step_id).copied().unwrap_or(usize::MAX)
                });
                ready = VecDeque::from(next_ready);
            }
        }
    }

    if planned.len() != step_ids.len() {
        let remaining_step_ids = step_ids
            .into_iter()
            .filter(|step_id| !planned.contains(step_id))
            .collect::<Vec<_>>();
        return Err(DependencyPlanningError::CyclicStepDependencies { remaining_step_ids });
    }

    Ok(RoutineExecutionPlan {
        routine_id: routine.routine_id.clone(),
        strategy: routine.dependency_resolution.strategy.clone(),
        partial_failure_mode: routine.dependency_resolution.partial_failure_mode.clone(),
        reentry_point: routine.dependency_resolution.reentry_point.clone(),
        external_prerequisites: external_prerequisites.into_iter().collect(),
        batches,
    })
}

fn release_dependents(
    step_id: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    indegree: &mut BTreeMap<String, usize>,
    declared_index: &BTreeMap<String, usize>,
    ready: &mut VecDeque<String>,
) {
    let mut released = Vec::<String>::new();
    collect_released_dependents(step_id, adjacency, indegree, &mut released);
    released.sort_by_key(|candidate| declared_index.get(candidate).copied().unwrap_or(usize::MAX));
    for candidate in released {
        ready.push_back(candidate);
    }
}

fn collect_released_dependents(
    step_id: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    indegree: &mut BTreeMap<String, usize>,
    released: &mut Vec<String>,
) {
    if let Some(dependents) = adjacency.get(step_id) {
        for dependent in dependents {
            if let Some(entry) = indegree.get_mut(dependent) {
                *entry -= 1;
                if *entry == 0 {
                    released.push(dependent.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_package::{
        ApprovalMode, AuditScope, CrossRoutineVisibility, DataScope, DependencyResolution,
        DependencyResolutionStrategy, FinalArtifactVisibility, IntermediateArtifactVisibility,
        MidRoutineConnectorFailureMode, MissionContextScope, PartialFailureMode, ReentryPoint,
        RoutineConnectorResolution, RoutineDependency, RoutineSemanticKind, RunHistoryVisibility,
        StepPackage, SuccessCriteria, TriggerDefinition, TriggerKind,
    };

    fn sample_routine(strategy: DependencyResolutionStrategy) -> RoutinePackage {
        RoutinePackage {
            routine_id: "routine_a".to_string(),
            semantic_kind: RoutineSemanticKind::Mixed,
            trigger: TriggerDefinition {
                trigger_type: TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: vec![RoutineDependency {
                dependency_type: "routine".to_string(),
                routine_id: "upstream_routine".to_string(),
                mode: crate::plan_package::DependencyMode::Hard,
            }],
            dependency_resolution: DependencyResolution {
                strategy,
                partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                reentry_point: ReentryPoint::FailedStep,
                mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: RoutineConnectorResolution::default(),
            data_scope: DataScope {
                readable_paths: vec!["mission.goal".to_string()],
                writable_paths: vec!["knowledge/workflows/drafts/routine_a/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: CrossRoutineVisibility::None,
                mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: AuditScope {
                run_history_visibility: RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
            },
            success_criteria: SuccessCriteria::default(),
            steps: vec![
                StepPackage {
                    step_id: "step_a".to_string(),
                    label: "A".to_string(),
                    kind: "analysis".to_string(),
                    action: "A".to_string(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    dependencies: vec!["upstream_routine".to_string()],
                    context_reads: Vec::new(),
                    context_writes: Vec::new(),
                    connector_requirements: Vec::new(),
                    model_policy: Default::default(),
                    approval_policy: ApprovalMode::InternalOnly,
                    success_criteria: SuccessCriteria::default(),
                    failure_policy: Default::default(),
                    retry_policy: Default::default(),
                    artifacts: Vec::new(),
                    provenance: None,
                    notes: None,
                },
                StepPackage {
                    step_id: "step_b".to_string(),
                    label: "B".to_string(),
                    kind: "analysis".to_string(),
                    action: "B".to_string(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    dependencies: vec!["step_a".to_string()],
                    context_reads: Vec::new(),
                    context_writes: Vec::new(),
                    connector_requirements: Vec::new(),
                    model_policy: Default::default(),
                    approval_policy: ApprovalMode::InternalOnly,
                    success_criteria: SuccessCriteria::default(),
                    failure_policy: Default::default(),
                    retry_policy: Default::default(),
                    artifacts: Vec::new(),
                    provenance: None,
                    notes: None,
                },
                StepPackage {
                    step_id: "step_c".to_string(),
                    label: "C".to_string(),
                    kind: "analysis".to_string(),
                    action: "C".to_string(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    dependencies: vec!["step_a".to_string()],
                    context_reads: Vec::new(),
                    context_writes: Vec::new(),
                    connector_requirements: Vec::new(),
                    model_policy: Default::default(),
                    approval_policy: ApprovalMode::InternalOnly,
                    success_criteria: SuccessCriteria::default(),
                    failure_policy: Default::default(),
                    retry_policy: Default::default(),
                    artifacts: Vec::new(),
                    provenance: None,
                    notes: None,
                },
                StepPackage {
                    step_id: "step_d".to_string(),
                    label: "D".to_string(),
                    kind: "analysis".to_string(),
                    action: "D".to_string(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    dependencies: vec!["step_b".to_string(), "step_c".to_string()],
                    context_reads: Vec::new(),
                    context_writes: Vec::new(),
                    connector_requirements: Vec::new(),
                    model_policy: Default::default(),
                    approval_policy: ApprovalMode::InternalOnly,
                    success_criteria: SuccessCriteria::default(),
                    failure_policy: Default::default(),
                    retry_policy: Default::default(),
                    artifacts: Vec::new(),
                    provenance: None,
                    notes: None,
                },
            ],
        }
    }

    #[test]
    fn topological_parallel_groups_ready_steps() {
        let routine = sample_routine(DependencyResolutionStrategy::TopologicalParallel);

        let plan = plan_routine_execution(&routine).expect("plan");

        assert_eq!(
            plan.external_prerequisites,
            vec!["upstream_routine".to_string()]
        );
        assert_eq!(
            plan.batches,
            vec![
                RoutineExecutionBatch {
                    step_ids: vec!["step_a".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_b".to_string(), "step_c".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_d".to_string()]
                }
            ]
        );
    }

    #[test]
    fn topological_sequential_emits_single_step_batches() {
        let routine = sample_routine(DependencyResolutionStrategy::TopologicalSequential);

        let plan = plan_routine_execution(&routine).expect("plan");

        assert_eq!(
            plan.batches,
            vec![
                RoutineExecutionBatch {
                    step_ids: vec!["step_a".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_b".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_c".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_d".to_string()]
                }
            ]
        );
    }

    #[test]
    fn strict_sequential_uses_declared_order() {
        let mut routine = sample_routine(DependencyResolutionStrategy::StrictSequential);
        routine.steps.swap(1, 2);

        let plan = plan_routine_execution(&routine).expect("plan");

        assert_eq!(
            plan.batches,
            vec![
                RoutineExecutionBatch {
                    step_ids: vec!["step_a".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_c".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_b".to_string()]
                },
                RoutineExecutionBatch {
                    step_ids: vec!["step_d".to_string()]
                }
            ]
        );
    }

    #[test]
    fn strict_sequential_rejects_order_that_contradicts_dependencies() {
        let mut routine = sample_routine(DependencyResolutionStrategy::StrictSequential);
        routine.steps.swap(0, 1);

        let error = plan_routine_execution(&routine).expect_err("strict order conflict");

        assert_eq!(
            error,
            DependencyPlanningError::StrictSequentialOrderContradictsDependency {
                step_id: "step_b".to_string(),
                dependency: "step_a".to_string(),
            }
        );
    }

    #[test]
    fn missing_step_dependency_returns_error() {
        let mut routine = sample_routine(DependencyResolutionStrategy::TopologicalParallel);
        routine.steps[1].dependencies = vec!["missing_step".to_string()];

        let error = plan_routine_execution(&routine).expect_err("missing dependency error");

        assert_eq!(
            error,
            DependencyPlanningError::MissingStepDependency {
                step_id: "step_b".to_string(),
                dependency: "missing_step".to_string(),
            }
        );
    }

    #[test]
    fn cyclic_dependencies_return_error() {
        let mut routine = sample_routine(DependencyResolutionStrategy::TopologicalParallel);
        routine.steps[0].dependencies.push("step_d".to_string());

        let error = plan_routine_execution(&routine).expect_err("cycle error");

        match error {
            DependencyPlanningError::CyclicStepDependencies { remaining_step_ids } => {
                assert!(remaining_step_ids.contains(&"step_a".to_string()));
                assert!(remaining_step_ids.contains(&"step_d".to_string()));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
