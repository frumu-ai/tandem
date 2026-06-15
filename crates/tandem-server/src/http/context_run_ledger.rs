use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::audit::ProtectedAuditEnvelope;
use tandem_core::{
    build_fintech_audit_package, classify_fintech_tool, connector_proof_from_tool_record,
    ToolEffectLedgerPhase, ToolEffectLedgerRecord, ToolEffectLedgerStatus,
};
use tandem_types::{
    AccessDecision, AccessPermission, DataClass, PolicyDecisionEffect, PolicyDecisionRecord,
    ResourceRef, StrictTenantContext, TenantContext,
};

#[derive(Debug, Clone, serde::Serialize)]
struct ContextRunLedgerEventView {
    seq: u64,
    ts_ms: u64,
    event_id: String,
    record: ToolEffectLedgerRecord,
}

pub(super) async fn context_run_ledger(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let run = super::context_runs::load_context_run_state(&state, &run_id).await?;
    super::ensure_same_tenant(&tenant_context, &run.tenant_context)?;
    let events =
        load_context_run_ledger_source_events(&state, &run_id, query.since_seq, query.tail);
    let records = context_run_ledger_records(&events);
    Ok(Json(json!({
        "records": records,
        "summary": context_run_ledger_summary(&records),
    })))
}

pub(super) async fn context_run_governance_evidence_export(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::governance::premium_governance_required(&state)?;
    let context_run = super::context_runs::load_context_run_state(&state, &run_id)
        .await
        .map_err(governance_evidence_status_error)?;
    super::ensure_same_tenant(&tenant_context, &context_run.tenant_context)
        .map_err(governance_evidence_status_error)?;

    let automation_run_id = automation_run_id_from_context_run_id(&context_run.run_id);
    let automation_run = match automation_run_id.as_deref() {
        Some(run_id) => state.get_automation_v2_run(run_id).await,
        None => None,
    };
    if let Some(run) = automation_run.as_ref() {
        super::ensure_same_tenant(&tenant_context, &run.tenant_context)
            .map_err(governance_evidence_status_error)?;
    }

    let events = load_context_run_ledger_source_events(&state, &context_run.run_id, None, None);
    let records = context_run_ledger_records(&events);
    let run_ids = governance_evidence_run_ids(&context_run.run_id, automation_run.as_ref());
    let policy_decisions =
        load_governance_evidence_policy_decisions(&state, &tenant_context, &run_ids, &records)
            .await;
    let policy_decision_ids = policy_decisions
        .iter()
        .map(|decision| decision.decision_id.clone())
        .collect::<BTreeSet<_>>();
    let memory_audit =
        load_governance_evidence_memory_audit(&state, &tenant_context, &run_ids).await;
    let protected_audit = load_governance_evidence_protected_audit(
        &state,
        &tenant_context,
        &run_ids,
        &policy_decision_ids,
    )
    .await;
    // EAA-03 (TAN-28): the evidence package is a mixed-scope export (tool
    // ledger + policy decisions + memory/protected audit + artifacts).
    // Under a strict tenant projection the principal must hold explicit
    // read authority for the run's audit-export resource at every data
    // class included in the package; otherwise the whole export is
    // rejected. A silently incomplete evidence package would itself be an
    // integrity hazard, so this fails closed rather than filtering.
    // Local/default exports (no strict projection) are unchanged.
    if let Some(strict) = verified_tenant_context
        .as_ref()
        .and_then(|verified| verified.0.strict_projection.as_ref())
    {
        let export_resource_id = automation_run
            .as_ref()
            .map(|run| run.run_id.as_str())
            .unwrap_or(context_run.run_id.as_str());
        if let Some(denied) = governance_evidence_export_denial(
            strict,
            export_resource_id,
            &policy_decisions,
            automation_run.as_ref(),
            crate::now_ms(),
        ) {
            let _ = crate::audit::append_protected_audit_event(
                &state,
                "audit.export.denied",
                &tenant_context,
                Some(strict.principal.id.clone()),
                json!({
                    "runID": export_resource_id,
                    "resourceKind": "audit_export",
                    "dataClass": denied,
                    "reason": "missing read authority for included data class",
                }),
            )
            .await;
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": format!(
                        "export denied: principal lacks read authority for data class `{denied:?}` included in this evidence package"
                    ),
                    "code": "EXPORT_AUTHORITY_DENIED",
                    "data_class": denied,
                })),
            ));
        }
    }

    let package = governance_evidence_package_for_records(
        &context_run,
        automation_run.as_ref(),
        &records,
        &policy_decisions,
        &memory_audit,
        &protected_audit,
    );
    let content_sha256 = stable_json_sha256(&package);
    let filename = format!(
        "tandem-governance-evidence-{}.json",
        sanitize_filename(
            automation_run
                .as_ref()
                .map(|run| run.run_id.as_str())
                .unwrap_or(context_run.run_id.as_str())
        )
    );

    // EUAI-09 (TAN-250): record an audit-health event whenever the exported packet
    // is not fully complete, so an incomplete evidence export is itself auditable.
    let export_run_id = automation_run
        .as_ref()
        .map(|run| run.run_id.as_str())
        .unwrap_or(context_run.run_id.as_str());
    let export_principal = verified_tenant_context
        .as_ref()
        .and_then(|verified| verified.0.strict_projection.as_ref())
        .map(|strict| strict.principal.id.clone());
    emit_completeness_health_event(
        &state,
        &tenant_context,
        export_run_id,
        export_principal,
        &package["audit_completeness"],
    )
    .await;

    Ok(Json(json!({
        "evidence_package": package,
        "filename": filename,
        "content_sha256": content_sha256,
    })))
}

fn governance_evidence_status_error(status: StatusCode) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": status.canonical_reason().unwrap_or("request failed"),
        })),
    )
}

/// EAA-03: decide whether a strict principal may export the governance
/// evidence package for a run. The package's baseline content (tool-effect
/// ledger, memory/protected audit, artifacts) is classified `Internal`;
/// included policy decisions contribute their own data classes. Every
/// class must be explicitly readable on the run's `AuditExport` resource —
/// the same fail-closed `evaluate_access` grant evaluation used by
/// memory/search filtering. Returns the first denied data class.
fn governance_evidence_export_denial(
    strict: &tandem_types::StrictTenantContext,
    export_resource_id: &str,
    policy_decisions: &[PolicyDecisionRecord],
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
    now_ms: u64,
) -> Option<tandem_types::DataClass> {
    let mut required: Vec<tandem_types::DataClass> = vec![tandem_types::DataClass::Internal];
    for decision in policy_decisions {
        for data_class in &decision.data_classes {
            if !required.contains(data_class) {
                required.push(*data_class);
            }
        }
    }
    // The artifact section serializes per-node `data_class` metadata (and a
    // redacted output reference); those classes gate the export exactly like
    // policy-decision classes, mirroring governance_evidence_artifacts'
    // extraction keys.
    if let Some(run) = automation_run {
        for output in run.checkpoint.node_outputs.values() {
            let class_value = output
                .get("data_class")
                .or_else(|| output.get("enterprise_data_class"))
                .or_else(|| output.get("dataClass"));
            if let Some(data_class) = class_value.and_then(|value| {
                serde_json::from_value::<tandem_types::DataClass>(value.clone()).ok()
            }) {
                if !required.contains(&data_class) {
                    required.push(data_class);
                }
            }
        }
    }
    let resource = tandem_types::ResourceRef::new(
        strict.tenant_context.org_id.clone(),
        strict.tenant_context.workspace_id.clone(),
        tandem_types::ResourceKind::AuditExport,
        export_resource_id,
    );
    required.into_iter().find(|data_class| {
        strict
            .evaluate_access(
                &resource,
                tandem_types::AccessPermission::Read,
                *data_class,
                now_ms,
            )
            .decision
            != tandem_types::AccessDecision::Allow
    })
}

pub(super) fn context_run_ledger_summary_for_run(state: &AppState, run_id: &str) -> Value {
    let events = load_context_run_ledger_source_events(state, run_id, None, None);
    let records = context_run_ledger_records(&events);
    context_run_ledger_summary(&records)
}

pub(super) fn fintech_audit_package_for_automation_v2_run(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> Value {
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    let events = load_context_run_ledger_source_events(state, &context_run_id, None, None);
    let records = context_run_ledger_records(&events);
    fintech_audit_package_for_automation_v2_run_records_authorized(run, &records, None)
}

pub(super) async fn persist_fintech_audit_package_for_automation_v2_run(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> anyhow::Result<Value> {
    let package = fintech_audit_package_for_automation_v2_run(state, run);
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    let relative_path = "artifacts/fintech.audit_package.json";
    let path = super::context_runs::context_run_dir(state, &context_run_id).join(relative_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, serde_json::to_vec_pretty(&package)?).await?;
    Ok(json!({
        "context_run_id": context_run_id,
        "artifact_id": "fintech-audit-package",
        "artifact_type": "fintech_audit_package",
        "relative_path": relative_path,
        "path": path.to_string_lossy().to_string(),
        "package": package,
    }))
}

fn fintech_audit_package_for_automation_v2_run_records(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    records: &[ContextRunLedgerEventView],
) -> Value {
    fintech_audit_package_for_automation_v2_run_records_authorized(run, records, None)
}

fn fintech_audit_package_for_automation_v2_run_records_authorized(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    records: &[ContextRunLedgerEventView],
    strict_context: Option<&StrictTenantContext>,
) -> Value {
    let tool_calls = records
        .iter()
        .map(|record| record.record.clone())
        .collect::<Vec<_>>();
    let mut limitations = Vec::new();
    let artifacts = run
        .checkpoint
        .node_outputs
        .iter()
        .filter_map(|(node_id, output)| {
            match artifact_export_decision(node_id, output, strict_context) {
                ArtifactExportDecision::Include => Some(json!({
                "node_id": node_id,
                "output": output,
                })),
                ArtifactExportDecision::Exclude(reason) => {
                    limitations.push(reason);
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    let approvals = run
        .checkpoint
        .gate_history
        .iter()
        .map(|record| serde_json::to_value(record).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    let policy_decisions = records
        .iter()
        .filter(|record| record.record.status == ToolEffectLedgerStatus::Blocked)
        .map(|record| {
            json!({
                "event_id": record.event_id,
                "policy_decision_id": record.record.policy_decision_id,
                "tool": record.record.tool,
                "error": record.record.error,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_value(build_fintech_audit_package(
        run.run_id.clone(),
        serde_json::to_value(&run.tenant_context).unwrap_or(Value::Null),
        run.tenant_context.actor_id.clone(),
        tool_calls,
        artifacts,
        approvals,
        policy_decisions,
        limitations,
    ))
    .unwrap_or(Value::Null)
}

fn governance_evidence_package_for_records(
    context_run: &ContextRunState,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
    records: &[ContextRunLedgerEventView],
    policy_decisions: &[PolicyDecisionRecord],
    memory_audit: &[crate::MemoryAuditEvent],
    protected_audit: &[ProtectedAuditEnvelope],
) -> Value {
    let run_goal = automation_run
        .and_then(|run| {
            run.automation_snapshot.as_ref().and_then(|automation| {
                first_nonempty_str([
                    context_run.objective.as_str(),
                    automation.name.as_str(),
                    automation.description.as_deref().unwrap_or_default(),
                ])
            })
        })
        .unwrap_or(context_run.objective.as_str());
    let automation_status =
        automation_run.map(|run| serde_json::to_value(&run.status).unwrap_or(Value::Null));
    let policy_decision_ids = policy_decisions
        .iter()
        .map(|decision| decision.decision_id.clone())
        .chain(
            records
                .iter()
                .filter_map(|row| row.record.policy_decision_id.clone()),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let audit_event_ids = policy_decisions
        .iter()
        .filter_map(|decision| decision.audit_event_id.clone())
        .chain(protected_audit.iter().map(|event| event.event_id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let tool_calls = records
        .iter()
        .map(governance_evidence_tool_call)
        .collect::<Vec<_>>();
    let policy_decision_rows = policy_decisions
        .iter()
        .map(governance_evidence_policy_decision)
        .collect::<Vec<_>>();
    let memory_audit_rows = memory_audit
        .iter()
        .map(governance_evidence_memory_audit_row)
        .collect::<Vec<_>>();
    let memory_promotion_rows = memory_audit_rows
        .iter()
        .filter(|row| {
            row["action"]
                .as_str()
                .unwrap_or_default()
                .contains("promot")
        })
        .cloned()
        .collect::<Vec<_>>();
    let protected_audit_rows = protected_audit
        .iter()
        .map(governance_evidence_protected_audit_row)
        .collect::<Vec<_>>();
    let node_approval_ids = governance_evidence_node_approval_ids(policy_decisions);
    let (artifacts, artifact_limitations) = automation_run
        .map(|run| governance_evidence_artifacts(run, &node_approval_ids))
        .unwrap_or_else(|| (Vec::new(), Vec::new()));

    json!({
        "schema_version": 1,
        "package_type": "tandem_run_governance_evidence",
        "provenance": governance_evidence_provenance(context_run, automation_run),
        "run": {
            "run_id": automation_run
                .map(|run| run.run_id.as_str())
                .unwrap_or(context_run.run_id.as_str()),
            "context_run_id": context_run.run_id,
            "automation_v2_run_id": automation_run.map(|run| run.run_id.clone()),
            "automation_id": automation_run.map(|run| run.automation_id.clone()),
            "run_type": context_run.run_type,
            "goal": run_goal,
            "goal_sha256": crate::sha256_hex(&[run_goal]),
            "tenant_context": context_run.tenant_context,
            "trigger_type": automation_run.map(|run| run.trigger_type.clone()),
            "status": {
                "context": context_run.status,
                "automation": automation_status,
            },
            "timing": {
                "created_at_ms": context_run.created_at_ms,
                "started_at_ms": context_run.started_at_ms,
                "ended_at_ms": context_run.ended_at_ms,
                "updated_at_ms": context_run.updated_at_ms,
                "automation_created_at_ms": automation_run.map(|run| run.created_at_ms),
                "automation_started_at_ms": automation_run.and_then(|run| run.started_at_ms),
                "automation_finished_at_ms": automation_run.and_then(|run| run.finished_at_ms),
                "automation_updated_at_ms": automation_run.map(|run| run.updated_at_ms),
            },
            "counts": {
                "context_steps": context_run.steps.len(),
                "tool_calls": tool_calls.len(),
                "policy_decisions": policy_decision_rows.len(),
                "approval_records": automation_run.map(|run| run.checkpoint.gate_history.len()).unwrap_or(0),
                "memory_audit_records": memory_audit_rows.len(),
                "protected_audit_records": protected_audit_rows.len(),
                "artifacts": artifacts.len(),
            },
        },
        "actors": governance_evidence_actors(
            context_run,
            automation_run,
            policy_decisions,
            memory_audit,
        ),
        "tool_calls": tool_calls,
        "tool_call_summary": context_run_ledger_summary(records),
        "policy_decisions": policy_decision_rows,
        "approvals": governance_evidence_approvals(automation_run),
        "audit": {
            "policy_decision_ids": policy_decision_ids,
            "audit_event_ids": audit_event_ids,
            "protected_events": protected_audit_rows,
        },
        "audit_completeness": governance_evidence_completeness(
            context_run,
            automation_run,
            records,
            policy_decisions,
            protected_audit,
        ),
        "memory_promotions": memory_promotion_rows,
        "memory_audit": memory_audit_rows,
        "artifacts": artifacts,
        "final_outcome": governance_evidence_final_outcome(context_run, automation_run),
        "limitations": artifact_limitations,
        "redaction_policy": {
            "goal_included": true,
            "tool_arguments": "summaries_only",
            "tool_results": "summaries_and_hashes_only",
            "automation_node_outputs": "redacted_to_shape_safe_ids_and_sha256",
            "approval_instructions_and_reasons": "hashed_length_only",
            "memory_content": "omitted",
            "policy_metadata_and_protected_audit_payloads": "hashed_shape_only",
        },
    })
}

include!("context_run_ledger_parts/provenance.rs");
include!("context_run_ledger_parts/completeness.rs");

fn governance_evidence_tool_call(row: &ContextRunLedgerEventView) -> Value {
    json!({
        "seq": row.seq,
        "ts_ms": row.ts_ms,
        "event_id": row.event_id,
        "session_id": row.record.session_id,
        "message_id": row.record.message_id,
        "tool_call_id": row.record.tool_call_id,
        "tool": row.record.tool,
        "phase": row.record.phase,
        "status": row.record.status,
        "policy_decision_id": row.record.policy_decision_id,
        "args_summary": row.record.args_summary,
        "result_summary": row.record.result_summary,
        "error": redacted_text_ref(row.record.error.as_deref()),
        "connector_proof": connector_proof_from_tool_record(&row.record),
    })
}

fn governance_evidence_policy_decision(decision: &PolicyDecisionRecord) -> Value {
    json!({
        "decision_id": decision.decision_id,
        "actor_id": decision.actor_id,
        "session_id": decision.session_id,
        "message_id": decision.message_id,
        "run_id": decision.run_id,
        "automation_id": decision.automation_id,
        "node_id": decision.node_id,
        "tool": decision.tool,
        "resource": decision.resource,
        "data_classes": decision.data_classes,
        "risk_tier": decision.risk_tier,
        "decision": decision.decision,
        "reason_code": decision.reason_code,
        "reason": decision.reason,
        "policy_id": decision.policy_id,
        "grant_id": decision.grant_id,
        "approval_id": decision.approval_id,
        "audit_event_id": decision.audit_event_id,
        "created_at_ms": decision.created_at_ms,
        "metadata": redacted_value_ref(&decision.metadata),
    })
}

fn governance_evidence_memory_audit_row(event: &crate::MemoryAuditEvent) -> Value {
    json!({
        "audit_id": event.audit_id,
        "action": event.action,
        "run_id": event.run_id,
        "memory_id": event.memory_id,
        "source_memory_id": event.source_memory_id,
        "to_tier": event.to_tier,
        "partition_key": event.partition_key,
        "actor": event.actor,
        "status": event.status,
        "detail": redacted_text_ref(event.detail.as_deref()),
        "created_at_ms": event.created_at_ms,
    })
}

fn governance_evidence_protected_audit_row(event: &ProtectedAuditEnvelope) -> Value {
    json!({
        "event_id": event.event_id,
        "event_type": event.event_type,
        "durability": event.durability,
        "actor": event.actor,
        "created_at_ms": event.created_at_ms,
        "payload": redacted_value_ref(&event.payload),
    })
}

fn governance_evidence_approvals(
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
) -> Value {
    let Some(run) = automation_run else {
        return json!({
            "pending_gate": null,
            "gate_history": [],
        });
    };
    let pending_gate = run.checkpoint.awaiting_gate.as_ref().map(|gate| {
        json!({
            "node_id": gate.node_id,
            "title": gate.title,
            "instructions": redacted_text_ref(gate.instructions.as_deref()),
            "decisions": gate.decisions,
            "rework_targets": gate.rework_targets,
            "requested_at_ms": gate.requested_at_ms,
            "upstream_node_ids": gate.upstream_node_ids,
            "metadata": gate.metadata.as_ref().map(redacted_value_ref),
        })
    });
    let gate_history = run
        .checkpoint
        .gate_history
        .iter()
        .map(|record| {
            json!({
                "node_id": record.node_id,
                "decision": record.decision,
                "reason": redacted_text_ref(record.reason.as_deref()),
                "decided_at_ms": record.decided_at_ms,
                "decided_by": record.decided_by,
                "metadata": record.metadata.as_ref().map(redacted_value_ref),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "pending_gate": pending_gate,
        "gate_history": gate_history,
    })
}

fn governance_evidence_artifacts(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_approval_ids: &BTreeMap<String, String>,
) -> (Vec<Value>, Vec<String>) {
    let artifact_gate_coverage: BTreeMap<String, Vec<String>> = run
        .automation_snapshot
        .as_ref()
        .map(|spec| build_artifact_gate_coverage(spec))
        .unwrap_or_default();
    let mut limitations = Vec::new();
    let mut rows = run
        .checkpoint
        .node_outputs
        .iter()
        .map(|(node_id, output)| {
            if artifact_resource_target(output).is_some() {
                limitations.push(format!(
                    "artifact_payload_redacted_scoped_resource:{node_id}"
                ));
            }
            let (reviewer_state, transparency_label, review, matched_gate_id) =
                artifact_reviewer_state(
                    node_id,
                    &run.checkpoint.gate_history,
                    &artifact_gate_coverage,
                );
            // Prefer the approval_id from the gate that reviewed this artifact; fall back to
            // a direct policy decision keyed on the artifact's own node_id.
            let approval_id = matched_gate_id
                .as_deref()
                .and_then(|gid| node_approval_ids.get(gid))
                .or_else(|| node_approval_ids.get(node_id.as_str()));
            json!({
                "node_id": node_id,
                "artifact_id": first_string_field(output, &["artifact_id", "artifactID", "id"]),
                "resource_ref": output
                    .get("resource_ref")
                    .or_else(|| output.get("enterprise_resource_ref"))
                    .or_else(|| output.get("resourceRef"))
                    .cloned(),
                "data_class": output
                    .get("data_class")
                    .or_else(|| output.get("enterprise_data_class"))
                    .or_else(|| output.get("dataClass"))
                    .cloned(),
                "validation_outcome": output
                    .pointer("/artifact_validation/validation_outcome")
                    .or_else(|| output.pointer("/artifactValidation/validationOutcome"))
                    .cloned(),
                "provenance": {
                    "generation": "ai_generated",
                    "transparency_label": transparency_label,
                    "reviewer_state": reviewer_state,
                    "review": review,
                    "approval_id": approval_id,
                    "generated_at_ms": run.created_at_ms,
                },
                "output": redacted_value_ref(output),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        a["node_id"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["node_id"].as_str().unwrap_or_default())
    });
    limitations.sort();
    (rows, limitations)
}

fn governance_evidence_final_outcome(
    context_run: &ContextRunState,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
) -> Value {
    json!({
        "context_status": context_run.status,
        "automation_status": automation_run.map(|run| serde_json::to_value(&run.status).unwrap_or(Value::Null)),
        "completed_nodes": automation_run.map(|run| run.checkpoint.completed_nodes.clone()).unwrap_or_default(),
        "pending_nodes": automation_run.map(|run| run.checkpoint.pending_nodes.clone()).unwrap_or_default(),
        "blocked_nodes": automation_run.map(|run| run.checkpoint.blocked_nodes.clone()).unwrap_or_default(),
        "last_error": redacted_text_ref(context_run.last_error.as_deref()),
        "detail": redacted_text_ref(automation_run.and_then(|run| run.detail.as_deref())),
        "stop_kind": automation_run.and_then(|run| run.stop_kind.as_ref()).map(|kind| serde_json::to_value(kind).unwrap_or(Value::Null)),
        "stop_reason": redacted_text_ref(automation_run.and_then(|run| run.stop_reason.as_deref())),
    })
}

fn governance_evidence_actors(
    context_run: &ContextRunState,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
    policy_decisions: &[PolicyDecisionRecord],
    memory_audit: &[crate::MemoryAuditEvent],
) -> Value {
    let mut policy_actor_ids = BTreeSet::new();
    for decision in policy_decisions {
        if let Some(actor_id) = decision
            .actor_id
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            policy_actor_ids.insert(actor_id.to_string());
        }
    }
    let mut memory_actor_ids = BTreeSet::new();
    for event in memory_audit {
        if !event.actor.is_empty() {
            memory_actor_ids.insert(event.actor.clone());
        }
    }
    let approval_deciders = automation_run
        .map(|run| {
            run.checkpoint
                .gate_history
                .iter()
                .filter_map(|record| record.decided_by.as_ref())
                .filter_map(|actor| serde_json::to_value(actor).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "tenant_actor_id": context_run.tenant_context.actor_id,
        "automation_actor_id": automation_run.and_then(|run| run.tenant_context.actor_id.clone()),
        "policy_actor_ids": policy_actor_ids.into_iter().collect::<Vec<_>>(),
        "memory_actor_ids": memory_actor_ids.into_iter().collect::<Vec<_>>(),
        "approval_deciders": approval_deciders,
    })
}

async fn load_governance_evidence_policy_decisions(
    state: &AppState,
    tenant_context: &TenantContext,
    run_ids: &BTreeSet<String>,
    records: &[ContextRunLedgerEventView],
) -> Vec<PolicyDecisionRecord> {
    let mut rows = BTreeMap::<String, PolicyDecisionRecord>::new();
    for run_id in run_ids {
        for decision in state
            .list_policy_decisions_for_run(tenant_context, run_id, 500)
            .await
        {
            rows.insert(decision.decision_id.clone(), decision);
        }
    }
    for decision_id in records
        .iter()
        .filter_map(|row| row.record.policy_decision_id.as_deref())
    {
        let Some(decision) = state.get_policy_decision(decision_id).await else {
            continue;
        };
        if decision.tenant_context == *tenant_context {
            rows.insert(decision.decision_id.clone(), decision);
        }
    }
    let mut rows = rows.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then(a.decision_id.cmp(&b.decision_id))
    });
    rows
}

async fn load_governance_evidence_memory_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    run_ids: &BTreeSet<String>,
) -> Vec<crate::MemoryAuditEvent> {
    let mut rows = load_jsonl_rows::<crate::MemoryAuditEvent>(&state.memory_audit_path).await;
    if rows.is_empty() {
        rows = state.memory_audit_log.read().await.clone();
    }
    rows.retain(|event| event.tenant_context == *tenant_context && run_ids.contains(&event.run_id));
    rows.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then(a.audit_id.cmp(&b.audit_id))
    });
    rows
}

async fn load_governance_evidence_protected_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    run_ids: &BTreeSet<String>,
    policy_decision_ids: &BTreeSet<String>,
) -> Vec<ProtectedAuditEnvelope> {
    let mut rows =
        crate::audit::load_protected_audit_events_for_tenant(state, tenant_context).await;
    rows.retain(|event| {
        value_contains_any_string(&event.payload, run_ids)
            || value_contains_any_string(&event.payload, policy_decision_ids)
    });
    rows.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then(a.event_id.cmp(&b.event_id))
    });
    rows
}

async fn load_jsonl_rows<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Vec<T> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<T>(trimmed).ok()
        })
        .collect()
}

fn automation_run_id_from_context_run_id(context_run_id: &str) -> Option<String> {
    context_run_id
        .strip_prefix("automation-v2-")
        .map(str::to_string)
}

fn governance_evidence_run_ids(
    context_run_id: &str,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::from([context_run_id.to_string()]);
    if let Some(stripped) = context_run_id.strip_prefix("automation-v2-") {
        ids.insert(stripped.to_string());
    } else {
        ids.insert(format!("automation-v2-{context_run_id}"));
    }
    if let Some(run) = automation_run {
        ids.insert(run.run_id.clone());
        ids.insert(super::context_runs::automation_v2_context_run_id(
            &run.run_id,
        ));
    }
    ids
}

fn redacted_value_ref(value: &Value) -> Value {
    json!({
        "sha256": stable_json_sha256(value),
        "shape": evidence_value_shape(value),
        "redacted": true,
    })
}

fn redacted_text_ref(value: Option<&str>) -> Value {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Value::Null;
    };
    json!({
        "chars": value.chars().count(),
        "sha256": crate::sha256_hex(&[value]),
        "redacted": true,
    })
}

fn stable_json_sha256(value: &Value) -> String {
    let encoded = serde_json::to_string(value).unwrap_or_default();
    crate::sha256_hex(&[encoded.as_str()])
}

fn evidence_value_shape(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<BTreeSet<_>>();
            json!({
                "type": "object",
                "keys": keys.into_iter().collect::<Vec<_>>(),
                "field_count": map.len(),
            })
        }
        Value::Array(rows) => json!({
            "type": "array",
            "length": rows.len(),
        }),
        Value::String(text) => json!({
            "type": "string",
            "chars": text.chars().count(),
        }),
        Value::Number(_) => json!({ "type": "number" }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Null => json!({ "type": "null" }),
    }
}

fn value_contains_any_string(value: &Value, needles: &BTreeSet<String>) -> bool {
    if needles.is_empty() {
        return false;
    }
    match value {
        Value::String(text) => needles.contains(text),
        Value::Array(rows) => rows
            .iter()
            .any(|row| value_contains_any_string(row, needles)),
        Value::Object(map) => map
            .values()
            .any(|row| value_contains_any_string(row, needles)),
        Value::Number(_) | Value::Bool(_) | Value::Null => false,
    }
}

fn first_nonempty_str<const N: usize>(values: [&str; N]) -> Option<&str> {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn first_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_string))
}

fn sanitize_filename(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized
        .trim_matches('-')
        .chars()
        .take(96)
        .collect::<String>()
}

enum ArtifactExportDecision {
    Include,
    Exclude(String),
}

fn artifact_export_decision(
    node_id: &str,
    output: &Value,
    strict_context: Option<&StrictTenantContext>,
) -> ArtifactExportDecision {
    let Some((resource, data_class)) = artifact_resource_target(output) else {
        return ArtifactExportDecision::Include;
    };
    let Some(strict_context) = strict_context else {
        return ArtifactExportDecision::Exclude(format!(
            "artifact_excluded_missing_strict_projection:{node_id}"
        ));
    };
    let now_ms = crate::util::time::now_ms();
    let evaluation =
        strict_context.evaluate_access(&resource, AccessPermission::Read, data_class, now_ms);
    if evaluation.decision == AccessDecision::Allow {
        ArtifactExportDecision::Include
    } else {
        ArtifactExportDecision::Exclude(format!(
            "artifact_excluded_unauthorized:{node_id}:{}",
            evaluation.reason
        ))
    }
}

fn artifact_resource_target(output: &Value) -> Option<(ResourceRef, DataClass)> {
    let resource_value = output
        .get("resource_ref")
        .or_else(|| output.get("enterprise_resource_ref"))
        .or_else(|| output.get("resourceRef"))?;
    let data_class_value = output
        .get("data_class")
        .or_else(|| output.get("enterprise_data_class"))
        .or_else(|| output.get("dataClass"))?;
    let resource = serde_json::from_value(resource_value.clone()).ok()?;
    let data_class = serde_json::from_value(data_class_value.clone()).ok()?;
    Some((resource, data_class))
}

fn load_context_run_ledger_source_events(
    state: &AppState,
    run_id: &str,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Vec<ContextRunEventRecord> {
    super::context_runs::load_context_run_events_jsonl(
        &super::context_runs::context_run_events_path(state, run_id),
        since_seq,
        tail,
    )
}

fn context_run_ledger_records(events: &[ContextRunEventRecord]) -> Vec<ContextRunLedgerEventView> {
    events
        .iter()
        .filter_map(|event| {
            if event.event_type != "tool_effect_recorded" {
                return None;
            }
            let record =
                event.payload.get("record").cloned().and_then(|value| {
                    serde_json::from_value::<ToolEffectLedgerRecord>(value).ok()
                })?;
            Some(ContextRunLedgerEventView {
                seq: event.seq,
                ts_ms: event.ts_ms,
                event_id: event.event_id.clone(),
                record,
            })
        })
        .collect()
}

fn context_run_ledger_summary(records: &[ContextRunLedgerEventView]) -> Value {
    let mut by_status = BTreeMap::<String, u64>::new();
    let mut by_phase = BTreeMap::<String, u64>::new();
    let mut by_tool = BTreeMap::<String, u64>::new();

    for row in records {
        *by_status
            .entry(serde_json::to_string(&row.record.status).unwrap_or_default())
            .or_default() += 1;
        *by_phase
            .entry(serde_json::to_string(&row.record.phase).unwrap_or_default())
            .or_default() += 1;
        *by_tool.entry(row.record.tool.clone()).or_default() += 1;
    }

    let last_seq = records.last().map(|record| record.seq);
    let last_ts_ms = records.last().map(|record| record.ts_ms);
    let connector_proof = records
        .iter()
        .filter_map(|record| connector_proof_from_tool_record(&record.record))
        .collect::<Vec<_>>();

    json!({
        "record_count": records.len(),
        "by_status": normalize_serialized_enum_counts(by_status),
        "by_phase": normalize_serialized_enum_counts(by_phase),
        "by_tool": by_tool,
        "fintech_connector_proof": connector_proof,
        "last_seq": last_seq,
        "last_ts_ms": last_ts_ms,
    })
}

fn normalize_serialized_enum_counts(counts: BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    counts
        .into_iter()
        .map(|(key, value)| (key.trim_matches('"').to_string(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tandem_types::TenantContext;

    fn tool_effect_event(seq: u64, tool: &str, phase: &str, status: &str) -> ContextRunEventRecord {
        ContextRunEventRecord {
            event_id: format!("event-{seq}"),
            run_id: "run-1".to_string(),
            seq,
            ts_ms: seq * 10,
            event_type: "tool_effect_recorded".to_string(),
            status: ContextRunStatus::Running,
            revision: seq,
            step_id: Some("session-run".to_string()),
            task_id: None,
            command_id: None,
            payload: json!({
                "record": {
                    "session_id": "session-1",
                    "message_id": "message-1",
                    "tool": tool,
                    "phase": phase,
                    "status": status,
                    "args_summary": {"keys":["path"],"field_count":1,"type":"object"},
                }
            }),
        }
    }

    fn tool_effect_event_with_args(
        seq: u64,
        tool: &str,
        phase: &str,
        status: &str,
        args_summary: Value,
    ) -> ContextRunEventRecord {
        let mut event = tool_effect_event(seq, tool, phase, status);
        event.payload["record"]["args_summary"] = args_summary;
        event
    }

    #[test]
    fn context_run_ledger_filters_and_summarizes_records() {
        let records = context_run_ledger_records(&[
            tool_effect_event(1, "read", "invocation", "started"),
            ContextRunEventRecord {
                event_id: "event-2".to_string(),
                run_id: "run-1".to_string(),
                seq: 2,
                ts_ms: 20,
                event_type: "planning_started".to_string(),
                status: ContextRunStatus::Running,
                revision: 2,
                step_id: None,
                task_id: None,
                command_id: None,
                payload: json!({}),
            },
            tool_effect_event(3, "write", "outcome", "succeeded"),
        ]);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].record.tool, "read");
        assert_eq!(records[1].record.tool, "write");

        let summary = context_run_ledger_summary(&records);
        assert_eq!(summary["record_count"].as_u64(), Some(2));
        assert_eq!(summary["by_tool"]["read"].as_u64(), Some(1));
        assert_eq!(summary["by_tool"]["write"].as_u64(), Some(1));
        assert_eq!(summary["by_status"]["started"].as_u64(), Some(1));
        assert_eq!(summary["by_status"]["succeeded"].as_u64(), Some(1));
        assert_eq!(summary["last_seq"].as_u64(), Some(3));
    }

    #[test]
    fn context_run_ledger_summary_includes_fintech_connector_proof() {
        let records = context_run_ledger_records(&[
            tool_effect_event_with_args(
                1,
                "mcp.regulator.fetch_bulletin",
                "outcome",
                "succeeded",
                json!({
                    "keys": ["source_id"],
                    "field_count": 1,
                    "type": "object",
                    "source_id": "reg-bulletin-1"
                }),
            ),
            tool_effect_event_with_args(
                2,
                "mcp.regulator.list_tools",
                "outcome",
                "succeeded",
                json!({
                    "keys": ["query"],
                    "field_count": 1,
                    "type": "object",
                    "query_hash": "abc"
                }),
            ),
        ]);
        let summary = context_run_ledger_summary(&records);
        assert_eq!(
            summary["fintech_connector_proof"][0]["source_ids"][0].as_str(),
            Some("reg-bulletin-1")
        );
        assert_eq!(
            summary["fintech_connector_proof"].as_array().map(Vec::len),
            Some(1)
        );
    }

    fn fintech_audit_fixture_run() -> crate::automation_v2::types::AutomationV2RunRecord {
        crate::automation_v2::types::AutomationV2RunRecord {
            run_id: "automation-v2-run-fintech".to_string(),
            automation_id: "automation-fintech".to_string(),
            tenant_context: TenantContext::local_implicit(),
            trigger_type: "manual".to_string(),
            status: crate::AutomationRunStatus::Running,
            created_at_ms: 1,
            updated_at_ms: 2,
            started_at_ms: Some(1),
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: crate::AutomationRunCheckpoint {
                completed_nodes: vec!["draft_compliance_brief".to_string()],
                pending_nodes: Vec::new(),
                node_outputs: HashMap::from([(
                    "draft_compliance_brief".to_string(),
                    json!({
                        "artifact_id": "brief-1",
                        "artifact_validation": {
                            "validation_outcome": "passed",
                            "fintech_compliance_brief_validation": {"passed": true}
                        }
                    }),
                )]),
                node_attempts: HashMap::new(),
                node_attempt_verdicts: HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
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
            effective_execution_profile:
                crate::automation_v2::execution_profile::ExecutionProfile::Strict,
            requested_execution_profile: None,
        }
    }

    fn governance_evidence_context_run(
        run: &crate::automation_v2::types::AutomationV2RunRecord,
    ) -> ContextRunState {
        ContextRunState {
            run_id: super::super::context_runs::automation_v2_context_run_id(&run.run_id),
            run_type: "automation_v2".to_string(),
            tenant_context: run.tenant_context.clone(),
            source_client: None,
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: ContextRunStatus::Completed,
            objective: "Review PCI transfer evidence".to_string(),
            workspace: ContextWorkspaceLease {
                workspace_id: "workspace".to_string(),
                canonical_path: "/tmp/tandem".to_string(),
                lease_epoch: 1,
            },
            steps: vec![ContextRunStep {
                step_id: "step-1".to_string(),
                title: "Check policy gate".to_string(),
                status: ContextStepStatus::Done,
            }],
            tasks: Vec::new(),
            why_next_step: None,
            revision: 1,
            last_event_seq: 2,
            created_at_ms: 1,
            started_at_ms: Some(2),
            ended_at_ms: Some(50),
            last_error: None,
            updated_at_ms: 50,
        }
    }

    #[test]
    fn fintech_audit_package_fixture_includes_run_evidence() {
        let records = context_run_ledger_records(&[
            tool_effect_event_with_args(
                1,
                "mcp.regulator.fetch_bulletin",
                "outcome",
                "succeeded",
                json!({
                    "keys": ["source_id"],
                    "field_count": 1,
                    "type": "object",
                    "source_id": "reg-bulletin-1"
                }),
            ),
            tool_effect_event_with_args(
                2,
                "mcp.bank.release_funds",
                "outcome",
                "blocked",
                json!({
                    "keys": [],
                    "field_count": 0,
                    "type": "object"
                }),
            ),
        ]);
        let package = fintech_audit_package_for_automation_v2_run_records(
            &fintech_audit_fixture_run(),
            &records,
        );

        assert_eq!(package["run_id"], "automation-v2-run-fintech");
        assert_eq!(
            package["connector_proof"][0]["source_ids"][0].as_str(),
            Some("reg-bulletin-1")
        );
        assert_eq!(
            package["artifacts"][0]["node_id"].as_str(),
            Some("draft_compliance_brief")
        );
        assert_eq!(
            package["policy_decisions"][0]["tool"].as_str(),
            Some("mcp.bank.release_funds")
        );
    }

    #[test]
    fn governance_evidence_package_redacts_payloads_and_keeps_review_evidence() {
        let mut run = fintech_audit_fixture_run();
        run.status = crate::AutomationRunStatus::Blocked;
        run.finished_at_ms = Some(50);
        run.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
            node_id: "release_funds".to_string(),
            title: "Release funds".to_string(),
            instructions: Some("approval instructions secret".to_string()),
            decisions: vec!["approve".to_string(), "deny".to_string()],
            rework_targets: Vec::new(),
            requested_at_ms: 30,
            upstream_node_ids: vec!["draft_compliance_brief".to_string()],
            metadata: Some(json!({"secret": "pending gate metadata"})),
        });
        run.checkpoint
            .gate_history
            .push(crate::AutomationGateDecisionRecord {
                node_id: "release_funds".to_string(),
                decision: "denied".to_string(),
                reason: Some("operator reason secret".to_string()),
                decided_at_ms: 40,
                decided_by: None,
                metadata: Some(json!({"secret": "gate decision metadata"})),
            });
        run.checkpoint.node_outputs.insert(
            "release_funds".to_string(),
            json!({
                "artifact_id": "transfer-artifact",
                "payload": "sk-live-secret",
                "customer_note": "raw customer secret",
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "data_store",
                    "resource_id": "finance-ledger"
                },
                "data_class": "financial_record",
                "artifact_validation": {
                    "validation_outcome": "blocked",
                    "internal_detail": "validation raw secret"
                }
            }),
        );
        let context_run = governance_evidence_context_run(&run);
        let mut blocked = tool_effect_event_with_args(
            1,
            "mcp.bank.release_funds",
            "outcome",
            "blocked",
            json!({
                "keys": ["amount", "account_id"],
                "field_count": 2,
                "type": "object",
                "command_hash": "abc123"
            }),
        );
        blocked.payload["record"]["policy_decision_id"] = json!("decision-approval");
        blocked.payload["record"]["error"] = json!("tool error secret");
        let records = context_run_ledger_records(&[blocked]);
        let policy_decisions = vec![tandem_types::PolicyDecisionRecord {
            decision_id: "decision-approval".to_string(),
            tenant_context: run.tenant_context.clone(),
            actor_id: Some("finance-user".to_string()),
            session_id: Some("session-1".to_string()),
            message_id: Some("message-1".to_string()),
            run_id: Some(run.run_id.clone()),
            automation_id: Some(run.automation_id.clone()),
            node_id: Some("release_funds".to_string()),
            tool: Some("mcp.bank.release_funds".to_string()),
            resource: None,
            data_classes: vec![tandem_types::DataClass::FinancialRecord],
            risk_tier: Some("money_movement".to_string()),
            decision: tandem_types::PolicyDecisionEffect::ApprovalRequired,
            reason_code: "approval_required_unverified".to_string(),
            reason: "approval required".to_string(),
            policy_id: Some("fintech_strict".to_string()),
            grant_id: None,
            approval_id: Some("approval-release-funds".to_string()),
            audit_event_id: Some("audit-policy-1".to_string()),
            created_at_ms: 35,
            metadata: json!({"secret": "policy metadata secret"}),
        }];
        let memory_audit = vec![crate::MemoryAuditEvent {
            audit_id: "memory-audit-1".to_string(),
            action: "memory_promote".to_string(),
            run_id: run.run_id.clone(),
            tenant_context: run.tenant_context.clone(),
            memory_id: Some("memory-1".to_string()),
            source_memory_id: Some("source-memory-1".to_string()),
            to_tier: None,
            partition_key: "acme:finance:user".to_string(),
            actor: "finance-user".to_string(),
            status: "blocked".to_string(),
            detail: Some("memory detail secret".to_string()),
            created_at_ms: 45,
        }];
        let protected_audit = vec![ProtectedAuditEnvelope {
            event_id: "protected-audit-1".to_string(),
            durability: crate::audit::AuditDurability::DurableRequired,
            event_type: "approval.gate.approval_required".to_string(),
            tenant_context: run.tenant_context.clone(),
            actor: Some("finance-user".to_string()),
            payload: json!({
                "decision_id": "decision-approval",
                "raw_payload": "protected audit secret"
            }),
            created_at_ms: 36,
            seq: 0,
            prev_hash: None,
            record_hash: String::new(),
        }];

        let package = governance_evidence_package_for_records(
            &context_run,
            Some(&run),
            &records,
            &policy_decisions,
            &memory_audit,
            &protected_audit,
        );

        assert_eq!(package["schema_version"].as_u64(), Some(1));
        assert_eq!(
            package["policy_decisions"][0]["decision"].as_str(),
            Some("approval_required")
        );
        assert_eq!(
            package["approvals"]["gate_history"][0]["decision"].as_str(),
            Some("denied")
        );
        assert_eq!(
            package["memory_promotions"][0]["audit_id"].as_str(),
            Some("memory-audit-1")
        );
        assert_eq!(
            package["audit"]["protected_events"][0]["event_id"].as_str(),
            Some("protected-audit-1")
        );
        let serialized = serde_json::to_string(&package).expect("package json");
        for secret in [
            "sk-live-secret",
            "raw customer secret",
            "validation raw secret",
            "approval instructions secret",
            "operator reason secret",
            "policy metadata secret",
            "memory detail secret",
            "protected audit secret",
            "tool error secret",
        ] {
            assert!(
                !serialized.contains(secret),
                "governance evidence leaked `{secret}`"
            );
        }
        assert!(serialized.contains("decision-approval"));
        assert!(serialized.contains("protected-audit-1"));
        assert!(serialized.contains("sha256"));
        assert_eq!(stable_json_sha256(&package), stable_json_sha256(&package));
    }

    #[test]
    fn governance_evidence_package_includes_article_50_provenance() {
        let mut run = fintech_audit_fixture_run();
        run.status = crate::AutomationRunStatus::Completed;
        run.finished_at_ms = Some(60);

        // In the normal approval-gate flow the decision is recorded for the gate node
        // ("brief_approval_gate"), not the artifact node ("draft_compliance_brief").
        // The gate node's `depends_on` in the automation spec links it to the artifact.
        run.automation_snapshot = Some(crate::automation_v2::types::AutomationV2Spec {
            automation_id: run.automation_id.clone(),
            name: "compliance-automation".to_string(),
            description: None,
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            status: crate::automation_v2::types::AutomationV2Status::Active,
            schedule: crate::automation_v2::types::AutomationV2Schedule {
                schedule_type: crate::automation_v2::types::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::Skip,
            },
            agents: Vec::new(),
            flow: crate::automation_v2::types::AutomationFlowSpec {
                nodes: vec![
                    crate::automation_v2::types::AutomationFlowNode {
                        node_id: "draft_compliance_brief".to_string(),
                        agent_id: "writer".to_string(),
                        objective: "Write brief".to_string(),
                        depends_on: Vec::new(),
                        input_refs: Vec::new(),
                        output_contract: None,
                        tool_policy: None,
                        mcp_policy: None,
                        retry_policy: None,
                        timeout_ms: None,
                        max_tool_calls: None,
                        stage_kind: None,
                        gate: None,
                        metadata: None,
                        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    },
                    crate::automation_v2::types::AutomationFlowNode {
                        node_id: "brief_approval_gate".to_string(),
                        agent_id: "gate".to_string(),
                        objective: "Approve brief".to_string(),
                        depends_on: vec!["draft_compliance_brief".to_string()],
                        input_refs: Vec::new(),
                        output_contract: None,
                        tool_policy: None,
                        mcp_policy: None,
                        retry_policy: None,
                        timeout_ms: None,
                        max_tool_calls: None,
                        stage_kind: None,
                        gate: Some(crate::automation_v2::types::AutomationApprovalGate {
                            required: true,
                            decisions: vec!["approve".to_string(), "rework".to_string()],
                            rework_targets: Vec::new(),
                            instructions: None,
                        }),
                        metadata: None,
                        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    },
                ],
            },
            execution: crate::automation_v2::types::AutomationExecutionPolicy {
                profile: None,
                max_parallel_agents: None,
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
            creator_id: "tests".to_string(),
            workspace_root: None,
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        });
        // Gate decision recorded for the gate node; the artifact derives its state via coverage.
        run.checkpoint
            .gate_history
            .push(crate::AutomationGateDecisionRecord {
                node_id: "brief_approval_gate".to_string(),
                decision: "approved".to_string(),
                reason: None,
                decided_at_ms: 55,
                decided_by: None,
                metadata: None,
            });
        run.checkpoint.node_outputs.insert(
            "draft_compliance_brief".to_string(),
            json!({
                "artifact_id": "compliance-brief-1",
                "summary": "generated brief body",
            }),
        );

        let mut context_run = governance_evidence_context_run(&run);
        context_run.model_provider = Some("anthropic".to_string());
        context_run.model_id = Some("test-model".to_string());

        // Policy decision is keyed on the gate node_id, not the artifact node_id.
        let policy_decisions = vec![tandem_types::PolicyDecisionRecord {
            decision_id: "decision-1".to_string(),
            tenant_context: run.tenant_context.clone(),
            actor_id: Some("finance-user".to_string()),
            session_id: None,
            message_id: None,
            run_id: Some(run.run_id.clone()),
            automation_id: Some(run.automation_id.clone()),
            node_id: Some("brief_approval_gate".to_string()),
            tool: None,
            resource: None,
            data_classes: vec![tandem_types::DataClass::Internal],
            risk_tier: None,
            decision: tandem_types::PolicyDecisionEffect::ApprovalRequired,
            reason_code: "approval_required".to_string(),
            reason: "approval required".to_string(),
            policy_id: None,
            grant_id: None,
            approval_id: Some("approval-brief-1".to_string()),
            audit_event_id: None,
            created_at_ms: 50,
            metadata: json!({}),
        }];

        let package = governance_evidence_package_for_records(
            &context_run,
            Some(&run),
            &[],
            &policy_decisions,
            &[],
            &[],
        );

        // Top-level provenance block.
        let provenance = &package["provenance"];
        assert_eq!(provenance["generation"].as_str(), Some("ai_generated"));
        assert_eq!(
            provenance["transparency_label"].as_str(),
            Some("AI-Generated, approved")
        );
        assert_eq!(provenance["reviewer_state"].as_str(), Some("approved"));
        assert_eq!(provenance["model_provider"].as_str(), Some("anthropic"));
        assert_eq!(provenance["model_id"].as_str(), Some("test-model"));
        assert!(provenance["article_50_notice"]
            .as_str()
            .unwrap_or_default()
            .contains("AI system"));

        // Per-artifact provenance with reviewer state and approval linkage.
        let artifact = package["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .find(|row| row["node_id"].as_str() == Some("draft_compliance_brief"))
            .expect("brief artifact present");
        let artifact_provenance = &artifact["provenance"];
        assert_eq!(
            artifact_provenance["reviewer_state"].as_str(),
            Some("approved")
        );
        assert_eq!(
            artifact_provenance["transparency_label"].as_str(),
            Some("AI-Generated, approved")
        );
        assert_eq!(
            artifact_provenance["approval_id"].as_str(),
            Some("approval-brief-1")
        );
        assert_eq!(
            artifact_provenance["review"]["decision"].as_str(),
            Some("approved")
        );
    }

    #[test]
    fn fintech_audit_package_excludes_unauthorized_scoped_artifacts() {
        let mut run = fintech_audit_fixture_run();
        let finance_resource = tandem_types::ResourceRef::new(
            "acme",
            "finance",
            tandem_types::ResourceKind::DataStore,
            "finance-ledger",
        );
        let engineering_resource = tandem_types::ResourceRef::new(
            "acme",
            "engineering",
            tandem_types::ResourceKind::Repository,
            "product-api",
        );
        run.checkpoint.node_outputs = HashMap::from([
            (
                "finance_summary".to_string(),
                json!({
                    "artifact_id": "finance-summary",
                    "resource_ref": finance_resource,
                    "data_class": "financial_record",
                }),
            ),
            (
                "engineering_patch".to_string(),
                json!({
                    "artifact_id": "engineering-patch",
                    "resource_ref": engineering_resource,
                    "data_class": "source_code",
                }),
            ),
        ]);
        let strict_context = test_artifact_export_context(
            tandem_types::ResourceRef::new(
                "acme",
                "finance",
                tandem_types::ResourceKind::DataStore,
                "finance-ledger",
            ),
            tandem_types::DataClass::FinancialRecord,
        );

        let package = fintech_audit_package_for_automation_v2_run_records_authorized(
            &run,
            &[],
            Some(&strict_context),
        );

        let artifacts = package["artifacts"].as_array().expect("artifacts");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0]["node_id"].as_str(), Some("finance_summary"));
        assert!(
            package["limitations"]
                .as_array()
                .expect("limitations")
                .iter()
                .any(|row| row
                    .as_str()
                    .is_some_and(|value| value.contains("engineering_patch"))),
            "engineering scoped artifact should be excluded from the package"
        );
    }

    #[test]
    fn fintech_audit_package_excludes_scoped_artifacts_without_strict_projection() {
        let mut run = fintech_audit_fixture_run();
        run.checkpoint.node_outputs = HashMap::from([(
            "hr_compensation".to_string(),
            json!({
                "artifact_id": "hr-compensation",
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "hr",
                    "resource_kind": "document",
                    "resource_id": "compensation-bands"
                },
                "data_class": "financial_record"
            }),
        )]);

        let package =
            fintech_audit_package_for_automation_v2_run_records_authorized(&run, &[], None);

        assert_eq!(package["artifacts"].as_array().map(Vec::len), Some(0));
        assert!(
            package["limitations"]
                .as_array()
                .expect("limitations")
                .iter()
                .any(|row| row
                    .as_str()
                    .is_some_and(|value| value.contains("missing_strict_projection"))),
            "scoped artifacts should fail closed without strict projection"
        );
    }

    #[tokio::test]
    async fn persists_fintech_audit_package_to_context_run_artifact() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::new_starting("test".to_string(), true);
        state.shared_resources_path = root.path().join("system").join("shared.json");
        let run = fintech_audit_fixture_run();

        let receipt = persist_fintech_audit_package_for_automation_v2_run(&state, &run)
            .await
            .expect("persist package");
        let path = receipt["path"].as_str().expect("path");
        let raw = std::fs::read_to_string(path).expect("audit package file");
        let persisted: Value = serde_json::from_str(&raw).expect("package json");

        assert_eq!(receipt["artifact_id"], "fintech-audit-package");
        assert_eq!(persisted["run_id"], "automation-v2-run-fintech");
        assert_eq!(
            persisted["artifacts"][0]["node_id"].as_str(),
            Some("draft_compliance_brief")
        );
    }

    fn test_artifact_export_context(
        resource: tandem_types::ResourceRef,
        data_class: tandem_types::DataClass,
    ) -> tandem_types::StrictTenantContext {
        let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "acme",
            "finance",
            Some("deployment-test".to_string()),
            "finance-user",
        );
        let principal = tandem_types::PrincipalRef::human_user("finance-user");
        let grant = tandem_types::ScopedGrant::new(
            "grant-artifact-export",
            principal.clone(),
            resource.clone(),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(vec![tandem_types::AccessPermission::Read])
        .with_data_classes(vec![data_class]);
        tandem_types::StrictTenantContext::new(
            tenant_context,
            principal.clone(),
            tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(principal.id, "tandem-web"),
            ),
            tandem_types::ResourceScope::root(resource),
            tandem_types::AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-artifact-export",
            ),
        )
        .with_grants(vec![grant])
        .with_data_boundary(tandem_types::DataBoundary::allow(vec![data_class]))
    }

    include!("context_run_ledger_parts/completeness_tests.rs");
}

#[cfg(test)]
mod export_authority_tests {
    use super::*;

    fn strict_context_with_classes(
        run_id: &str,
        data_classes: Vec<tandem_types::DataClass>,
    ) -> tandem_types::StrictTenantContext {
        let resource = tandem_types::ResourceRef::new(
            "org-a",
            "workspace-a",
            tandem_types::ResourceKind::AuditExport,
            run_id,
        );
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "auditor-1");
        let principal = tandem_types::PrincipalRef::human_user("auditor-1");
        let grant = tandem_types::ScopedGrant::new(
            "grant-export",
            principal.clone(),
            resource.clone(),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(vec![tandem_types::AccessPermission::Read])
        .with_data_classes(data_classes);
        tandem_types::StrictTenantContext::new(
            tenant_context,
            principal.clone(),
            tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(principal.id, "tandem-web"),
            ),
            tandem_types::ResourceScope::root(resource),
            tandem_types::AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-export",
            ),
        )
        .with_grants(vec![grant])
    }

    fn decision_with_classes(data_classes: Vec<tandem_types::DataClass>) -> PolicyDecisionRecord {
        serde_json::from_value(json!({
            "decision_id": "decision-1",
            "tenant_context": TenantContext::local_implicit(),
            "data_classes": data_classes,
            "decision": "allow",
            "reason_code": "test",
            "reason": "test fixture",
            "created_at_ms": 1_500,
        }))
        .expect("policy decision fixture")
    }

    #[test]
    fn export_allowed_when_every_included_class_is_granted() {
        let strict = strict_context_with_classes(
            "run-1",
            vec![
                tandem_types::DataClass::Internal,
                tandem_types::DataClass::Restricted,
            ],
        );
        let decisions = vec![decision_with_classes(vec![
            tandem_types::DataClass::Restricted,
        ])];
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &decisions, None, 2_000),
            None
        );
    }

    #[test]
    fn export_rejected_when_a_policy_decision_class_is_not_granted() {
        // The principal can read Internal evidence but a included policy
        // decision carries Restricted data: the whole package is rejected,
        // so restricted data is never included in an unauthorized export.
        let strict = strict_context_with_classes("run-1", vec![tandem_types::DataClass::Internal]);
        let decisions = vec![decision_with_classes(vec![
            tandem_types::DataClass::Restricted,
        ])];
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &decisions, None, 2_000),
            Some(tandem_types::DataClass::Restricted)
        );
    }

    #[test]
    fn export_rejected_without_any_grant_for_the_run_resource() {
        // Grant is scoped to a different run: baseline Internal evidence is
        // already unreadable, fail closed.
        let strict =
            strict_context_with_classes("run-other", vec![tandem_types::DataClass::Internal]);
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &[], None, 2_000),
            Some(tandem_types::DataClass::Internal)
        );
    }

    #[test]
    fn export_rejected_when_an_artifact_class_is_not_granted() {
        // A node output carrying a Restricted artifact class gates the export
        // even when no policy decision carries that class (Codex P1 on
        // PR #1557): artifact metadata is serialized into the package, so
        // its classes must be readable too.
        let strict = strict_context_with_classes("run-1", vec![tandem_types::DataClass::Internal]);
        let mut run: crate::automation_v2::types::AutomationV2RunRecord =
            serde_json::from_value(json!({
                "run_id": "run-1",
                "automation_id": "auto-1",
                "tenant_context": TenantContext::local_implicit(),
                "trigger_type": "manual",
                "status": "completed",
                "created_at_ms": 1_500,
                "updated_at_ms": 1_500,
                "checkpoint": {},
            }))
            .expect("run fixture");
        run.checkpoint.node_outputs.insert(
            "export_step".to_string(),
            json!({
                "artifact_id": "artifact-1",
                "data_class": "restricted",
                "content": "redacted"
            }),
        );
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &[], Some(&run), 2_000),
            Some(tandem_types::DataClass::Restricted)
        );

        // With the Restricted grant the same package exports.
        let granted = strict_context_with_classes(
            "run-1",
            vec![
                tandem_types::DataClass::Internal,
                tandem_types::DataClass::Restricted,
            ],
        );
        assert_eq!(
            governance_evidence_export_denial(&granted, "run-1", &[], Some(&run), 2_000),
            None
        );
    }

    #[test]
    fn expired_assertion_fails_closed() {
        let strict = strict_context_with_classes("run-1", vec![tandem_types::DataClass::Internal]);
        // now_ms beyond the assertion expiry of the fixture
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &[], None, 99_999_999_999_999),
            Some(tandem_types::DataClass::Internal)
        );
    }
}
