// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn promote_materialized_output(
    output: &mut Value,
    node: &crate::automation_v2::types::AutomationFlowNode,
    artifact_path: &str,
    artifact_text: &str,
    recovery_source: Option<&str>,
) {
    let accepted_candidate_source = if recovery_source.is_some() {
        "session_write_recovery"
    } else {
        "verified_output"
    };
    let content_digest = crate::sha256_hex(&[artifact_text]);
    let schema_issue = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.schema.as_ref())
        .and_then(|schema| {
            serde_json::from_str::<Value>(artifact_text)
                .map_err(|err| format!("artifact is not valid JSON: {err}"))
                .and_then(|artifact| {
                    crate::app::state::automation::automation_output_schema_validation_issue(
                        schema, &artifact,
                    )
                    .map(Err)
                    .unwrap_or(Ok(()))
                })
                .err()
        });
    let should_complete = matches!(
        node_output_status(output).as_str(),
        "blocked" | "needs_repair"
    ) && schema_issue.is_none()
        && output_only_failed_for_missing_materialized_artifact(output);

    if let Some(object) = output.as_object_mut() {
        object.insert(
            "summary".to_string(),
            json!(format!(
                "Verified workspace output `{}` for node `{}`.",
                artifact_path, node.node_id
            )),
        );
        if should_complete {
            object.insert(
                "status".to_string(),
                json!(if crate::app::state::automation_output_validator_kind(node)
                    == crate::AutomationOutputValidatorKind::CodePatch
                {
                    "done"
                } else {
                    "completed"
                }),
            );
            object.insert("blocked_reason".to_string(), Value::Null);
            object.insert("failure_kind".to_string(), Value::Null);
        }
        if let Some(issue) = schema_issue.as_ref() {
            object.insert("status".to_string(), json!("needs_repair"));
            object.insert(
                "blocked_reason".to_string(),
                json!(format!(
                    "artifact does not match output_contract.schema: {issue}"
                )),
            );
            object.insert("failure_kind".to_string(), json!("artifact_rejected"));
        }
    }

    let artifact_validation = output
        .as_object_mut()
        .and_then(|object| object.get_mut("artifact_validation"))
        .and_then(Value::as_object_mut);
    if let Some(artifact_validation) = artifact_validation {
        artifact_validation.insert(
            "accepted_candidate_source".to_string(),
            json!(accepted_candidate_source),
        );
        if let Some(issue) = schema_issue.as_ref() {
            let reason = format!("artifact does not match output_contract.schema: {issue}");
            artifact_validation.insert("rejected_artifact_reason".to_string(), json!(reason));
            artifact_validation.insert(
                "semantic_block_reason".to_string(),
                json!(format!(
                    "artifact does not match output_contract.schema: {issue}"
                )),
            );
            artifact_validation.insert("validation_outcome".to_string(), json!("needs_repair"));
            let unmet = artifact_validation
                .entry("unmet_requirements".to_string())
                .or_insert_with(|| json!([]));
            if let Some(rows) = unmet.as_array_mut() {
                if !rows
                    .iter()
                    .any(|value| value.as_str() == Some("output_schema_invalid"))
                {
                    rows.push(json!("output_schema_invalid"));
                }
            }
        } else {
            artifact_validation.insert("rejected_artifact_reason".to_string(), Value::Null);
        }
        if should_complete {
            artifact_validation.insert("semantic_block_reason".to_string(), Value::Null);
            artifact_validation.insert("unmet_requirements".to_string(), json!([]));
        }
        if let Some(validation_basis) = artifact_validation
            .entry("validation_basis".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
        {
            validation_basis.insert(
                "current_attempt_output_materialized".to_string(),
                json!(true),
            );
            validation_basis.insert(
                "current_attempt_output_materialized_via_filesystem".to_string(),
                json!(true),
            );
            validation_basis.insert("verified_output_materialized".to_string(), json!(true));
            validation_basis.insert("required_output_path".to_string(), json!(artifact_path));
        }
        if recovery_source.is_some() {
            artifact_validation.insert("artifact_recovered_from_session".to_string(), json!(true));
        }
    }

    let validator_summary = output
        .as_object_mut()
        .and_then(|object| object.get_mut("validator_summary"))
        .and_then(Value::as_object_mut);
    if let Some(validator_summary) = validator_summary {
        validator_summary.insert(
            "accepted_candidate_source".to_string(),
            json!(accepted_candidate_source),
        );
        if let Some(issue) = schema_issue.as_ref() {
            validator_summary.insert("outcome".to_string(), json!("needs_repair"));
            validator_summary.insert(
                "reason".to_string(),
                json!(format!(
                    "artifact does not match output_contract.schema: {issue}"
                )),
            );
            let unmet = validator_summary
                .entry("unmet_requirements".to_string())
                .or_insert_with(|| json!([]));
            if let Some(rows) = unmet.as_array_mut() {
                if !rows
                    .iter()
                    .any(|value| value.as_str() == Some("output_schema_invalid"))
                {
                    rows.push(json!("output_schema_invalid"));
                }
            }
        }
        if should_complete {
            validator_summary.insert(
                "outcome".to_string(),
                json!(if crate::app::state::automation_output_validator_kind(node)
                    == crate::AutomationOutputValidatorKind::CodePatch
                {
                    "done"
                } else {
                    "completed"
                }),
            );
            validator_summary.insert("reason".to_string(), Value::Null);
            validator_summary.insert("unmet_requirements".to_string(), json!([]));
        }
    }

    let attempt_artifact = output
        .as_object_mut()
        .and_then(|object| object.get_mut("attempt_evidence"))
        .and_then(|value| value.get_mut("artifact"))
        .and_then(Value::as_object_mut);
    if let Some(attempt_artifact) = attempt_artifact {
        attempt_artifact.insert("status".to_string(), json!("written"));
        attempt_artifact.insert("path".to_string(), json!(artifact_path));
        attempt_artifact.insert("content_digest".to_string(), json!(content_digest));
        attempt_artifact.insert(
            "accepted_candidate_source".to_string(),
            json!(accepted_candidate_source),
        );
        if let Some(recovery_source) = recovery_source {
            attempt_artifact.insert("recovery_source".to_string(), json!(recovery_source));
        }
    }
}
