use super::*;
use serde_json::{json, Value};

pub(crate) fn automation_upstream_output_for_alias<'a>(
    upstream_inputs: &'a [Value],
    alias: &str,
) -> Option<&'a Value> {
    upstream_inputs
        .iter()
        .find(|input| input.get("alias").and_then(Value::as_str) == Some(alias))
        .and_then(|input| input.get("output"))
}

pub(crate) fn automation_upstream_structured_handoff<'a>(output: &'a Value) -> Option<&'a Value> {
    output
        .pointer("/content/structured_handoff")
        .or_else(|| output.get("structured_handoff"))
}

fn routine_id_for_node(run: &AutomationV2RunRecord, node: &AutomationFlowNode) -> Option<String> {
    let runtime_context = run.runtime_context.as_ref()?;
    runtime_context
        .routines
        .iter()
        .find(|partition| {
            partition
                .step_context_bindings
                .iter()
                .any(|binding| binding.step_id == node.node_id)
        })
        .map(|partition| partition.routine_id.clone())
}

fn runtime_context_partition_upstream_input(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> Option<Value> {
    let runtime_context = run.runtime_context.as_ref()?;
    let partitions = runtime_context
        .routines
        .iter()
        .filter(|partition| {
            partition
                .step_context_bindings
                .iter()
                .any(|binding| binding.step_id == node.node_id)
        })
        .map(|partition| {
            json!({
                "routine_id": partition.routine_id,
                "visible_context_object_ids": partition
                    .visible_context_objects
                    .iter()
                    .map(|context| context.context_object_id.clone())
                    .collect::<Vec<_>>(),
                "step_context_bindings": partition.step_context_bindings,
            })
        })
        .collect::<Vec<_>>();

    if partitions.is_empty() {
        return None;
    }

    Some(json!({
        "alias": "runtime_context_partition",
        "from_step_id": "runtime_context",
        "output": {
            "summary": format!(
                "compiler-derived runtime context partition for node `{}`",
                node.node_id
            ),
            "content": {
                "structured_handoff": {
                    "runtime_context_partition": {
                        "node_id": node.node_id,
                        "partitions": partitions,
                    }
                }
            }
        }
    }))
}

fn runtime_credential_envelope_upstream_input(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> Option<Value> {
    let routine_id = routine_id_for_node(run, node)?;
    let scope_snapshot = run
        .automation_snapshot
        .as_ref()?
        .plan_scope_snapshot_materialization()?;
    let credential_envelope = scope_snapshot
        .credential_envelopes
        .into_iter()
        .find(|envelope| envelope.routine_id == routine_id)?;

    Some(json!({
        "alias": "runtime_credential_envelope",
        "from_step_id": "runtime_credential_envelope",
        "output": {
            "summary": format!(
                "compiler-derived credential envelope for routine `{}`",
                routine_id
            ),
            "content": {
                "structured_handoff": {
                    "runtime_credential_envelope": {
                        "routine_id": routine_id,
                        "credential_envelope": credential_envelope,
                    }
                }
            }
        }
    }))
}

pub(crate) fn build_automation_v2_upstream_inputs(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> anyhow::Result<Vec<Value>> {
    let mut inputs = Vec::new();
    for input_ref in &node.input_refs {
        let Some(output) = run.checkpoint.node_outputs.get(&input_ref.from_step_id) else {
            anyhow::bail!(
                "missing upstream output for `{}` referenced by node `{}`",
                input_ref.from_step_id,
                node.node_id
            );
        };
        inputs.push(json!({
            "alias": input_ref.alias,
            "from_step_id": input_ref.from_step_id,
            "output": super::path_hygiene::normalize_upstream_research_output_paths(
                workspace_root,
                Some(&run.run_id),
                output
            ),
        }));
    }
    if let Some(runtime_context_input) = runtime_context_partition_upstream_input(run, node) {
        inputs.push(runtime_context_input);
    }
    if let Some(credential_envelope_input) = runtime_credential_envelope_upstream_input(run, node) {
        inputs.push(credential_envelope_input);
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tandem_plan_compiler::api::{
        ContextObject, ContextObjectProvenance, ContextObjectScope, ContextValidationStatus,
        ProjectedRoutineContextPartition, ProjectedStepContextBindings,
    };

    fn test_runtime_context() -> AutomationRuntimeContextMaterialization {
        AutomationRuntimeContextMaterialization {
            routines: vec![ProjectedRoutineContextPartition {
                routine_id: "routine_a".to_string(),
                visible_context_objects: vec![ContextObject {
                    context_object_id: "ctx:routine_a:mission.goal".to_string(),
                    name: "Mission goal".to_string(),
                    kind: "mission_goal".to_string(),
                    scope: ContextObjectScope::Mission,
                    owner_routine_id: "routine_a".to_string(),
                    producer_step_id: None,
                    declared_consumers: vec!["routine_a".to_string()],
                    artifact_ref: None,
                    data_scope_refs: vec!["mission.goal".to_string()],
                    freshness_window_hours: Some(12),
                    validation_status: ContextValidationStatus::Pending,
                    provenance: ContextObjectProvenance {
                        plan_id: "plan_1".to_string(),
                        routine_id: "routine_a".to_string(),
                        step_id: None,
                    },
                    summary: Some("Mission goal".to_string()),
                }],
                step_context_bindings: vec![ProjectedStepContextBindings {
                    step_id: "step_a".to_string(),
                    context_reads: vec!["ctx:routine_a:mission.goal".to_string()],
                    context_writes: vec!["ctx:routine_a:step_a:artifact.md".to_string()],
                }],
            }],
        }
    }

    fn test_run() -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: "run_1".to_string(),
            automation_id: "automation_1".to_string(),
            tenant_context: tandem_types::TenantContext::local_implicit(),
            trigger_type: "manual".to_string(),
            status: AutomationRunStatus::Running,
            created_at_ms: 1,
            updated_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: Vec::new(),
                node_outputs: HashMap::new(),
                node_attempts: HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: Some(test_runtime_context()),
            automation_snapshot: None,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
        }
    }

    fn test_run_with_snapshot() -> AutomationV2RunRecord {
        let mut run = test_run();
        run.automation_snapshot = Some(crate::AutomationV2Spec {
            automation_id: "automation_1".to_string(),
            name: "Automation".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: Vec::new(),
            flow: crate::AutomationFlowSpec { nodes: Vec::new() },
            execution: crate::AutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some("/workspace".to_string()),
            metadata: Some(json!({
                "plan_package_bundle": {
                    "scope_snapshot": {
                        "plan_id": "plan_1",
                        "plan_revision": 1,
                        "credential_envelopes": [
                            {
                                "routine_id": "routine_a",
                                "entitled_connectors": [
                                    {
                                        "capability": "read",
                                        "binding_id": "binding_a"
                                    }
                                ],
                                "denied_connectors": [],
                                "issuing_authority": "compiler"
                            }
                        ]
                    }
                }
            })),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        });
        run
    }

    #[test]
    fn build_upstream_inputs_appends_runtime_context_partition_for_node() {
        let run = test_run();
        let node = AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "step_a".to_string(),
            agent_id: "agent_a".to_string(),
            objective: "Do work".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: None,
        };

        let inputs = build_automation_v2_upstream_inputs(&run, &node, "/workspace")
            .expect("runtime context inputs");

        assert_eq!(inputs.len(), 1);
        let runtime_context_input = &inputs[0];
        assert_eq!(
            runtime_context_input.get("alias").and_then(Value::as_str),
            Some("runtime_context_partition")
        );
        assert_eq!(
            runtime_context_input
                .get("from_step_id")
                .and_then(Value::as_str),
            Some("runtime_context")
        );
        let structured_handoff = runtime_context_input
            .get("output")
            .and_then(|value| value.get("content"))
            .and_then(|value| value.get("structured_handoff"))
            .and_then(|value| value.get("runtime_context_partition"))
            .expect("runtime context partition handoff");
        assert_eq!(
            structured_handoff.get("node_id").and_then(Value::as_str),
            Some("step_a")
        );
        assert_eq!(
            structured_handoff
                .get("partitions")
                .and_then(Value::as_array)
                .and_then(|partitions| partitions.first())
                .and_then(|partition| partition.get("routine_id"))
                .and_then(Value::as_str),
            Some("routine_a")
        );
        assert_eq!(
            structured_handoff
                .get("partitions")
                .and_then(Value::as_array)
                .and_then(|partitions| partitions.first())
                .and_then(|partition| partition.get("visible_context_object_ids"))
                .and_then(Value::as_array)
                .and_then(|ids| ids.first())
                .and_then(Value::as_str),
            Some("ctx:routine_a:mission.goal")
        );
    }

    #[test]
    fn build_upstream_inputs_appends_runtime_credential_envelope_for_node() {
        let run = test_run_with_snapshot();
        let node = AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "step_a".to_string(),
            agent_id: "agent_a".to_string(),
            objective: "Do work".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: None,
        };

        let inputs = build_automation_v2_upstream_inputs(&run, &node, "/workspace")
            .expect("runtime credential envelope inputs");

        assert_eq!(inputs.len(), 2);
        let credential_envelope_input = &inputs[1];
        assert_eq!(
            credential_envelope_input
                .get("alias")
                .and_then(Value::as_str),
            Some("runtime_credential_envelope")
        );
        assert_eq!(
            credential_envelope_input
                .get("from_step_id")
                .and_then(Value::as_str),
            Some("runtime_credential_envelope")
        );
        let structured_handoff = credential_envelope_input
            .get("output")
            .and_then(|value| value.get("content"))
            .and_then(|value| value.get("structured_handoff"))
            .and_then(|value| value.get("runtime_credential_envelope"))
            .expect("runtime credential envelope handoff");
        assert_eq!(
            structured_handoff.get("routine_id").and_then(Value::as_str),
            Some("routine_a")
        );
        assert_eq!(
            structured_handoff
                .get("credential_envelope")
                .and_then(|value| value.get("routine_id"))
                .and_then(Value::as_str),
            Some("routine_a")
        );
        assert_eq!(
            structured_handoff
                .get("credential_envelope")
                .and_then(|value| value.get("entitled_connectors"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn normalize_upstream_research_output_paths_normalizes_source_material_entries() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-upstream-source-material-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        std::fs::write(
            workspace_root.join("RESUME.md"),
            "# Resume\n\nKeep the source text intact.\n",
        )
        .expect("write resume");

        let output = json!({
            "content": {
                "structured_handoff": {
                    "source_material": [
                        {
                            "path": workspace_root.join("RESUME.md").to_string_lossy().to_string(),
                            "content": "Keep the source text intact.",
                            "tool": "read"
                        }
                    ]
                }
            }
        });

        let normalized = normalize_upstream_research_output_paths(
            workspace_root.to_str().expect("workspace root string"),
            Some("run_1"),
            &output,
        );
        let source_material = normalized
            .pointer("/content/structured_handoff/source_material")
            .and_then(Value::as_array)
            .expect("source material");
        assert_eq!(
            source_material
                .first()
                .and_then(|value| value.get("path"))
                .and_then(Value::as_str),
            Some("RESUME.md")
        );
        assert_eq!(
            source_material
                .first()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("Keep the source text intact.")
        );

        let _ = std::fs::remove_dir_all(workspace_root);
    }
}
