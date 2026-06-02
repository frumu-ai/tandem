// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::automation_projection::{
    ProjectedAutomationAgentProfile, ProjectedAutomationDraft, ProjectedAutomationExecutionPolicy,
    ProjectedAutomationNode,
};
use crate::plan_package::{ContextObject, PlanPackage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectedStepContextBindings {
    pub step_id: String,
    #[serde(default)]
    pub context_reads: Vec<String>,
    #[serde(default)]
    pub context_writes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedRoutineContextPartition {
    pub routine_id: String,
    #[serde(default)]
    pub visible_context_objects: Vec<ContextObject>,
    #[serde(default)]
    pub step_context_bindings: Vec<ProjectedStepContextBindings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProjectedAutomationContextMaterialization {
    #[serde(default)]
    pub routines: Vec<ProjectedRoutineContextPartition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovedPlanStepContextBinding {
    pub step_id: String,
    #[serde(default)]
    pub context_reads: Vec<String>,
    #[serde(default)]
    pub context_writes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovedPlanRoutineMaterialization {
    pub routine_id: String,
    #[serde(default)]
    pub step_ids: Vec<String>,
    #[serde(default)]
    pub visible_context_object_ids: Vec<String>,
    #[serde(default)]
    pub step_context_bindings: Vec<ApprovedPlanStepContextBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovedPlanMaterialization {
    pub plan_id: String,
    pub plan_revision: u32,
    pub lifecycle_state: crate::plan_package::PlanLifecycleState,
    pub routine_count: usize,
    pub step_count: usize,
    pub context_object_count: usize,
    #[serde(default)]
    pub routines: Vec<ApprovedPlanRoutineMaterialization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationMaterializationSeed<I, O> {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    #[serde(default)]
    pub agents: Vec<ProjectedAutomationAgentProfile>,
    #[serde(default)]
    pub nodes: Vec<ProjectedAutomationNode<I, O>>,
    pub execution: ProjectedAutomationExecutionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ProjectedAutomationContextMaterialization>,
    pub metadata: Value,
}

impl<I, O> From<ProjectedAutomationDraft<I, O>> for ProjectedAutomationMaterializationSeed<I, O> {
    fn from(value: ProjectedAutomationDraft<I, O>) -> Self {
        Self {
            name: value.name,
            description: value.description,
            workspace_root: value.workspace_root,
            output_targets: value.output_targets,
            agents: value.agents,
            nodes: value.nodes,
            execution: value.execution,
            context: value.context,
            metadata: value.metadata,
        }
    }
}

impl<I, O> From<ProjectedAutomationMaterializationSeed<I, O>> for ProjectedAutomationDraft<I, O> {
    fn from(value: ProjectedAutomationMaterializationSeed<I, O>) -> Self {
        Self {
            name: value.name,
            description: value.description,
            workspace_root: value.workspace_root,
            output_targets: value.output_targets,
            agents: value.agents,
            nodes: value.nodes,
            execution: value.execution,
            context: value.context,
            metadata: value.metadata,
        }
    }
}

pub fn materialization_seed_from_projection<I, O>(
    draft: ProjectedAutomationDraft<I, O>,
) -> ProjectedAutomationMaterializationSeed<I, O> {
    draft.into()
}

pub fn project_plan_context_materialization(
    plan: &PlanPackage,
) -> ProjectedAutomationContextMaterialization {
    ProjectedAutomationContextMaterialization {
        routines: plan
            .routine_graph
            .iter()
            .map(|routine| ProjectedRoutineContextPartition {
                routine_id: routine.routine_id.clone(),
                visible_context_objects: plan
                    .context_objects
                    .iter()
                    .filter(|context_object| {
                        context_object.owner_routine_id == routine.routine_id
                            || context_object
                                .declared_consumers
                                .iter()
                                .any(|consumer| consumer == &routine.routine_id)
                    })
                    .cloned()
                    .collect(),
                step_context_bindings: routine
                    .steps
                    .iter()
                    .map(|step| ProjectedStepContextBindings {
                        step_id: step.step_id.clone(),
                        context_reads: step.context_reads.clone(),
                        context_writes: step.context_writes.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub fn approved_plan_materialization(plan_package: &PlanPackage) -> ApprovedPlanMaterialization {
    let context_materialization = project_plan_context_materialization(plan_package);
    let mut step_count = 0usize;
    let mut routines = context_materialization
        .routines
        .iter()
        .map(|partition| {
            let step_ids = plan_package
                .routine_graph
                .iter()
                .find(|routine| routine.routine_id == partition.routine_id)
                .map(|routine| {
                    routine
                        .steps
                        .iter()
                        .map(|step| step.step_id.clone())
                        .collect()
                })
                .unwrap_or_else(Vec::new);
            step_count += step_ids.len();
            ApprovedPlanRoutineMaterialization {
                routine_id: partition.routine_id.clone(),
                step_ids,
                visible_context_object_ids: partition
                    .visible_context_objects
                    .iter()
                    .map(|context_object| context_object.context_object_id.clone())
                    .collect(),
                step_context_bindings: partition
                    .step_context_bindings
                    .iter()
                    .map(|binding| ApprovedPlanStepContextBinding {
                        step_id: binding.step_id.clone(),
                        context_reads: binding.context_reads.clone(),
                        context_writes: binding.context_writes.clone(),
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    routines.sort_by(|left, right| left.routine_id.cmp(&right.routine_id));
    ApprovedPlanMaterialization {
        plan_id: plan_package.plan_id.clone(),
        plan_revision: plan_package.plan_revision,
        lifecycle_state: plan_package.lifecycle_state,
        routine_count: routines.len(),
        step_count,
        context_object_count: plan_package.context_objects.len(),
        routines,
    }
}

pub fn approved_plan_success_memory_value(plan_package: &PlanPackage) -> Value {
    serde_json::to_value(approved_plan_materialization(plan_package)).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_projection::{
        ProjectedAutomationExecutionPolicy, ProjectedAutomationNode,
    };
    use crate::plan_package::{
        ApprovalMatrix, ApprovalMode, AuditScope, CommunicationModel, ContextObject,
        ContextObjectProvenance, ContextObjectScope, ContextValidationStatus,
        CrossRoutineVisibility, DataScope, DependencyResolution, DependencyResolutionStrategy,
        InterRoutinePolicy, IntermediateArtifactVisibility, MidRoutineConnectorFailureMode,
        MissionContextScope, MissionDefinition, PartialFailureMode, PeerVisibility,
        PlanLifecycleState, PlanOwner, ReentryPoint, RoutinePackage, RoutineSemanticKind,
        RunHistoryVisibility, StepPackage, SuccessCriteria, TriggerDefinition, TriggerKind,
    };
    use serde_json::json;

    #[test]
    fn materialization_seed_roundtrips_projection_shape() {
        let draft = ProjectedAutomationDraft {
            name: "Example".to_string(),
            description: Some("desc".to_string()),
            workspace_root: Some("/repo".to_string()),
            output_targets: vec!["notes.md".to_string()],
            agents: vec![ProjectedAutomationAgentProfile {
                agent_id: "agent_worker".to_string(),
                template_id: None,
                display_name: "Worker".to_string(),
                model_policy: None,
                tool_allowlist: vec!["*".to_string()],
                allowed_mcp_servers: vec!["github".to_string()],
            }],
            nodes: vec![ProjectedAutomationNode {
                node_id: "step_1".to_string(),
                agent_id: "agent_worker".to_string(),
                objective: "Do work".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::<serde_json::Value>::new(),
                output_contract: Some(json!({"kind": "structured_json"})),
                retry_policy: None,
                timeout_ms: None,
                stage_kind: None,
                gate: None,
                partial_failure_mode: Some(PartialFailureMode::PauseDownstreamOnly),
                metadata: Some(json!({"phase": "main"})),
            }],
            execution: ProjectedAutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            context: Some(ProjectedAutomationContextMaterialization {
                routines: vec![ProjectedRoutineContextPartition {
                    routine_id: "routine_a".to_string(),
                    visible_context_objects: Vec::new(),
                    step_context_bindings: vec![ProjectedStepContextBindings {
                        step_id: "step_1".to_string(),
                        context_reads: vec!["ctx:routine_a:mission.goal".to_string()],
                        context_writes: Vec::new(),
                    }],
                }],
            }),
            metadata: json!({"workflow_plan_id": "wfplan_1"}),
        };

        let seed = materialization_seed_from_projection(draft.clone());
        let roundtrip: ProjectedAutomationDraft<serde_json::Value, serde_json::Value> = seed.into();
        assert_eq!(roundtrip.name, draft.name);
        assert_eq!(roundtrip.nodes.len(), draft.nodes.len());
        assert_eq!(roundtrip.context, draft.context);
        assert_eq!(roundtrip.output_targets, draft.output_targets);
        assert_eq!(roundtrip.metadata, draft.metadata);
    }

    #[test]
    fn project_plan_context_materialization_partitions_by_routine() {
        let plan = PlanPackage {
            plan_id: "plan_1".to_string(),
            plan_revision: 1,
            lifecycle_state: PlanLifecycleState::Preview,
            owner: PlanOwner {
                owner_id: "owner".to_string(),
                scope: "workspace".to_string(),
                audience: "internal".to_string(),
            },
            mission: MissionDefinition {
                goal: "Goal".to_string(),
                summary: None,
                domain: None,
            },
            success_criteria: SuccessCriteria::default(),
            budget_policy: None,
            budget_enforcement: None,
            approval_policy: Some(ApprovalMatrix::default()),
            inter_routine_policy: Some(InterRoutinePolicy {
                communication_model: CommunicationModel::ArtifactOnly,
                shared_memory_access: false,
                shared_memory_justification: None,
                peer_visibility: PeerVisibility::DeclaredOutputsOnly,
                artifact_handoff_validation: true,
            }),
            trigger_policy: None,
            output_roots: None,
            precedence_log: Vec::new(),
            plan_diff: None,
            manual_trigger_record: None,
            validation_state: None,
            overlap_policy: None,
            routine_graph: vec![
                RoutinePackage {
                    routine_id: "routine_a".to_string(),
                    semantic_kind: RoutineSemanticKind::Mixed,
                    trigger: TriggerDefinition {
                        trigger_type: TriggerKind::Manual,
                        schedule: None,
                        timezone: None,
                    },
                    dependencies: Vec::new(),
                    dependency_resolution: DependencyResolution {
                        strategy: DependencyResolutionStrategy::TopologicalSequential,
                        partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                        reentry_point: ReentryPoint::FailedStep,
                        mid_routine_connector_failure:
                            MidRoutineConnectorFailureMode::SurfaceAndPause,
                    },
                    connector_resolution: Default::default(),
                    data_scope: DataScope {
                        readable_paths: vec!["mission.goal".to_string()],
                        writable_paths: vec!["knowledge/workflows/drafts/routine_a/**".to_string()],
                        denied_paths: vec!["credentials/**".to_string()],
                        cross_routine_visibility: CrossRoutineVisibility::DeclaredOutputsOnly,
                        mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                        mission_context_justification: None,
                    },
                    audit_scope: AuditScope {
                        run_history_visibility: RunHistoryVisibility::PlanOwner,
                        named_audit_roles: Vec::new(),
                        intermediate_artifact_visibility:
                            IntermediateArtifactVisibility::RoutineOnly,
                        final_artifact_visibility:
                            crate::plan_package::FinalArtifactVisibility::PlanOwner,
                    },
                    success_criteria: SuccessCriteria::default(),
                    steps: vec![StepPackage {
                        step_id: "step_a".to_string(),
                        label: "A".to_string(),
                        kind: "analysis".to_string(),
                        action: "A".to_string(),
                        inputs: Vec::new(),
                        outputs: Vec::new(),
                        dependencies: Vec::new(),
                        context_reads: vec!["ctx:routine_a:mission.goal".to_string()],
                        context_writes: vec!["ctx:routine_a:step_a:artifact.md".to_string()],
                        connector_requirements: Vec::new(),
                        model_policy: Default::default(),
                        approval_policy: ApprovalMode::InternalOnly,
                        success_criteria: SuccessCriteria::default(),
                        failure_policy: Default::default(),
                        retry_policy: Default::default(),
                        artifacts: vec!["artifact.md".to_string()],
                        provenance: None,
                        notes: None,
                    }],
                },
                RoutinePackage {
                    routine_id: "routine_b".to_string(),
                    semantic_kind: RoutineSemanticKind::Mixed,
                    trigger: TriggerDefinition {
                        trigger_type: TriggerKind::Manual,
                        schedule: None,
                        timezone: None,
                    },
                    dependencies: Vec::new(),
                    dependency_resolution: DependencyResolution {
                        strategy: DependencyResolutionStrategy::TopologicalSequential,
                        partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                        reentry_point: ReentryPoint::FailedStep,
                        mid_routine_connector_failure:
                            MidRoutineConnectorFailureMode::SurfaceAndPause,
                    },
                    connector_resolution: Default::default(),
                    data_scope: DataScope {
                        readable_paths: vec!["mission.goal".to_string()],
                        writable_paths: vec!["knowledge/workflows/drafts/routine_b/**".to_string()],
                        denied_paths: vec!["credentials/**".to_string()],
                        cross_routine_visibility: CrossRoutineVisibility::DeclaredOutputsOnly,
                        mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                        mission_context_justification: None,
                    },
                    audit_scope: AuditScope {
                        run_history_visibility: RunHistoryVisibility::PlanOwner,
                        named_audit_roles: Vec::new(),
                        intermediate_artifact_visibility:
                            IntermediateArtifactVisibility::RoutineOnly,
                        final_artifact_visibility:
                            crate::plan_package::FinalArtifactVisibility::PlanOwner,
                    },
                    success_criteria: SuccessCriteria::default(),
                    steps: vec![StepPackage {
                        step_id: "step_b".to_string(),
                        label: "B".to_string(),
                        kind: "analysis".to_string(),
                        action: "B".to_string(),
                        inputs: Vec::new(),
                        outputs: Vec::new(),
                        dependencies: Vec::new(),
                        context_reads: vec!["ctx:routine_a:step_a:artifact.md".to_string()],
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
                    }],
                },
            ],
            connector_intents: Vec::new(),
            connector_bindings: Vec::new(),
            connector_binding_resolution: None,
            model_routing_resolution: None,
            credential_envelopes: Vec::new(),
            context_objects: vec![ContextObject {
                context_object_id: "ctx:routine_a:step_a:artifact.md".to_string(),
                name: "handoff".to_string(),
                kind: "step_output_handoff".to_string(),
                scope: ContextObjectScope::Handoff,
                owner_routine_id: "routine_a".to_string(),
                producer_step_id: Some("step_a".to_string()),
                declared_consumers: vec!["routine_a".to_string(), "routine_b".to_string()],
                artifact_ref: Some("artifact.md".to_string()),
                data_scope_refs: vec!["knowledge/workflows/drafts/routine_a/**".to_string()],
                freshness_window_hours: None,
                validation_status: ContextValidationStatus::Pending,
                provenance: ContextObjectProvenance {
                    plan_id: "plan_1".to_string(),
                    routine_id: "routine_a".to_string(),
                    step_id: Some("step_a".to_string()),
                },
                summary: None,
            }],
            metadata: None,
        };

        let materialized = project_plan_context_materialization(&plan);

        assert_eq!(materialized.routines.len(), 2);
        assert_eq!(materialized.routines[0].step_context_bindings.len(), 1);
        assert_eq!(
            materialized.routines[1].visible_context_objects[0].context_object_id,
            "ctx:routine_a:step_a:artifact.md"
        );
    }

    #[test]
    fn approved_plan_materialization_summarizes_context_partitions() {
        let plan = crate::contracts::WorkflowPlanJson {
            plan_id: "plan_materialized".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "test".to_string(),
            original_prompt: "Compile a brief".to_string(),
            normalized_prompt: "compile a brief".to_string(),
            confidence: "high".to_string(),
            title: "Compile a brief".to_string(),
            description: Some("Preview plan".to_string()),
            schedule: crate::contracts::default_fallback_schedule_json(),
            execution_target: "automation_v2".to_string(),
            workspace_root: "/repo".to_string(),
            steps: vec![crate::contracts::WorkflowPlanStepJson {
                step_id: "draft_brief".to_string(),
                kind: "analysis".to_string(),
                objective: "Draft a brief".to_string(),
                depends_on: Vec::new(),
                agent_role: "worker".to_string(),
                input_refs: Vec::new(),
                output_contract: Some(json!({"kind": "report_markdown"})),
                metadata: None,
            }],
            requires_integrations: vec!["github".to_string()],
            allowed_mcp_servers: vec!["github".to_string()],
            operator_preferences: None,
            save_options: json!({}),
        };

        let mut package =
            crate::plan_package::compile_workflow_plan_preview_package(&plan, Some("owner"));
        package.plan_revision = 3;
        package.lifecycle_state = PlanLifecycleState::Approved;

        let materialized = approved_plan_materialization(&package);
        assert_eq!(materialized.plan_id, "plan_materialized");
        assert_eq!(materialized.plan_revision, 3);
        assert_eq!(materialized.lifecycle_state, PlanLifecycleState::Approved);
        assert_eq!(materialized.routine_count, 1);
        assert_eq!(materialized.step_count, 1);
        assert_eq!(
            materialized.context_object_count,
            package.context_objects.len()
        );
        assert_eq!(
            materialized.routines[0].routine_id,
            package.routine_graph[0].routine_id
        );
        assert_eq!(
            materialized.routines[0].step_ids,
            vec!["draft_brief".to_string()]
        );
        assert_eq!(
            approved_plan_success_memory_value(&package)
                .get("plan_revision")
                .and_then(Value::as_u64),
            Some(3)
        );
    }
}
