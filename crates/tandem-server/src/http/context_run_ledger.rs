use std::collections::BTreeMap;

use super::*;
use tandem_core::{
    build_fintech_audit_package, connector_proof_from_tool_record, ToolEffectLedgerRecord,
    ToolEffectLedgerStatus,
};
use tandem_types::{AccessDecision, AccessPermission, DataClass, ResourceRef, StrictTenantContext};

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
}
