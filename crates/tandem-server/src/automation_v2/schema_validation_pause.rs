use serde_json::{json, Value};

use crate::automation_v2::types::{
    AutomationFailureRecord, AutomationFlowNode, AutomationRunStatus, AutomationV2RunRecord,
    AutomationV2Spec,
};
use crate::util::time::now_ms;

pub(crate) const SCHEMA_VALIDATION_PAUSE_REASON: &str = "schema_validation_failed";

#[derive(Debug, Clone)]
pub(crate) struct SchemaValidationPause {
    pub reason: String,
    pub metadata: Value,
}

pub(crate) fn schema_validation_pause_for_output(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    run_id: &str,
    output: &Value,
    attempt: u32,
    max_attempts: u32,
) -> Option<SchemaValidationPause> {
    if attempt >= max_attempts {
        return None;
    }
    if !output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("needs_repair"))
    {
        return None;
    }
    let artifact_validation = output.get("artifact_validation")?;
    if artifact_validation
        .get("repair_exhausted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    if !artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter()
                .any(|value| value.as_str() == Some("output_schema_invalid"))
        })
    {
        return None;
    }

    let schema = node.output_contract.as_ref()?.schema.as_ref()?;
    let reason = artifact_validation
        .get("rejected_artifact_reason")
        .or_else(|| artifact_validation.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| output.get("blocked_reason").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "node `{}` output did not match output_contract.schema",
                node.node_id
            )
        });
    let metadata = build_schema_validation_pause_metadata(
        automation,
        node,
        run_id,
        output,
        artifact_validation,
        schema,
        attempt,
        max_attempts,
        &reason,
    );
    Some(SchemaValidationPause { reason, metadata })
}

pub(crate) fn apply_schema_validation_pause(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    output: &Value,
    attempt: u32,
    max_attempts: u32,
) -> bool {
    let Some(pause) = schema_validation_pause_for_output(
        automation,
        node,
        &run.run_id,
        output,
        attempt,
        max_attempts,
    ) else {
        return false;
    };
    let node_id = node.node_id.clone();
    run.status = AutomationRunStatus::Paused;
    run.pause_reason = Some(SCHEMA_VALIDATION_PAUSE_REASON.to_string());
    run.stop_reason = None;
    run.detail = Some(format!(
        "node `{}` paused for schema validation repair: {}",
        node_id, pause.reason
    ));
    run.checkpoint.last_failure = Some(AutomationFailureRecord {
        node_id: node_id.clone(),
        reason: pause.reason.clone(),
        failed_at_ms: now_ms(),
        failure_kind: Some("output_schema_invalid".to_string()),
        metadata: Some(pause.metadata.clone()),
    });
    annotate_paused_output(run, &node_id, &pause.metadata);
    crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
        run,
        "node_schema_validation_paused",
        Some(format!(
            "node `{}` paused for schema validation repair",
            node_id
        )),
        None,
        Some(pause.metadata),
    );
    true
}

fn annotate_paused_output(run: &mut AutomationV2RunRecord, node_id: &str, metadata: &Value) {
    let Some(output) = run.checkpoint.node_outputs.get_mut(node_id) else {
        return;
    };
    let Some(object) = output.as_object_mut() else {
        return;
    };
    object.insert("paused_attention_required".to_string(), json!(true));
    object.insert("runtime_pause".to_string(), metadata.clone());
    if let Some(validation) = object
        .get_mut("artifact_validation")
        .and_then(Value::as_object_mut)
    {
        validation.insert(
            "runtime_state".to_string(),
            json!("paused_attention_required"),
        );
        validation.insert("runtime_pause".to_string(), metadata.clone());
    }
}

fn build_schema_validation_pause_metadata(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    run_id: &str,
    output: &Value,
    artifact_validation: &Value,
    schema: &Value,
    attempt: u32,
    max_attempts: u32,
    reason: &str,
) -> Value {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("unknown");
    json!({
        "version": 1,
        "runtime_state": "paused_attention_required",
        "pause_reason": SCHEMA_VALIDATION_PAUSE_REASON,
        "automation_id": automation.automation_id,
        "run_id": run_id,
        "node_id": node.node_id,
        "attempt": attempt,
        "max_attempts": max_attempts,
        "retry_exhausted": false,
        "failure_kind": "output_schema_invalid",
        "reason": reason,
        "expected_contract": {
            "kind": contract_kind,
            "schema": schema_display_summary(schema),
        },
        "actual": actual_output_digest(output, reason),
        "repair": {
            "repair_attempt": artifact_validation
                .get("repair_attempt")
                .and_then(Value::as_u64),
            "repair_attempts_remaining": artifact_validation
                .get("repair_attempts_remaining")
                .and_then(Value::as_u64),
            "repair_exhausted": false,
            "options": [
                {
                    "action": "resume_run",
                    "requires_status": "paused",
                    "node_id": node.node_id,
                },
                {
                    "action": "repair_node",
                    "requires_status": "paused",
                    "node_id": node.node_id,
                },
                {
                    "action": "retry_after_contract_fix",
                    "requires_status": "paused",
                    "node_id": node.node_id,
                }
            ],
        },
        "unmet_requirements": artifact_validation
            .get("unmet_requirements")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "required_next_tool_actions": artifact_validation
            .get("required_next_tool_actions")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "tenant_context": serde_json::to_value(automation.tenant_context())
            .unwrap_or(Value::Null),
    })
}

fn schema_display_summary(schema: &Value) -> Value {
    let encoded = serde_json::to_string(schema).unwrap_or_default();
    json!({
        "digest": format!("sha256:{}", crate::sha256_hex(&[encoded.as_str()])),
        "root_type": schema_type_label(schema),
        "required": schema_string_array(schema, "required"),
        "properties": schema_property_names(schema),
        "additional_properties_declared": schema.get("additionalProperties").is_some(),
    })
}

fn schema_type_label(schema: &Value) -> String {
    if schema.get("const").is_some() {
        return "const".to_string();
    }
    if schema.get("enum").is_some() {
        return "enum".to_string();
    }
    if let Some(value) = schema.get("type").and_then(Value::as_str) {
        return value.to_string();
    }
    if let Some(values) = schema.get("type").and_then(Value::as_array) {
        let mut labels = values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        labels.sort();
        labels.dedup();
        if !labels.is_empty() {
            return labels.join("|");
        }
    }
    if schema.get("oneOf").is_some() {
        return "oneOf".to_string();
    }
    if schema.get("anyOf").is_some() {
        return "anyOf".to_string();
    }
    "unknown".to_string()
}

fn schema_string_array(schema: &Value, key: &str) -> Vec<String> {
    let mut rows = schema
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    rows.sort();
    rows.dedup();
    rows
}

fn schema_property_names(schema: &Value) -> Vec<String> {
    let mut rows = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    rows.sort();
    rows
}

fn actual_output_digest(output: &Value, fallback: &str) -> Value {
    if let Some(digest) = output
        .pointer("/provenance/content_digest")
        .or_else(|| output.pointer("/attempt_evidence/artifact/content_digest"))
        .and_then(Value::as_str)
        .map(normalize_digest)
    {
        return json!({
            "digest": digest,
            "digest_source": "artifact_content",
            "raw_content_included": false,
        });
    }
    let summary = output
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    json!({
        "digest": format!("sha256:{}", crate::sha256_hex(&[summary])),
        "digest_source": "summary",
        "raw_content_included": false,
    })
}

fn normalize_digest(raw: &str) -> String {
    if raw.starts_with("sha256:") {
        raw.to_string()
    } else {
        format!("sha256:{raw}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_v2::types::{
        AutomationExecutionPolicy, AutomationFlowOutputContract, AutomationFlowSpec,
        AutomationRunCheckpoint, AutomationV2Schedule, AutomationV2ScheduleType,
        AutomationV2Status,
    };
    use crate::RoutineMisfirePolicy;
    use std::collections::HashMap;
    use tandem_types::TenantContext;

    fn test_node() -> AutomationFlowNode {
        AutomationFlowNode {
            node_id: "validate_json".to_string(),
            agent_id: "writer".to_string(),
            objective: "Write valid JSON".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "structured_json".to_string(),
                validator: None,
                enforcement: None,
                schema: Some(json!({
                    "type": "object",
                    "required": ["title"],
                    "properties": {
                        "title": { "type": "string" },
                        "status": { "enum": ["completed"] }
                    },
                    "additionalProperties": false
                })),
                summary_guidance: None,
            }),
            tool_policy: None,
            mcp_policy: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            wait: None,
            metadata: None,
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        }
    }

    fn test_automation(node: AutomationFlowNode) -> AutomationV2Spec {
        let tenant_context = TenantContext::explicit_user_workspace(
            "org-schema".to_string(),
            "workspace-schema".to_string(),
            Some("user-schema".to_string()),
            "test".to_string(),
        );
        let mut automation = AutomationV2Spec {
            automation_id: "automation-schema".to_string(),
            name: "Schema Pause".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: vec![node] },
            execution: AutomationExecutionPolicy {
                profile: None,
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
            workspace_root: None,
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        };
        automation.set_tenant_context(&tenant_context);
        automation
    }

    fn test_run(automation: &AutomationV2Spec) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: "run-schema".to_string(),
            automation_id: automation.automation_id.clone(),
            tenant_context: automation.tenant_context(),
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
                pending_nodes: vec!["validate_json".to_string()],
                node_outputs: HashMap::new(),
                node_attempts: HashMap::from([("validate_json".to_string(), 1)]),
                node_attempt_verdicts: HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: Some(automation.clone()),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
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
            effective_execution_profile:
                crate::automation_v2::execution_profile::ExecutionProfile::Strict,
            requested_execution_profile: None,
        }
    }

    fn schema_failure_output(repair_exhausted: bool) -> Value {
        json!({
            "node_id": "validate_json",
            "status": "needs_repair",
            "summary": "Artifact validation rejected node output.",
            "artifact_validation": {
                "validation_outcome": "needs_repair",
                "repair_attempt": 1,
                "repair_attempts_remaining": 1,
                "repair_exhausted": repair_exhausted,
                "unmet_requirements": ["output_schema_invalid"],
                "rejected_artifact_reason": "artifact does not match output_contract.schema: missing required property `title`",
                "required_next_tool_actions": ["Rewrite the JSON artifact with a title."]
            },
            "provenance": {
                "content_digest": "abcdef"
            }
        })
    }

    #[test]
    fn schema_validation_failure_pauses_with_structured_metadata() {
        let node = test_node();
        let automation = test_automation(node.clone());
        let mut run = test_run(&automation);
        let output = schema_failure_output(false);
        run.checkpoint
            .node_outputs
            .insert(node.node_id.clone(), output.clone());

        assert!(apply_schema_validation_pause(
            &mut run,
            &automation,
            &node,
            &output,
            1,
            3
        ));

        assert_eq!(run.status, AutomationRunStatus::Paused);
        assert_eq!(
            run.pause_reason.as_deref(),
            Some(SCHEMA_VALIDATION_PAUSE_REASON)
        );
        assert!(run.checkpoint.pending_nodes.contains(&node.node_id));
        let failure = run.checkpoint.last_failure.as_ref().expect("failure");
        assert_eq!(
            failure.failure_kind.as_deref(),
            Some("output_schema_invalid")
        );
        let metadata = failure.metadata.as_ref().expect("metadata");
        assert_eq!(
            metadata.pointer("/expected_contract/schema/root_type"),
            Some(&json!("object"))
        );
        assert_eq!(
            metadata.pointer("/actual/digest"),
            Some(&json!("sha256:abcdef"))
        );
        assert_eq!(
            metadata.pointer("/actual/raw_content_included"),
            Some(&json!(false))
        );
        assert_eq!(
            metadata.pointer("/repair/options/0/action"),
            Some(&json!("resume_run"))
        );
        let stored = run
            .checkpoint
            .node_outputs
            .get(&node.node_id)
            .expect("stored output");
        assert_eq!(
            stored.pointer("/artifact_validation/runtime_state"),
            Some(&json!("paused_attention_required"))
        );
        assert!(run
            .checkpoint
            .lifecycle_history
            .iter()
            .any(|entry| entry.event == "node_schema_validation_paused"));
    }

    #[test]
    fn exhausted_schema_validation_failure_does_not_pause() {
        let node = test_node();
        let automation = test_automation(node.clone());
        let mut run = test_run(&automation);
        let output = schema_failure_output(true);
        run.checkpoint
            .node_outputs
            .insert(node.node_id.clone(), output.clone());

        assert!(!apply_schema_validation_pause(
            &mut run,
            &automation,
            &node,
            &output,
            3,
            3
        ));

        assert_eq!(run.status, AutomationRunStatus::Running);
        assert!(run.checkpoint.last_failure.is_none());
    }

    #[test]
    fn schema_validation_pause_resume_keeps_repair_node_pending() {
        let node = test_node();
        let automation = test_automation(node.clone());
        let mut run = test_run(&automation);
        let output = schema_failure_output(false);
        run.checkpoint
            .node_outputs
            .insert(node.node_id.clone(), output.clone());
        apply_schema_validation_pause(&mut run, &automation, &node, &output, 1, 3);

        run.status = AutomationRunStatus::Queued;
        run.pause_reason = None;
        run.resume_reason = Some("operator repaired schema output".to_string());

        assert_eq!(run.status, AutomationRunStatus::Queued);
        assert_eq!(run.pause_reason, None);
        assert!(run.checkpoint.pending_nodes.contains(&node.node_id));
        assert_eq!(
            run.resume_reason.as_deref(),
            Some("operator repaired schema output")
        );
    }
}
