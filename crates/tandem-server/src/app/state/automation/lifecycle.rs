use crate::automation_v2::types::{
    AutomationLifecycleRecord, AutomationStopKind, AutomationV2RunRecord,
};
use crate::util::time::now_ms;
use serde_json::{json, Value};

pub fn record_automation_lifecycle_event(
    run: &mut AutomationV2RunRecord,
    event: impl Into<String>,
    reason: Option<String>,
    stop_kind: Option<AutomationStopKind>,
) {
    record_automation_lifecycle_event_with_metadata(run, event, reason, stop_kind, None);
}

pub fn record_automation_lifecycle_event_with_metadata(
    run: &mut AutomationV2RunRecord,
    event: impl Into<String>,
    reason: Option<String>,
    stop_kind: Option<AutomationStopKind>,
    metadata: Option<Value>,
) {
    run.checkpoint
        .lifecycle_history
        .push(AutomationLifecycleRecord {
            event: event.into(),
            recorded_at_ms: now_ms(),
            reason,
            stop_kind,
            metadata,
        });
}

pub fn automation_last_activity_at_ms(run: &AutomationV2RunRecord) -> u64 {
    run.checkpoint
        .lifecycle_history
        .iter()
        .map(|record| record.recorded_at_ms)
        .max()
        .or(run.started_at_ms)
        .unwrap_or(run.created_at_ms)
}

pub fn automation_in_progress_node_ids(run: &AutomationV2RunRecord) -> Vec<String> {
    let mut in_progress = std::collections::HashSet::new();
    for record in &run.checkpoint.lifecycle_history {
        let Some(node_id) = record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("node_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        match record.event.as_str() {
            "node_started" => {
                in_progress.insert(node_id.to_string());
            }
            "node_completed"
            | "node_completed_with_warnings"
            | "node_blocked"
            | "node_repair_requested"
            | "node_verify_failed"
            | "node_failed"
            | "node_skipped_no_work"
            | "node_approval_rollback" => {
                in_progress.remove(node_id);
            }
            _ => {}
        }
    }
    let mut node_ids = in_progress.into_iter().collect::<Vec<_>>();
    node_ids.sort();
    node_ids
}

pub fn automation_lifecycle_event_metadata_for_node(
    node_id: &str,
    attempt: u32,
    session_id: Option<&str>,
    summary: &str,
    contract_kind: &str,
    workflow_class: &str,
    phase: &str,
    status: &str,
    failure_kind: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("node_id".to_string(), json!(node_id));
    map.insert("attempt".to_string(), json!(attempt));
    map.insert("summary".to_string(), json!(summary));
    map.insert("contract_kind".to_string(), json!(contract_kind));
    map.insert("workflow_class".to_string(), json!(workflow_class));
    map.insert("phase".to_string(), json!(phase));
    map.insert("status".to_string(), json!(status));
    map.insert("event_contract_version".to_string(), json!(1));
    if let Some(value) = session_id.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert("session_id".to_string(), json!(value));
    }
    if let Some(value) = failure_kind
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("failure_kind".to_string(), json!(value));
    }
    map
}

pub fn record_automation_workflow_state_events(
    run: &mut AutomationV2RunRecord,
    node_id: &str,
    output: &Value,
    attempt: u32,
    session_id: Option<&str>,
    summary: &str,
    contract_kind: &str,
) {
    let workflow_class = output
        .get("workflow_class")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("artifact");
    let phase = output
        .get("phase")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = output
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let failure_kind = output
        .get("failure_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let artifact_validation = output.get("artifact_validation");
    let base_reason = output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("rejected_artifact_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        });

    let mut base_metadata = automation_lifecycle_event_metadata_for_node(
        node_id,
        attempt,
        session_id,
        summary,
        contract_kind,
        workflow_class,
        phase,
        status,
        failure_kind,
    );
    if let Some(classification) = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        base_metadata.insert("blocking_classification".to_string(), json!(classification));
    }
    if let Some(actions) = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        base_metadata.insert(
            "required_next_tool_actions".to_string(),
            Value::Array(actions.clone()),
        );
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "workflow_state_changed",
        base_reason.clone(),
        None,
        Some(Value::Object(base_metadata.clone())),
    );

    if let Some(candidates) = artifact_validation
        .and_then(|value| value.get("artifact_candidates"))
        .and_then(Value::as_array)
    {
        for candidate in candidates {
            let mut metadata = base_metadata.clone();
            metadata.insert("candidate".to_string(), candidate.clone());
            record_automation_lifecycle_event_with_metadata(
                run,
                "artifact_candidate_written",
                None,
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(source) = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut metadata = base_metadata.clone();
        metadata.insert("accepted_candidate_source".to_string(), json!(source));
        record_automation_lifecycle_event_with_metadata(
            run,
            "artifact_accepted",
            None,
            None,
            Some(Value::Object(metadata)),
        );
    }

    if let Some(reason) = artifact_validation
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut metadata = base_metadata.clone();
        metadata.insert("rejected_artifact_reason".to_string(), json!(reason));
        record_automation_lifecycle_event_with_metadata(
            run,
            "artifact_rejected",
            Some(reason.to_string()),
            None,
            Some(Value::Object(metadata)),
        );
    }

    let repair_attempted = artifact_validation
        .and_then(|value| value.get("repair_attempted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let repair_succeeded = artifact_validation
        .and_then(|value| value.get("repair_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if repair_attempted {
        let mut metadata = base_metadata.clone();
        metadata.insert("repair_attempt".to_string(), json!(repair_attempt));
        metadata.insert(
            "repair_attempts_remaining".to_string(),
            json!(repair_attempts_remaining),
        );
        metadata.insert("repair_succeeded".to_string(), json!(repair_succeeded));
        metadata.insert("repair_exhausted".to_string(), json!(repair_exhausted));
        record_automation_lifecycle_event_with_metadata(
            run,
            "repair_started",
            None,
            None,
            Some(Value::Object(metadata.clone())),
        );
        if !repair_succeeded {
            record_automation_lifecycle_event_with_metadata(
                run,
                "repair_exhausted",
                base_reason.clone(),
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(unmet_requirements) = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        if workflow_class == "research" {
            let mut metadata = base_metadata.clone();
            metadata.insert(
                "unmet_requirements".to_string(),
                Value::Array(unmet_requirements.clone()),
            );
            record_automation_lifecycle_event_with_metadata(
                run,
                "research_coverage_failed",
                base_reason.clone(),
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(verification) = artifact_validation.and_then(|value| value.get("verification")) {
        let expected = verification
            .get("verification_expected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let ran = verification
            .get("verification_ran")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let failed = verification
            .get("verification_failed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if expected {
            let mut metadata = base_metadata.clone();
            metadata.insert("verification".to_string(), verification.clone());
            record_automation_lifecycle_event_with_metadata(
                run,
                "verification_started",
                None,
                None,
                Some(Value::Object(metadata.clone())),
            );
            if failed {
                record_automation_lifecycle_event_with_metadata(
                    run,
                    "verification_failed",
                    base_reason.clone(),
                    None,
                    Some(Value::Object(metadata)),
                );
            } else if ran {
                record_automation_lifecycle_event_with_metadata(
                    run,
                    "verification_passed",
                    None,
                    None,
                    Some(Value::Object(metadata)),
                );
            }
        }
    }
}
